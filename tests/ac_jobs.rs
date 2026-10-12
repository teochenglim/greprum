//! AC0.8.x: scheduled jobs. Unit-level with a fake runner; live tests need GreptimeDB.
mod common;
use async_trait::async_trait;
use greprum::{
    jobs::{JobName, JobRunner, Scheduler, reconcile::Report, run_once},
    metrics::Metrics,
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
    fail_first_seen: Mutex<bool>,
    report: Mutex<Option<Report>>,
    last_shed: Mutex<i64>,
}

#[async_trait]
impl JobRunner for Fake {
    async fn first_seen(&self, site: &str, since: i64) -> anyhow::Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("first_seen:{site}:{since}"));
        if *self.fail_first_seen.lock().unwrap() {
            anyhow::bail!("db down");
        }
        Ok(())
    }

    async fn reconcile(&self, site: &str, from: i64, to: i64, shed: i64) -> anyhow::Result<Report> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("reconcile:{site}:{from}:{to}"));
        *self.last_shed.lock().unwrap() = shed;
        Ok(self.report.lock().unwrap().unwrap_or(Report {
            expected: 0,
            received: 0,
            missing: 0,
            loss: 0.0,
            duplicate_rate: 0.0,
        }))
    }
}

fn sched(f: &Arc<Fake>, m: &Arc<Metrics>) -> Scheduler {
    Scheduler::new(
        f.clone(),
        vec!["a".into(), "b".into()],
        3600,
        3600,
        m.clone(),
    )
}

const H: i64 = 3_600_000;
const T: i64 = 1_760_000_000_000;

// ---- AC0.8.1 first-seen scheduling -----------------------------------------

#[tokio::test]
async fn ac_0_8_1_a_tick_runs_first_seen_once_per_site() {
    let f = Arc::new(Fake::default());
    let m = Arc::new(Metrics::default());
    let s = sched(&f, &m);
    let r = s.tick(T).await;
    assert_eq!(
        r.ran.iter().filter(|x| x.starts_with("first_seen")).count(),
        2
    );
    let calls = f.calls.lock().unwrap().clone();
    assert!(
        calls.contains(&"first_seen:a:0".to_string()),
        "first run scans everything: {calls:?}"
    );
    assert!(calls.contains(&"first_seen:b:0".to_string()));
}

#[tokio::test]
async fn ac_0_8_1_b_interval_honoured_and_later_runs_scan_two_days() {
    let f = Arc::new(Fake::default());
    let m = Arc::new(Metrics::default());
    let s = sched(&f, &m);
    s.tick(T).await;
    let n = f.calls.lock().unwrap().len();
    s.tick(T + H - 1).await;
    assert_eq!(f.calls.lock().unwrap().len(), n, "not due yet");
    s.tick(T + H).await;
    let calls = f.calls.lock().unwrap().clone();
    assert!(calls.len() > n);
    assert!(calls.contains(&format!("first_seen:a:{}", T + H - 2 * 86_400_000)));
}

#[tokio::test]
async fn ac_0_8_1_c_failure_is_counted_and_does_not_stop_the_loop() {
    let f = Arc::new(Fake::default());
    *f.fail_first_seen.lock().unwrap() = true;
    let m = Arc::new(Metrics::default());
    let s = sched(&f, &m);
    let r = s.tick(T).await;
    assert_eq!(
        r.failed
            .iter()
            .filter(|x| x.starts_with("first_seen"))
            .count(),
        2
    );
    assert!(
        r.ran.iter().any(|x| x.starts_with("reconcile")),
        "other jobs still ran"
    );
    assert_eq!(m.label_value("job_failures_total", "first_seen"), 2);
    // not retried in a tight loop: next attempt after the interval
    let n = f.calls.lock().unwrap().len();
    s.tick(T + 1000).await;
    assert_eq!(f.calls.lock().unwrap().len(), n);
    *f.fail_first_seen.lock().unwrap() = false;
    let r = s.tick(T + H).await;
    assert!(r.failed.is_empty());
}

// ---- AC0.8.2 reconcile ------------------------------------------------------

#[tokio::test]
async fn ac_0_8_2_a_gauges_are_set_from_the_report() {
    let f = Arc::new(Fake::default());
    *f.report.lock().unwrap() = Some(Report {
        expected: 100,
        received: 98,
        missing: 2,
        loss: 0.02,
        duplicate_rate: 0.005,
    });
    let m = Arc::new(Metrics::default());
    sched(&f, &m).tick(T).await;
    let t = m.render();
    assert!(t.contains("greprum_browser_loss_estimate_ppm 20000"), "{t}");
    assert!(t.contains("greprum_duplicate_rate_ppm 5000"));
    assert!(t.contains("greprum_site_browser_loss_ppm{site=\"a\"} 20000"));
}

#[tokio::test]
async fn ac_0_8_2_b_range_is_the_last_full_hour_and_shed_since_last_run_is_subtracted() {
    let f = Arc::new(Fake::default());
    let m = Arc::new(Metrics::default());
    let s = Scheduler::new(f.clone(), vec!["a".into()], 3600, 3600, m.clone());
    m.inc_label("shed_total", "p2", 7);
    m.inc_label("shed_total", "p1", 3);
    let now = (T / H) * H + 1234;
    s.tick(now).await;
    let calls = f.calls.lock().unwrap().clone();
    let rc = calls.iter().find(|c| c.starts_with("reconcile")).unwrap();
    assert_eq!(
        rc,
        &format!("reconcile:a:{}:{}", (T / H) * H - H, (T / H) * H)
    );
    assert_eq!(*f.last_shed.lock().unwrap(), 10);
    m.inc_label("shed_total", "p2", 4);
    s.tick(now + H).await;
    assert_eq!(
        *f.last_shed.lock().unwrap(),
        4,
        "only the delta since the previous run"
    );
}

#[tokio::test]
async fn ac_0_8_2_c_quiet_hour_leaves_gauges_at_zero() {
    let f = Arc::new(Fake::default());
    let m = Arc::new(Metrics::default());
    sched(&f, &m).tick(T).await;
    assert!(m.render().contains("greprum_browser_loss_estimate_ppm 0"));
    assert!(m.render().contains("greprum_duplicate_rate_ppm 0"));
}

#[tokio::test]
async fn ac_0_8_2_d_job_metrics_expose_runs_failures_and_last_run() {
    let f = Arc::new(Fake::default());
    let m = Arc::new(Metrics::default());
    sched(&f, &m).tick(T).await;
    let t = m.render();
    assert!(t.contains("greprum_job_runs_total{job=\"first_seen\"} 2"));
    assert!(t.contains("greprum_job_runs_total{job=\"reconcile\"} 2"));
    assert!(t.contains(&format!(
        "greprum_job_last_run_timestamp_seconds{{job=\"reconcile\"}} {}",
        T / 1000
    )));
}

// ---- AC0.8.3 CLI ------------------------------------------------------------

#[test]
fn ac_0_8_3_a_job_names_parse_and_unknown_is_rejected() {
    assert_eq!(JobName::parse("first-seen").unwrap(), JobName::FirstSeen);
    assert_eq!(JobName::parse("first_seen").unwrap(), JobName::FirstSeen);
    assert_eq!(JobName::parse("reconcile").unwrap(), JobName::Reconcile);
    assert!(JobName::parse("drop-tables").is_err());
}

#[test]
fn ac_0_8_3_b_print_shows_the_sql_without_connecting() {
    let a = JobName::FirstSeen.sql();
    assert!(a.contains("INSERT INTO first_seen") && a.contains("IS NULL"));
    let b = JobName::Reconcile.sql();
    assert!(b.contains("FROM events WHERE site_id = ?"));
}

#[tokio::test]
async fn ac_0_8_3_c_run_once_returns_a_json_result_for_each_job() {
    let f = Fake::default();
    *f.report.lock().unwrap() = Some(Report {
        expected: 10,
        received: 9,
        missing: 1,
        loss: 0.1,
        duplicate_rate: 0.0,
    });
    let a = run_once(&f, JobName::FirstSeen, "a", T).await.unwrap();
    assert_eq!(a["job"], "first-seen");
    assert_eq!(a["ok"], true);
    let b = run_once(&f, JobName::Reconcile, "a", T).await.unwrap();
    assert_eq!(b["missing"], 1);
    assert!(b["to"].as_i64().unwrap() - b["from"].as_i64().unwrap() == H);
}

// ---- AC0.8.4 idempotence / multi-gateway (live) ------------------------------

#[tokio::test]
#[ignore]
async fn ac_0_8_4_a_two_concurrent_first_seen_runs_leave_one_row_per_visitor() {
    use common::db::*;
    use greprum::jobs::SqlJobs;
    let db = fresh().await;
    let now = api::now_ms();
    let h = (now / H) * H - H;
    let mk = |v: &str, s: &str, seq: i32| greprum::model::StoredEvent {
        site_id: "mj".into(),
        event_name: "$pageview".into(),
        visitor_id: v.into(),
        session_id: s.into(),
        seq,
        ts: h + 1000,
        path: "/".into(),
        sample_rate: 1.0,
        ..Default::default()
    };
    let evs: Vec<_> = (0..40)
        .map(|i| mk(&format!("v{}", i % 10), "s", i))
        .collect();
    db.write(&evs).await;
    db.wait_count(
        "SELECT count(DISTINCT visitor_id) FROM visitor_hours WHERE site_id = 'mj'",
        10,
    )
    .await;
    let a = SqlJobs::lazy(&db.url).unwrap();
    let b = SqlJobs::lazy(&db.url).unwrap();
    let (ra, rb) = tokio::join!(a.first_seen("mj", 0), b.first_seen("mj", 0));
    ra.unwrap();
    rb.unwrap();
    assert_eq!(
        db.count("SELECT count(*) FROM first_seen WHERE site_id = 'mj'")
            .await,
        10
    );
    // a third run is a no-op
    a.first_seen("mj", 0).await.unwrap();
    assert_eq!(
        db.count("SELECT count(*) FROM first_seen WHERE site_id = 'mj'")
            .await,
        10
    );
}

#[tokio::test]
#[ignore]
async fn ac_0_8_4_b_live_scheduler_makes_retention_non_empty_and_loss_visible() {
    use common::db::*;
    use greprum::jobs::SqlJobs;
    let db = fresh().await;
    let now = api::now_ms();
    let hour_start = (now / H) * H - H; // the last full hour: what reconcile inspects
    let day = 86_400_000;
    let ev = |v: &str, s: &str, seq: i32, ts: i64| greprum::model::StoredEvent {
        site_id: "sj".into(),
        event_name: "$pageview".into(),
        visitor_id: v.into(),
        session_id: s.into(),
        seq,
        ts,
        path: "/".into(),
        sample_rate: 1.0,
        ..Default::default()
    };
    // Session s1 in the reconcile hour with a gap: seq 0,1,4 (2 and 3 lost in the browser).
    db.write(&[
        ev("a", "s1", 0, hour_start + 1000),
        ev("a", "s1", 1, hour_start + 2000),
        ev("a", "s1", 4, hour_start + 3000),
        ev("a", "s0", 0, now - day + 1000),
        ev("b", "s2", 0, now - 2 * day + 1000),
    ])
    .await;
    db.wait_count("SELECT count(*) FROM visitor_hours WHERE site_id = 'sj'", 3)
        .await;
    let m = Arc::new(Metrics::default());
    let s = Scheduler::new(
        Arc::new(SqlJobs::lazy(&db.url).unwrap()),
        vec!["sj".into()],
        3600,
        3600,
        m.clone(),
    );
    let r = s.tick(now).await;
    assert!(r.failed.is_empty(), "{r:?}");
    assert!(
        db.count("SELECT count(*) FROM first_seen WHERE site_id = 'sj'")
            .await
            >= 2
    );
    // seq 0..=4 expected (5), 3 received ⇒ 2 missing of 5 = 40 %
    assert!(
        m.render()
            .contains("greprum_site_browser_loss_ppm{site=\"sj\"} 400000"),
        "{}",
        m.render()
    );
}
