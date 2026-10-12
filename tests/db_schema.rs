//! L4: schema, flows and sink on a live GreptimeDB. `make db-up && make test-db`.
mod common;
use common::{db::*, spool::event as base_event, *};
use greprum::{migrate, model::StoredEvent, query::Bind};

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

/// Flows ignore data older than `EXPIRE AFTER` (7 days): fixtures must be recent.
fn hour() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    (now / 3_600_000) * 3_600_000 - 3_600_000
}

// ---- AC0.3.3 migrate --------------------------------------------------------

#[tokio::test]
#[ignore]
async fn ac_0_3_3_b_clean_database_migrates_ok() {
    let db = fresh().await;
    assert_eq!(db.count("SELECT count(*) FROM events").await, 0);
}

#[tokio::test]
#[ignore]
async fn ac_0_3_3_c_second_run_is_a_noop() {
    let db = fresh().await;
    let again = migrate::apply(&db.url, "13 months", &db.prefix)
        .await
        .expect("second apply");
    assert_eq!(again.len(), migrate::plan("13 months").unwrap().len());
}

#[tokio::test]
#[ignore]
async fn ac_0_3_3_d_all_expected_tables_and_flows_exist() {
    let db = fresh().await;
    let tables: Vec<String> = db
        .rows("SHOW TABLES", vec![])
        .await
        .iter()
        .flat_map(|r| {
            r.as_object()
                .unwrap()
                .values()
                .map(|v| v.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    for t in ["events", "first_seen", "sessions"]
        .iter()
        .chain(migrate::ROLLUP_TABLES.iter())
    {
        assert!(
            tables.iter().any(|x| x == t),
            "missing table {t}: {tables:?}"
        );
    }
    let flows = db.rows("SHOW FLOWS", vec![]).await.len();
    assert!(flows >= 6, "{flows}");
}

#[tokio::test]
#[ignore]
async fn ac_0_3_4_c_ttl_present_in_show_create_table() {
    let db = fresh_with_ttl("30 days").await;
    let r = db.rows("SHOW CREATE TABLE events", vec![]).await;
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("ttl") && s.contains("append_mode"), "{s}");
}

// ---- AC0.3.5 first-seen job --------------------------------------------------

#[tokio::test]
#[ignore]
async fn ac_0_3_5_c_first_seen_insert_if_absent_keeps_earliest() {
    let db = fresh().await;
    let h = hour();
    db.write(&[ev("fs", "$pageview", "v1", "a", 0, h - 5 * 3_600_000, "/")])
        .await;
    db.wait_count("SELECT count(*) FROM visitor_hours", 1).await;
    let (sql, _) = greprum::jobs::first_seen_sql();
    let run = |since: i64| {
        let binds = vec![Bind::Str("fs".into()), Bind::Int(since)];
        db.rows(Box::leak(sql.clone().into_boxed_str()), binds)
    };
    // INSERT ... SELECT returns no rows; the reader tolerates that as an empty result.
    let _ = run(0).await;
    assert_eq!(db.count("SELECT count(*) FROM first_seen").await, 1);
    db.write(&[
        ev("fs", "$pageview", "v1", "b", 0, h, "/"),
        ev("fs", "$pageview", "v2", "c", 0, h, "/"),
    ])
    .await;
    db.wait_count("SELECT count(*) FROM visitor_hours", 3).await;
    let _ = run(0).await;
    assert_eq!(
        db.count("SELECT count(*) FROM first_seen").await,
        2,
        "v1 not duplicated, v2 added"
    );
    let r = db
        .rows(
            "SELECT CAST(first_seen_ts AS BIGINT) AS t FROM first_seen WHERE visitor_id = 'v1'",
            vec![],
        )
        .await;
    assert!(
        r[0]["t"].as_i64().unwrap() <= h - 5 * 3_600_000 + 3_600_000,
        "earliest hour kept"
    );
}

// ---- AC0.3.6 flows -----------------------------------------------------------

#[tokio::test]
#[ignore]
async fn ac_0_3_6_a_click_rollup_counts_per_path_and_scaled() {
    let db = fresh().await;
    let h = hour();
    let mut quarter = ev("f1", "$click", "v1", "a", 2, h, "/p");
    quarter.sample_rate = 0.25;
    db.write(&[
        ev("f1", "$click", "v1", "a", 0, h, "/p"),
        ev("f1", "$click", "v2", "b", 0, h, "/p"),
        quarter,
        ev("f1", "$click", "v1", "a", 1, h, "/q"),
    ])
    .await;
    db.wait_count("SELECT count(*) FROM click_stats_1h", 2)
        .await;
    let r = db.rows("SELECT `path` AS p, CAST(sum(click_count) AS BIGINT) AS n, sum(click_count_scaled) AS s FROM click_stats_1h WHERE site_id = 'f1' GROUP BY `path` ORDER BY p", vec![]).await;
    assert_eq!(
        (r[0]["p"].as_str(), r[0]["n"].as_i64()),
        (Some("/p"), Some(3))
    );
    assert!(
        (r[0]["s"].as_f64().unwrap() - 6.0).abs() < 1e-6,
        "1+1+4 = 6: {r:?}"
    );
    assert_eq!(r[1]["n"].as_i64(), Some(1));
}

#[tokio::test]
#[ignore]
async fn ac_0_3_6_b_session_start_splits_new_and_returning() {
    let db = fresh().await;
    let h = hour();
    let mut n = ev("f2", "$session_start", "v1", "a", 0, h, "/");
    n.is_new_visitor = true;
    let r = ev("f2", "$session_start", "v2", "b", 0, h, "/");
    db.write(&[
        n,
        r.clone(),
        StoredEvent {
            session_id: "c".into(),
            ..r
        },
    ])
    .await;
    db.wait_count("SELECT count(*) FROM session_starts_1h", 2)
        .await;
    let rows = db.rows("SELECT is_new_visitor AS n, CAST(sum(sessions) AS BIGINT) AS s FROM session_starts_1h WHERE site_id = 'f2' GROUP BY is_new_visitor", vec![]).await;
    let total: i64 = rows.iter().map(|r| r["s"].as_i64().unwrap()).sum();
    assert_eq!(total, 3);
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
#[ignore]
async fn ac_0_3_6_c_page_edges_group_parent_and_path() {
    let db = fresh().await;
    let h = hour();
    let mut a = ev("f3", "$pageview", "v1", "a", 0, h, "/pricing");
    a.parent_path = "/".into();
    db.write(&[
        a.clone(),
        StoredEvent {
            seq: 1,
            session_id: "b".into(),
            ..a
        },
    ])
    .await;
    db.wait_count("SELECT count(*) FROM page_edges_1h_tbl", 1)
        .await;
    let r = db.rows("SELECT parent_path AS f, `path` AS t, CAST(sum(views) AS BIGINT) AS v FROM page_edges_1h_tbl WHERE site_id = 'f3' GROUP BY parent_path, `path`", vec![]).await;
    assert_eq!(
        (r[0]["f"].as_str(), r[0]["t"].as_str(), r[0]["v"].as_i64()),
        (Some("/"), Some("/pricing"), Some(2))
    );
}

#[tokio::test]
#[ignore]
async fn ac_0_3_6_d_visitor_hours_hold_distinct_visitors() {
    let db = fresh().await;
    let h = hour();
    let evs: Vec<_> = (0..10)
        .map(|i| ev("f4", "$pageview", &format!("v{}", i % 3), "a", i, h, "/"))
        .collect();
    db.write(&evs).await;
    db.wait_count("SELECT count(*) FROM visitor_hours WHERE site_id = 'f4'", 3)
        .await;
    db.flush_flows().await;
    assert_eq!(
        db.count("SELECT count(DISTINCT visitor_id) FROM visitor_hours WHERE site_id = 'f4'")
            .await,
        3
    );
}

#[tokio::test]
#[ignore]
async fn ac_0_3_6_e_user_hours_only_identified() {
    let db = fresh().await;
    let h = hour();
    let mut id = ev("f5", "$pageview", "v1", "a", 0, h, "/");
    id.is_identified = true;
    id.user_id = "u_1".into();
    db.write(&[id, ev("f5", "$pageview", "v2", "b", 0, h, "/")])
        .await;
    db.wait_count("SELECT count(*) FROM user_hours WHERE site_id = 'f5'", 1)
        .await;
    assert_eq!(
        db.count("SELECT count(*) FROM user_hours WHERE site_id = 'f5'")
            .await,
        1
    );
}

#[tokio::test]
#[ignore]
async fn ac_0_3_6_f_one_minute_click_rollup_exists() {
    let db = fresh().await;
    let h = hour();
    db.write(&[
        ev("f6", "$click", "v1", "a", 0, h + 5_000, "/"),
        ev("f6", "$click", "v1", "a", 1, h + 65_000, "/"),
    ])
    .await;
    let n = db
        .wait_count(
            "SELECT count(*) FROM click_stats_1m WHERE site_id = 'f6'",
            2,
        )
        .await;
    assert_eq!(n, 2);
}

// ---- AC0.4.16 SqlxSink ------------------------------------------------------

#[tokio::test]
#[ignore]
async fn ac_0_4_16_d_rows_land_with_types_and_null_for_empty_optionals() {
    let db = fresh().await;
    let mut e = ev("k1", "$click", "v1", "s1", 7, NOW, "/x");
    e.el_text = "Start".into();
    e.is_identified = true;
    e.user_id = "u9".into();
    e.sample_rate = 0.5;
    db.write(&[e]).await;
    let r = db.rows("SELECT CAST(ts AS BIGINT) AS ts, seq, is_identified, user_id, el_text, el_css, sample_rate, `path` AS p FROM events WHERE site_id = 'k1'", vec![]).await;
    let r = &r[0];
    assert_eq!(r["ts"].as_i64(), Some(NOW));
    assert_eq!(r["seq"].as_i64(), Some(7));
    assert_eq!(r["user_id"].as_str(), Some("u9"));
    assert_eq!(r["el_text"].as_str(), Some("Start"));
    assert!(r["el_css"].is_null());
    assert_eq!(r["sample_rate"].as_f64(), Some(0.5));
    assert_eq!(r["p"].as_str(), Some("/x"));
}

#[tokio::test]
#[ignore]
async fn ac_0_4_16_e_thousand_row_multirow_insert() {
    let db = fresh().await;
    let evs: Vec<_> = (0..1000)
        .map(|i| {
            ev(
                "k2",
                "$pageview",
                &format!("v{}", i % 50),
                &format!("s{}", i % 20),
                i,
                NOW + i as i64,
                "/",
            )
        })
        .collect();
    db.write(&evs).await;
    assert_eq!(
        db.count("SELECT count(*) FROM events WHERE site_id = 'k2'")
            .await,
        1000
    );
}

#[tokio::test]
#[ignore]
async fn ac_0_4_16_f_hostile_strings_are_bound_not_interpreted() {
    let db = fresh().await;
    let nasty = "x'); DROP TABLE events; -- \" \\ 😀";
    let mut e = ev("k3", "$click", "v", "s", 0, NOW, "/");
    e.el_text = nasty.into();
    db.write(&[e]).await;
    let r = db
        .rows("SELECT el_text FROM events WHERE site_id = 'k3'", vec![])
        .await;
    assert_eq!(r[0]["el_text"].as_str(), Some(nasty));
    assert_eq!(db.count("SELECT count(*) FROM events").await, 1);
}
