//! AC0.10.1 / AC0.10.2: the real process, the real database. All `#[ignore]` (need GreptimeDB;
//! the outage test also needs the docker CLI): `make test-db`, `make outage-test`.
mod common;
use common::{db::*, ops::*};
use std::collections::HashSet;
use std::time::{Duration, Instant};

fn now_ms() -> i64 {
    api::now_ms()
}

/// Distinct `(session_id, seq)` pairs the database holds for the ops site.
async fn stored(db: &TestDb) -> (HashSet<(String, i64)>, i64) {
    let rows = db
        .rows(
            "SELECT session_id, CAST(seq AS BIGINT) AS seq FROM events WHERE site_id = ?",
            vec![greprum::query::Bind::Str(SITE.into())],
        )
        .await;
    let n = rows.len() as i64;
    (
        rows.iter()
            .map(|r| {
                (
                    r["session_id"].as_str().unwrap().to_string(),
                    r["seq"].as_i64().unwrap(),
                )
            })
            .collect(),
        n,
    )
}

async fn wait_for(
    db: &TestDb,
    want: &HashSet<(String, i64)>,
    secs: u64,
) -> (HashSet<(String, i64)>, i64) {
    let t = Instant::now();
    loop {
        let (got, n) = stored(db).await;
        if want.is_subset(&got) || t.elapsed() > Duration::from_secs(secs) {
            return (got, n);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

// ---- AC0.10.1 kill -9 -----------------------------------------------------------

#[tokio::test]
#[ignore]
async fn ac_0_10_1_a_kill9_during_ingest_loses_no_acknowledged_event() {
    let db = fresh().await;
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let mut g = start(dir.path(), port, &db.url).await;
    let mut acked = HashSet::new();
    let t0 = now_ms();
    let mut killed_at = 0;
    for i in 0..400 {
        let s = format!("a{i}");
        if post_batch(port, &s, 3, t0).await == Some(204) {
            acked.extend((0..3).map(|q| (s.clone(), q as i64)));
        }
        if i == 150 {
            killed_at = acked.len();
            g.kill9();
            break;
        }
    }
    assert!(
        killed_at > 100,
        "the gateway acknowledged traffic before the kill"
    );
    let mut g2 = start(dir.path(), port, &db.url).await;
    let (got, _) = wait_for(&db, &acked, 45).await;
    let missing: Vec<_> = acked.difference(&got).take(5).collect();
    assert!(missing.is_empty(), "acknowledged events lost: {missing:?}");
    g2.kill9();
}

#[tokio::test]
#[ignore]
async fn ac_0_10_1_b_kill9_during_drain_replays_from_the_checkpoint() {
    let db = fresh().await;
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let mut g = start(dir.path(), port, &db.url).await;
    let t0 = now_ms();
    let mut acked = HashSet::new();
    for i in 0..300 {
        let s = format!("b{i}");
        if post_batch(port, &s, 5, t0).await == Some(204) {
            acked.extend((0..5).map(|q| (s.clone(), q as i64)));
        }
    }
    // The drainer is working through the backlog (1 s lanes): kill it mid-way.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    g.kill9();
    let (partial, _) = stored(&db).await;
    let mut g2 = start(dir.path(), port, &db.url).await;
    let (got, n) = wait_for(&db, &acked, 45).await;
    assert!(
        acked.is_subset(&got),
        "lost {} events",
        acked.difference(&got).count()
    );
    // At-least-once: rows may exceed distinct pairs; the surplus is the measured duplicate rate.
    assert!(n >= got.len() as i64);
    println!(
        "partial before restart: {}, final distinct: {}, rows: {n}",
        partial.len(),
        got.len()
    );
    g2.kill9();
}

#[tokio::test]
#[ignore]
async fn ac_0_10_1_c_twenty_seeded_kill_restart_rounds() {
    use rand::{Rng, SeedableRng, rngs::StdRng};
    let db = fresh().await;
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let mut rng = StdRng::seed_from_u64(20261012);
    let mut acked = HashSet::new();
    let t0 = now_ms();
    for round in 0..20 {
        let mut g = start(dir.path(), port, &db.url).await;
        let n = rng.gen_range(5..60);
        for i in 0..n {
            let s = format!("r{round}_{i}");
            if post_batch(port, &s, 4, t0).await == Some(204) {
                acked.extend((0..4).map(|q| (s.clone(), q as i64)));
            }
        }
        tokio::time::sleep(Duration::from_millis(rng.gen_range(0..1500))).await;
        g.kill9();
    }
    let mut g = start(dir.path(), port, &db.url).await;
    let (got, n) = wait_for(&db, &acked, 90).await;
    assert!(!acked.is_empty());
    let lost = acked.difference(&got).count();
    assert_eq!(
        lost,
        0,
        "{lost} of {} acknowledged events lost over 20 kill -9 rounds",
        acked.len()
    );
    println!(
        "acked {} distinct, stored {} distinct in {n} rows ({} duplicates)",
        acked.len(),
        got.len(),
        n - got.len() as i64
    );
    g.kill9();
}

// ---- AC0.10.2 real outage -------------------------------------------------------

#[tokio::test]
#[ignore]
async fn ac_0_10_2_a_database_outage_loses_nothing_and_the_backlog_drains() {
    let secs: u64 = std::env::var("GREPRUM_OUTAGE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let container = std::env::var("GREPRUM_DB_CONTAINER").unwrap_or_else(|_| "greprum-db".into());
    let db = fresh().await;
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let mut g = start(dir.path(), port, &db.url).await;
    let docker = |verb: &str| {
        let ok = std::process::Command::new("docker")
            .args([verb, &container])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "docker {verb} {container}");
    };
    let t0 = now_ms();
    let mut acked = HashSet::new();
    let (mut non_204, mut sent, mut max_age) = (0usize, 0usize, 0.0f64);
    let mut statuses: std::collections::BTreeMap<String, usize> = Default::default();
    // 5 s healthy, `secs` s paused, 5 s healthy again; ~20 batches/s.
    let total = ((secs + 10) * 20) as usize;
    let pause_at = 100usize;
    let resume_at = pause_at + (secs as usize) * 20;
    for i in 0..total {
        if i == pause_at {
            docker("pause");
        }
        if i == resume_at {
            docker("unpause");
        }
        let s = format!("o{i}");
        sent += 1;
        let st = post_batch(port, &s, 3, t0).await;
        *statuses.entry(format!("{st:?}")).or_default() += 1;
        match st {
            Some(204) => acked.extend((0..3).map(|q| (s.clone(), q as i64))),
            _ => non_204 += 1,
        }
        if i % 40 == 0
            && let Some(a) = metric(port, "greprum_oldest_unwritten_event_age_ms").await
        {
            max_age = f64::max(max_age, a);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (got, n) = wait_for(&db, &acked, 180).await;
    let lost = acked.difference(&got).count();
    // After the backlog drains the age gauge returns to ~0.
    tokio::time::sleep(Duration::from_secs(5)).await;
    let age_after = metric(port, "greprum_oldest_unwritten_event_age_ms")
        .await
        .unwrap_or(-1.0);
    println!(
        "statuses={statuses:?}\n         outage={secs}s sent={sent} acked_events={} non_204={non_204} stored_distinct={} rows={n} lost={lost} \
         max_age_ms={max_age:.0} age_after_ms={age_after:.0}",
        acked.len(),
        got.len()
    );
    assert_eq!(
        non_204, 0,
        "503 only when the spool cap is reached (2 GB): never during a short outage"
    );
    assert_eq!(lost, 0, "accepted == written + counted drops");
    assert!(
        age_after <= 2000.0,
        "backlog drained, age gauge back to ~0: {age_after}"
    );
    g.kill9();
}

// ---- AC0.10.7 under load ---------------------------------------------------------

/// Sustained concurrent ingest on the real process: the age of the oldest unwritten event
/// (the PRD success metric) stays under 5 s and nothing acknowledged is lost.
#[tokio::test]
#[ignore]
async fn ac_0_10_7_b_sustained_load_keeps_oldest_unwritten_age_under_five_seconds() {
    let secs: u64 = std::env::var("GREPRUM_LOAD_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let workers: usize = std::env::var("GREPRUM_LOAD_WORKERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    // Sustained target: a very busy site. Override with GREPRUM_LOAD_EPS (events per second).
    let target_eps: f64 = std::env::var("GREPRUM_LOAD_EPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000.0);
    let db = fresh().await;
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let mut g = start(dir.path(), port, &db.url).await;
    let t0 = now_ms();
    let end = Instant::now() + Duration::from_secs(secs);
    let mut hs = vec![];
    for w in 0..workers {
        hs.push(tokio::spawn(async move {
            let mut acked = vec![];
            let mut i = 0u64;
            // Pace each worker to its share of the target rate.
            let gap = Duration::from_secs_f64(20.0 * workers as f64 / target_eps);
            while Instant::now() < end {
                let started = Instant::now();
                let s = format!("l{w}_{i}");
                if post_batch(port, &s, 20, t0).await == Some(204) {
                    acked.push(s);
                }
                i += 1;
                if let Some(rest) = gap.checked_sub(started.elapsed()) {
                    tokio::time::sleep(rest).await;
                }
            }
            acked
        }));
    }
    let mut max_age = 0.0f64;
    while Instant::now() < end {
        if let Some(a) = metric(port, "greprum_oldest_unwritten_event_age_ms").await {
            max_age = max_age.max(a);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let mut acked = HashSet::new();
    for h in hs {
        for s in h.await.unwrap() {
            acked.extend((0..20).map(|q| (s.clone(), q as i64)));
        }
    }
    let (got, n) = wait_for(&db, &acked, 120).await;
    let lost = acked.difference(&got).count();
    let rate = acked.len() as f64 / secs as f64;
    println!(
        "load: {workers} workers, {secs}s, acked {} events ({rate:.0}/s), stored {} distinct in {n} rows, lost {lost}, max_age_ms {max_age:.0}",
        acked.len(),
        got.len()
    );
    assert_eq!(lost, 0);
    assert!(
        max_age < 5000.0,
        "oldest unwritten event reached {max_age} ms"
    );
    g.kill9();
}

// ---- AC0.10.7 read-only database user --------------------------------------------

/// Needs a GreptimeDB with a read-only user (`scripts/ro-user-test.sh` starts a throwaway one):
/// GREPRUM_RW_URL = admin/writer URL (schema already migrated), GREPRUM_RO_URL = read-only user.
#[tokio::test]
#[ignore]
async fn ac_0_10_7_a_read_only_user_can_read_but_cannot_insert_delete_or_drop() {
    use greprum::{
        query::{Q, Reader, SqlxReader},
        sink::{Sink, SinkError, SqlxSink},
    };
    let (Ok(rw), Ok(ro)) = (
        std::env::var("GREPRUM_RW_URL"),
        std::env::var("GREPRUM_RO_URL"),
    ) else {
        panic!("set GREPRUM_RW_URL and GREPRUM_RO_URL (see scripts/ro-user-test.sh)");
    };
    let q = |sql: &str| Q {
        name: "t",
        sql: sql.into(),
        binds: vec![],
    };
    let admin = SqlxReader::connect(&rw).await.unwrap();
    let reader = SqlxReader::connect(&ro)
        .await
        .expect("read-only user can connect");
    // reads work
    admin
        .fetch(&q("SELECT count(*) AS n FROM events"))
        .await
        .expect("admin read");
    reader
        .fetch(&q("SELECT count(*) AS n FROM events"))
        .await
        .expect("ro read");
    // the gateway's own write path is refused for the read-only user
    let sink = SqlxSink::connect(&ro).await.unwrap();
    let ev = greprum::model::StoredEvent {
        site_id: "ro".into(),
        event_name: "$pageview".into(),
        ts: now_ms(),
        ..Default::default()
    };
    let r = sink.write(&[ev]).await;
    assert!(
        matches!(
            r,
            Err(SinkError::Permanent(_)) | Err(SinkError::Transient(_))
        ),
        "insert must fail: {r:?}"
    );
    // DDL and deletes are refused as well
    assert!(
        reader.fetch(&q("DROP TABLE events")).await.is_err(),
        "drop must fail"
    );
    assert!(
        reader
            .fetch(&q("DELETE FROM first_seen WHERE site_id = 'x'"))
            .await
            .is_err(),
        "delete must fail"
    );
    // and nothing was written
    let n = admin
        .fetch(&q("SELECT count(*) AS n FROM events WHERE site_id = 'ro'"))
        .await
        .unwrap();
    assert_eq!(n[0]["n"].as_i64(), Some(0));
}

// ---- AC0.10.2 evidence is recorded and reproducible ----------------------------------

fn repo(p: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(p))
        .unwrap_or_else(|e| panic!("{p}: {e}"))
}

#[test]
fn ac_0_10_2_b_results_of_the_real_outage_run_are_recorded_in_an_adr() {
    let a = repo("docs/adr/0049-real-database-outage-results.md");
    assert!(a.contains("Status: Accepted"));
    assert!(
        a.contains("12 200 batches") && a.contains("0 lost"),
        "numbers from the 600 s run"
    );
    assert!(a.contains("600 s"));
}

#[test]
fn ac_0_10_2_c_make_target_exists_and_defaults_to_ten_minutes() {
    let m = repo("Makefile");
    assert!(m.contains("outage-test:") && m.contains("GREPRUM_OUTAGE_SECS=$(or $(SECS),600)"));
    let t = repo("tests/ac_ops.rs");
    assert!(
        t.contains("\"pause\"") && t.contains("\"unpause\""),
        "the test drives docker pause/unpause"
    );
}
