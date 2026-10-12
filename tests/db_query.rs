//! L4: every query builder executes on the real engine and returns correct numbers.
mod common;
use common::{
    db::{api::*, *},
    spool::event as base_event,
};
use greprum::{
    collect, jobs,
    model::StoredEvent,
    query::{Bind, Kind, build},
    synth::Generator,
};
use std::collections::HashMap;

fn ev(
    site: &str,
    name: &str,
    visitor: &str,
    session: &str,
    seq: i32,
    ts: i64,
    path: &str,
) -> StoredEvent {
    StoredEvent {
        site_id: site.into(),
        event_name: name.into(),
        visitor_id: visitor.into(),
        session_id: session.into(),
        seq,
        ts,
        path: path.into(),
        sample_rate: 1.0,
        device: "desktop".into(),
        browser: "Chrome".into(),
        os: "macOS".into(),
        country: "SG".into(),
        ..base_event(session, seq)
    }
}

async fn run_first_seen(db: &TestDb, site: &str) {
    let (sql, _) = jobs::first_seen_sql();
    let _ = db
        .rows(
            Box::leak(sql.into_boxed_str()),
            vec![Bind::Str(site.into()), Bind::Int(0)],
        )
        .await;
}

/// Seed `sessions` generated sessions for `site` over the last ~2 days.
async fn seed(db: &TestDb, site: &str, sessions: usize) -> usize {
    let now = now_ms();
    let mut g = Generator::new(42, now - 2 * 86_400_000, 36 * 3_600_000);
    let s = site_cfg(site);
    let mut all = vec![];
    for _ in 0..sessions {
        let gs = g.session();
        let info = collect::RequestInfo {
            user_agent: gs.user_agent.into(),
            country: gs.country.into(),
            dnt: false,
        };
        for b in gs.batches {
            let rec = b["sent_at"].as_i64().unwrap();
            all.extend(
                collect::process(
                    b.to_string().as_bytes(),
                    &s,
                    &info,
                    &Default::default(),
                    rec,
                )
                .unwrap()
                .events,
            );
        }
    }
    for c in all.chunks(2000) {
        db.write(c).await;
    }
    all.len()
}

#[tokio::test]
#[ignore]
async fn ac_0_5_4_e_overview_totals_match_raw_events_on_seeded_data() {
    let db = fresh().await;
    let n = seed(&db, "q1", 120).await;
    db.wait_count(
        "SELECT count(DISTINCT visitor_id) FROM visitor_hours WHERE site_id = 'q1'",
        1,
    )
    .await;
    let raw_sessions = db
        .count("SELECT count(*) FROM events WHERE site_id = 'q1' AND event_name = '$session_start'")
        .await;
    let raw_pv = db
        .count("SELECT count(*) FROM events WHERE site_id = 'q1' AND event_name = '$pageview'")
        .await;
    db.wait_count("SELECT CAST(coalesce(sum(sessions),0) AS BIGINT) FROM session_starts_1h WHERE site_id = 'q1'", raw_sessions).await;
    db.wait_count(
        "SELECT CAST(coalesce(sum(views),0) AS BIGINT) FROM page_edges_1h_tbl WHERE site_id = 'q1'",
        raw_pv,
    )
    .await;
    assert!(n > 500);
    let api = api(&db, &["q1"]).await;
    let (st, b) = api.get("/api/v1/stats/overview?from=-1").await;
    assert_eq!(st, 400, "negative/garbage still validated: {b}");
    let to = now_ms() + 3_600_000;
    let from = to - 4 * 86_400_000;
    let (st, b) = api
        .get(&format!("/api/v1/stats/overview?from={from}&to={to}"))
        .await;
    assert_eq!(st, 200, "{b}");
    let d = &b["data"];
    assert_eq!(d["sessions"]["count"].as_i64(), Some(raw_sessions), "{d}");
    assert_eq!(d["pageviews"]["count"].as_i64(), Some(raw_pv));
    assert_eq!(
        d["sessions"]["scaled"].as_f64().map(|f| f.round() as i64),
        Some(raw_sessions)
    );
    let visitors = db
        .count("SELECT count(DISTINCT visitor_id) FROM events WHERE site_id = 'q1'")
        .await;
    assert_eq!(d["visitors"].as_i64(), Some(visitors));
    assert!(d["mau"].as_i64().unwrap() >= d["dau"].as_i64().unwrap());
}

#[tokio::test]
#[ignore]
async fn ac_0_5_5_d_pages_journeys_elements_execute_and_agree_with_raw() {
    let db = fresh().await;
    seed(&db, "q2", 100).await;
    let raw_clicks = db
        .count("SELECT count(*) FROM events WHERE site_id = 'q2' AND event_name = '$click'")
        .await;
    db.wait_count("SELECT CAST(coalesce(sum(click_count),0) AS BIGINT) FROM click_stats_1h WHERE site_id = 'q2'", raw_clicks).await;
    let api = api(&db, &["q2"]).await;
    let to = now_ms() + 3_600_000;
    let range = format!("from={}&to={}", to - 4 * 86_400_000, to);
    for p in [
        "top-pages",
        "top-pages?by=page_group",
        "entry-pages",
        "journeys",
        "top-elements",
        "rage-clicks",
    ] {
        let sep = if p.contains('?') { '&' } else { '?' };
        let (st, b) = api.get(&format!("/api/v1/stats/{p}{sep}{range}")).await;
        assert_eq!(st, 200, "{p}: {b}");
    }
    let (_, b) = api
        .get(&format!("/api/v1/stats/top-pages?{range}&limit=100"))
        .await;
    let total_clicks: i64 = b["data"]["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["clicks"].as_i64().unwrap_or(0))
        .sum();
    assert_eq!(
        total_clicks, raw_clicks,
        "top-pages click sum equals raw clicks"
    );
    let (_, b) = api
        .get(&format!("/api/v1/stats/top-elements?{range}"))
        .await;
    assert!(!b["data"]["elements"].as_array().unwrap().is_empty());
    let (_, b) = api
        .get(&format!(
            "/api/v1/stats/browsers-devices?by=browser&{range}"
        ))
        .await;
    assert!(!b["data"]["rows"].as_array().unwrap().is_empty());
}

#[tokio::test]
#[ignore]
async fn ac_0_5_7_e_funnel_counts_ordered_steps_within_window() {
    let db = fresh().await;
    let t = now_ms() - 3_600_000;
    let evs = vec![
        // v1 completes A -> B -> C within 60 s
        ev("fn", "A", "v1", "s1", 0, t, "/"),
        ev("fn", "B", "v1", "s1", 1, t + 10_000, "/"),
        ev("fn", "C", "v1", "s1", 2, t + 20_000, "/"),
        // v2 stops after B
        ev("fn", "A", "v2", "s2", 0, t, "/"),
        ev("fn", "B", "v2", "s2", 1, t + 5_000, "/"),
        // v3 does B before A: not a funnel entry in order
        ev("fn", "B", "v3", "s3", 0, t, "/"),
        ev("fn", "A", "v3", "s3", 1, t + 5_000, "/"),
        // v4 completes but too slowly (B after 120 s)
        ev("fn", "A", "v4", "s4", 0, t, "/"),
        ev("fn", "B", "v4", "s4", 1, t + 120_000, "/"),
        // other site must never leak
        ev("other", "A", "v9", "s9", 0, t, "/"),
        ev("other", "B", "v9", "s9", 1, t + 1_000, "/"),
    ];
    db.write(&evs).await;
    let api = api(&db, &["fn"]).await;
    let (st, b) = api
        .get(&format!(
            "/api/v1/stats/funnel?steps=A,B,C&window_seconds=60&from={}&to={}",
            t - 1000,
            t + 600_000
        ))
        .await;
    assert_eq!(st, 200, "{b}");
    let v: Vec<i64> = b["data"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["visitors"].as_i64().unwrap())
        .collect();
    // A: v1..v4 reach step 1; B in order within 60 s: v1, v2; C: v1 only. `other` never leaks.
    assert_eq!(v, vec![4, 2, 1]);
}

#[tokio::test]
#[ignore]
async fn ac_0_5_8_c_retention_cohorts_from_first_seen() {
    let db = fresh().await;
    let day = 86_400_000i64;
    let today = (now_ms() / day) * day;
    let evs = vec![
        ev(
            "rt",
            "$pageview",
            "a",
            "s1",
            0,
            today - 2 * day + 3_600_000,
            "/",
        ),
        ev(
            "rt",
            "$pageview",
            "a",
            "s2",
            0,
            today - day + 3_600_000,
            "/",
        ),
        ev(
            "rt",
            "$pageview",
            "b",
            "s3",
            0,
            today - 2 * day + 7_200_000,
            "/",
        ),
    ];
    db.write(&evs).await;
    db.wait_count("SELECT count(*) FROM visitor_hours WHERE site_id = 'rt'", 3)
        .await;
    run_first_seen(&db, "rt").await;
    let api = api(&db, &["rt"]).await;
    let (st, b) = api
        .get(&format!(
            "/api/v1/stats/retention?days=3&from={}&to={}",
            today - 3 * day,
            today + day
        ))
        .await;
    assert_eq!(st, 200, "{b}");
    let cohorts = b["data"]["cohorts"].as_array().unwrap();
    let c = cohorts
        .iter()
        .find(|c| c["size"].as_i64() == Some(2))
        .unwrap_or_else(|| panic!("{b}"));
    assert_eq!(
        c["retained"]["1"].as_i64(),
        Some(1),
        "only visitor a returned on day 1"
    );
    assert_eq!(c["rate"]["1"].as_f64(), Some(0.5));
}

#[tokio::test]
#[ignore]
async fn ac_0_5_10_d_sessions_list_and_detail_on_real_rows() {
    let db = fresh().await;
    seed(&db, "ss", 60).await;
    let api = api(&db, &["ss"]).await;
    let (st, b) = api.get("/api/v1/sessions?from=-1").await;
    assert_eq!(st, 400, "{b}");
    let to = now_ms() + 60_000;
    let (st, b) = api
        .get(&format!(
            "/api/v1/sessions?from={}&to={to}&limit=5",
            to - 3 * 86_400_000
        ))
        .await;
    assert_eq!(st, 200, "{b}");
    let s = b["data"]["sessions"].as_array().unwrap();
    assert!(!s.is_empty() && s.len() <= 5);
    let id = s[0]["session_id"].as_str().unwrap().to_string();
    let (st, b) = api
        .get(&format!(
            "/api/v1/sessions/{id}?from={}&to={to}",
            to - 3 * 86_400_000
        ))
        .await;
    assert_eq!(st, 200, "{b}");
    let evs = b["data"]["events"].as_array().unwrap();
    assert_eq!(evs[0]["event_name"], "$session_start");
    let seqs: Vec<i64> = evs.iter().map(|e| e["seq"].as_i64().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[0] <= w[1]), "ordered by seq");
    let (_, b) = api
        .get(&format!(
            "/api/v1/sessions?from={}&to={to}&has_rage=1",
            to - 3 * 86_400_000
        ))
        .await;
    for s in b["data"]["sessions"].as_array().unwrap() {
        assert!(s["rage_clicks"].as_i64().unwrap() > 0);
    }
}

#[tokio::test]
#[ignore]
async fn ac_0_5_11_d_events_endpoint_paginates_without_overlap() {
    let db = fresh().await;
    seed(&db, "ev", 30).await;
    let api = api(&db, &["ev"]).await;
    let to = now_ms() + 60_000;
    let base = format!(
        "/api/v1/events?from={}&to={to}&limit=20",
        to - 3 * 86_400_000
    );
    let (_, p1) = api.get(&base).await;
    let e1 = p1["data"]["events"].as_array().unwrap();
    assert_eq!(e1.len(), 20);
    let cur = p1["data"]["next_before"].as_i64().unwrap();
    let (_, p2) = api.get(&format!("{base}&before={cur}")).await;
    let e2 = p2["data"]["events"].as_array().unwrap();
    assert!(!e2.is_empty());
    assert!(e2.iter().all(|e| e["ts"].as_i64().unwrap() < cur));
}

#[tokio::test]
#[ignore]
async fn ac_0_1_3_d_all_spike_queries_execute_and_time_on_live_db() {
    let db = fresh().await;
    seed(&db, "sp", 150).await;
    db.wait_count("SELECT count(*) FROM visitor_hours WHERE site_id = 'sp'", 1)
        .await;
    run_first_seen(&db, "sp").await;
    let now = now_ms();
    let site = site_cfg("sp");
    let mut ms: HashMap<&str, f64> = HashMap::new();
    for (name, kind, extra) in [
        ("overview", Kind::Overview, vec![]),
        ("pages", Kind::TopPages, vec![]),
        (
            "funnel",
            Kind::Funnel,
            vec![
                ("steps", "$session_start,$pageview,signup"),
                ("window_seconds", "1800"),
            ],
        ),
        ("retention", Kind::Retention, vec![("days", "7")]),
        ("sessions", Kind::Sessions, vec![]),
    ] {
        let mut q: HashMap<String, String> = extra
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        q.insert("from".into(), (now - 5 * 86_400_000).to_string());
        q.insert("to".into(), (now + 60_000).to_string());
        let p = build::Params::parse(kind, &site, &q, None, now).unwrap();
        let t = std::time::Instant::now();
        for qq in build::plan(kind, &p).unwrap() {
            use greprum::query::Reader;
            db.reader
                .fetch(&qq)
                .await
                .unwrap_or_else(|e| panic!("{name}/{}: {e}\n{}", qq.name, qq.sql));
        }
        ms.insert(name, t.elapsed().as_secs_f64() * 1000.0);
    }
    assert_eq!(ms.len(), 5);
}
