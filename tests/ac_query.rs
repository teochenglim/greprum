//! L1/L2: Query API contract — AC0.5.1 – AC0.5.12. Uses a fake `Reader`: no database.
mod common;
use common::{qapp::*, *};
use greprum::query::{Bind, ENDPOINTS, Q};
use serde_json::json;

fn uri(path: &str, qs: &str) -> String {
    let p = path.replace("{id}", "sess_1");
    format!("{p}?{qs}")
}

fn all_sql(r: &FakeReader) -> Vec<Q> {
    r.calls()
}

// ---- AC0.5.1 auth & scoping -------------------------------------------------

#[tokio::test]
async fn ac_0_5_1_a_missing_or_wrong_key_401() {
    let h = qharness(FakeReader::with(&[]));
    for (path, _) in ENDPOINTS {
        let u = uri(path, "steps=a,b&window_seconds=60");
        assert_eq!(h.get(&u, None).await.0, 401, "{path} without key");
        assert_eq!(h.get(&u, Some("nope")).await.0, 401, "{path} wrong key");
    }
    assert!(h.reader.calls().is_empty(), "no query without auth");
}

#[tokio::test]
async fn ac_0_5_1_b_key_scoped_to_site_a_cannot_read_site_b_on_any_endpoint() {
    let h = qharness(FakeReader::with(&[]));
    for (path, _) in ENDPOINTS {
        let u = uri(path, "site=s2&steps=a,b&window_seconds=60");
        assert_eq!(h.get(&u, Some("key-a")).await.0, 403, "{path}");
    }
    assert!(h.reader.calls().is_empty());
}

#[tokio::test]
async fn ac_0_5_1_c_endpoint_list_covers_every_prd_endpoint() {
    let want = [
        "overview",
        "timeseries",
        "top-pages",
        "entry-pages",
        "journeys",
        "top-elements",
        "funnel",
        "retention",
        "rage-clicks",
        "browsers-devices",
    ];
    for w in want {
        assert!(
            ENDPOINTS
                .iter()
                .any(|(p, _)| p.ends_with(&format!("/stats/{w}"))),
            "{w}"
        );
    }
    for w in [
        "/api/v1/sessions",
        "/api/v1/sessions/{id}",
        "/api/v1/events",
        "/api/v1/usage",
    ] {
        assert!(ENDPOINTS.iter().any(|(p, _)| *p == w), "{w}");
    }
    assert_eq!(ENDPOINTS.len(), 14);
}

#[tokio::test]
async fn ac_0_5_1_d_multi_site_key_must_name_site_single_site_key_defaults() {
    let h = qharness(FakeReader::with(&[]));
    assert_eq!(
        h.get("/api/v1/stats/top-pages", Some("key-ab")).await.0,
        400
    );
    assert_eq!(
        h.get("/api/v1/stats/top-pages?site=s2", Some("key-ab"))
            .await
            .0,
        200
    );
    let (st, body) = h.get("/api/v1/stats/top-pages", Some("key-a")).await;
    assert_eq!((st, body["site"].as_str()), (200, Some("s1")));
}

#[tokio::test]
async fn ac_0_5_1_e_keys_are_compared_by_hash_clear_text_in_config_is_not_a_key() {
    let h = qharness(FakeReader::with(&[]));
    // The stored value is the hash; presenting the hash itself must not authenticate.
    let hash = greprum::query::hash_key("key-a");
    assert_eq!(h.get("/api/v1/stats/top-pages", Some(&hash)).await.0, 401);
}

// ---- AC0.5.2 SQL safety -----------------------------------------------------

const FUZZ: &[&str] = &[
    "x' OR '1'='1",
    "a; DROP TABLE events; --",
    "\" UNION SELECT 1 --",
    "/p?q=1#frag",
    "ünï😀code",
    "%27%20OR%201=1",
];

#[tokio::test]
async fn ac_0_5_2_a_every_query_is_site_scoped_with_bound_site() {
    let h = qharness(FakeReader::with(&[]));
    for (path, kind) in ENDPOINTS {
        if matches!(kind, greprum::query::Kind::Usage) {
            continue;
        }
        let u = uri(path, "steps=$pageview,signup&window_seconds=3600&days=3");
        assert_eq!(h.get(&u, Some("key-a")).await.0, 200, "{path}");
    }
    let calls = all_sql(&h.reader);
    assert!(calls.len() > 20);
    for q in &calls {
        assert!(q.sql.contains("site_id = ?"), "{}: {}", q.name, q.sql);
        assert!(
            q.binds.contains(&Bind::Str("s1".into())),
            "{} binds no site",
            q.name
        );
        assert_eq!(
            q.sql.matches('?').count(),
            q.binds.len(),
            "{}: placeholder/bind mismatch",
            q.name
        );
    }
}

#[tokio::test]
async fn ac_0_5_2_b_request_strings_never_appear_in_sql_text_fuzz() {
    let h = qharness(FakeReader::with(&[]));
    for f in FUZZ {
        let enc = urlencoding(f);
        for (path, k) in ENDPOINTS {
            if matches!(k, greprum::query::Kind::Usage) {
                continue;
            }
            let u = uri(
                path,
                &format!(
                    "event_name={enc}&path={enc}&user_id={enc}&visitor_id={enc}&country={enc}&device={enc}&from_page={enc}&event={enc}&steps={enc}@{enc},{enc}&window_seconds=60&session_id={enc}"
                ),
            );
            h.get(&u, Some("key-a")).await;
        }
    }
    for q in all_sql(&h.reader) {
        for f in FUZZ {
            assert!(
                !q.sql.contains(f),
                "fuzz leaked into SQL of {}: {}",
                q.name,
                q.sql
            );
        }
        assert!(!q.sql.to_lowercase().contains("drop table"));
    }
    assert!(!h.reader.calls().is_empty());
}

#[tokio::test]
async fn ac_0_5_2_c_unknown_dimension_bucket_metric_rejected_not_interpolated() {
    let h = qharness(FakeReader::with(&[]));
    for u in [
        "/api/v1/stats/browsers-devices?by=password",
        "/api/v1/stats/browsers-devices?by=browser;DROP",
        "/api/v1/stats/timeseries?bucket=1 second",
        "/api/v1/stats/timeseries?metric=events",
        "/api/v1/stats/top-pages?by=el_text",
    ] {
        assert_eq!(
            h.get(&u.replace(' ', "%20"), Some("key-a")).await.0,
            400,
            "{u}"
        );
    }
    assert!(h.reader.calls().is_empty());
}

#[tokio::test]
async fn ac_0_5_2_d_table_names_are_from_the_fixed_set() {
    let h = qharness(FakeReader::with(&[]));
    for (path, k) in ENDPOINTS {
        if matches!(k, greprum::query::Kind::Usage) {
            continue;
        }
        h.get(&uri(path, "steps=a,b&window_seconds=60"), Some("key-a"))
            .await;
    }
    let mut ok: Vec<&str> = greprum::migrate::ROLLUP_TABLES.to_vec();
    ok.extend(["events", "first_seen", "sessions"]);
    for q in all_sql(&h.reader) {
        for tok in q
            .sql
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .collect::<Vec<_>>()
            .windows(2)
        {
            if tok[0].eq_ignore_ascii_case("FROM") || tok[0].eq_ignore_ascii_case("JOIN") {
                let t = tok[1];
                let is_cte = t.len() <= 3 && t.starts_with('s') || t == "a" || t.is_empty();
                assert!(
                    ok.contains(&t) || is_cte,
                    "unexpected table `{t}` in {}",
                    q.sql
                );
            }
        }
    }
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

// ---- AC0.5.3 time caps ------------------------------------------------------

#[tokio::test]
async fn ac_0_5_3_a_range_over_31_days_on_raw_events_400() {
    let h = qharness(FakeReader::with(&[]));
    let to = NOW;
    let from = NOW - 32 * 86_400_000;
    for p in [
        "events",
        "sessions",
        "stats/top-elements",
        "stats/rage-clicks",
        "stats/browsers-devices",
    ] {
        let (st, _) = h
            .get(&format!("/api/v1/{p}?from={from}&to={to}"), Some("key-a"))
            .await;
        assert_eq!(st, 400, "{p}");
    }
    let from_ok = NOW - 31 * 86_400_000;
    assert_eq!(
        h.get(
            &format!("/api/v1/events?from={from_ok}&to={to}"),
            Some("key-a")
        )
        .await
        .0,
        200
    );
}

#[tokio::test]
async fn ac_0_5_3_b_sessions_default_range_is_24_hours() {
    let h = qharness(FakeReader::with(&[]));
    h.get("/api/v1/sessions", Some("key-a")).await;
    let q = &h.reader.calls()[0];
    assert!(q.binds.contains(&Bind::Int(NOW)));
    assert!(q.binds.contains(&Bind::Int(NOW - 86_400_000)));
    assert!(q.binds.contains(&Bind::Int(200)), "newest 200 by default");
}

#[tokio::test]
async fn ac_0_5_3_c_inverted_range_and_garbage_time_400() {
    let h = qharness(FakeReader::with(&[]));
    assert_eq!(
        h.get("/api/v1/events?from=2000&to=1000", Some("key-a"))
            .await
            .0,
        400
    );
    assert_eq!(
        h.get("/api/v1/events?from=yesterday", Some("key-a"))
            .await
            .0,
        400
    );
    assert_eq!(
        h.get("/api/v1/sessions?limit=abc", Some("key-a")).await.0,
        400
    );
    // limit is clamped, not trusted
    h.get("/api/v1/sessions?limit=99999", Some("key-a")).await;
    assert!(
        h.reader
            .calls()
            .last()
            .unwrap()
            .binds
            .contains(&Bind::Int(200))
    );
}

// ---- AC0.5.4 overview + timeseries -----------------------------------------

#[tokio::test]
async fn ac_0_5_4_a_dau_uses_site_timezone_day_boundary() {
    // NOW = 2025-10-12T02:40:00Z = 10:40 in Singapore (UTC+8); local day starts at 2025-10-11T16:00Z.
    let h = qharness(FakeReader::with(&[]));
    h.get("/api/v1/stats/overview", Some("key-a")).await;
    let dau = h
        .reader
        .calls()
        .into_iter()
        .find(|q| q.name == "dau")
        .unwrap();
    let day_start = 1_760_198_400_000i64; // 2025-10-11T16:00:00Z
    assert!(dau.binds.contains(&Bind::Int(day_start)), "{:?}", dau.binds);
    assert!(dau.binds.contains(&Bind::Int(day_start + 86_400_000)));
}

#[tokio::test]
async fn ac_0_5_4_b_wau_and_mau_windows_end_at_day_end() {
    let h = qharness(FakeReader::with(&[]));
    h.get("/api/v1/stats/overview", Some("key-a")).await;
    let end = 1_760_198_400_000i64 + 86_400_000;
    let c = h.reader.calls();
    let wau = c.iter().find(|q| q.name == "wau").unwrap();
    let mau = c.iter().find(|q| q.name == "mau").unwrap();
    assert!(
        wau.binds.contains(&Bind::Int(end - 7 * 86_400_000)) && wau.binds.contains(&Bind::Int(end))
    );
    assert!(mau.binds.contains(&Bind::Int(end - 30 * 86_400_000)));
    assert!(wau.sql.contains("visitor_hours") && wau.sql.contains("count(DISTINCT visitor_id)"));
}

#[tokio::test]
async fn ac_0_5_4_c_overview_response_has_all_fields_and_previous_period() {
    let rows = [
        ("dau", vec![json!({"n": 10})]),
        ("wau", vec![json!({"n": 40})]),
        ("mau", vec![json!({"n": 90})]),
        ("sessions", vec![json!({"n": 100, "scaled": 400.0})]),
        ("pageviews", vec![json!({"n": 250, "scaled": 1000.0})]),
        ("clicks", vec![json!({"n": 30, "scaled": 120.0})]),
        ("identified", vec![json!({"n": 7})]),
        ("sessions_prev", vec![json!({"n": 80, "scaled": 320.0})]),
    ];
    let h = qharness(FakeReader::with(&rows));
    let (st, b) = h.get("/api/v1/stats/overview", Some("key-a")).await;
    let d = &b["data"];
    assert_eq!(st, 200);
    assert_eq!(
        (
            d["dau"].as_i64(),
            d["wau"].as_i64(),
            d["mau"].as_i64(),
            d["identified_users"].as_i64()
        ),
        (Some(10), Some(40), Some(90), Some(7))
    );
    assert_eq!(d["sessions"]["count"], 100);
    assert_eq!(d["sessions"]["scaled"], 400.0);
    assert_eq!(d["previous"]["sessions"]["count"], 80);
    assert_eq!(d["pages_per_session"], 2.5);
    assert_eq!(d["timezone"], "Asia/Singapore");
}

#[tokio::test]
async fn ac_0_5_4_d_timeseries_bucket_and_metric_choose_fixed_tables() {
    let h = qharness(FakeReader::with(&[]));
    h.get(
        "/api/v1/stats/timeseries?metric=clicks&bucket=1m",
        Some("key-a"),
    )
    .await;
    h.get(
        "/api/v1/stats/timeseries?metric=clicks&bucket=1d",
        Some("key-a"),
    )
    .await;
    h.get(
        "/api/v1/stats/timeseries?metric=pageviews&bucket=1h",
        Some("key-a"),
    )
    .await;
    let c = h.reader.calls();
    assert!(c[0].sql.contains("click_stats_1m") && c[0].sql.contains("1 minute"));
    assert!(c[1].sql.contains("click_stats_1h") && c[1].sql.contains("1 day"));
    assert!(c[2].sql.contains("page_edges_1h_tbl"));
    assert_eq!(
        h.get(
            "/api/v1/stats/timeseries?metric=pageviews&bucket=1m",
            Some("key-a")
        )
        .await
        .0,
        400
    );
}

// ---- AC0.5.5 pages ----------------------------------------------------------

#[tokio::test]
async fn ac_0_5_5_a_top_pages_by_path_merges_views_clicks_visitors() {
    let rows = [
        (
            "views",
            vec![
                json!({"page": "/a", "views": 10, "views_scaled": 40.0}),
                json!({"page": "/b", "views": 5, "views_scaled": 5.0}),
            ],
        ),
        (
            "clicks",
            vec![json!({"page": "/a", "clicks": 3, "clicks_scaled": 12.0})],
        ),
        ("visitors", vec![json!({"page": "/a", "visitors": 7})]),
    ];
    let h = qharness(FakeReader::with(&rows));
    let (_, b) = h.get("/api/v1/stats/top-pages", Some("key-a")).await;
    let pages = b["data"]["pages"].as_array().unwrap();
    assert_eq!(pages[0]["page"], "/a");
    assert_eq!(
        (
            pages[0]["views"].as_i64(),
            pages[0]["clicks"].as_i64(),
            pages[0]["visitors"].as_i64()
        ),
        (Some(10), Some(3), Some(7))
    );
    assert_eq!(pages[0]["views_scaled"], 40.0);
    assert_eq!(pages[1]["clicks"], 0, "page without clicks still listed");
}

#[tokio::test]
async fn ac_0_5_5_b_top_pages_by_page_group_reads_events_only() {
    let h = qharness(FakeReader::with(&[]));
    assert_eq!(
        h.get("/api/v1/stats/top-pages?by=page_group", Some("key-a"))
            .await
            .0,
        200
    );
    let c = h.reader.calls();
    assert_eq!(c.len(), 1);
    assert!(c[0].sql.contains("FROM events") && c[0].sql.contains("GROUP BY page_group"));
    assert_eq!(
        h.get("/api/v1/stats/top-pages?by=el_text", Some("key-a"))
            .await
            .0,
        400
    );
}

#[tokio::test]
async fn ac_0_5_5_c_entry_pages_split_new_and_returning() {
    let rows = [(
        "entries",
        vec![
            json!({"page": "/", "sessions": 10, "sessions_scaled": 10.0, "new_visitors": 6, "returning_visitors": 4}),
        ],
    )];
    let h = qharness(FakeReader::with(&rows));
    let (_, b) = h.get("/api/v1/stats/entry-pages", Some("key-a")).await;
    let p = &b["data"]["pages"][0];
    assert_eq!(
        p["new_visitors"].as_i64().unwrap() + p["returning_visitors"].as_i64().unwrap(),
        p["sessions"].as_i64().unwrap()
    );
    assert!(h.reader.calls()[0].sql.contains("session_starts_1h"));
}

// ---- AC0.5.6 journeys & elements -------------------------------------------

#[tokio::test]
async fn ac_0_5_6_a_journeys_from_page_is_bound() {
    let h = qharness(FakeReader::with(&[]));
    h.get("/api/v1/stats/journeys?from_page=/pricing", Some("key-a"))
        .await;
    let q = &h.reader.calls()[0];
    assert!(q.sql.contains("parent_path = ?") && q.sql.contains("page_edges_1h_tbl"));
    assert!(q.binds.contains(&Bind::Str("/pricing".into())));
    assert!(!q.sql.contains("/pricing"));
}

#[tokio::test]
async fn ac_0_5_6_b_limit_is_capped_at_500() {
    let h = qharness(FakeReader::with(&[]));
    h.get("/api/v1/stats/journeys?limit=100000", Some("key-a"))
        .await;
    assert!(h.reader.calls()[0].binds.contains(&Bind::Int(500)));
}

#[tokio::test]
async fn ac_0_5_6_c_top_elements_group_by_css_and_path_clicks_only() {
    let h = qharness(FakeReader::with(&[(
        "elements",
        vec![json!({"el_css": "main>button#cta", "page": "/", "clicks": 9})],
    )]));
    let (_, b) = h.get("/api/v1/stats/top-elements", Some("key-a")).await;
    let q = &h.reader.calls()[0];
    assert!(q.sql.contains("GROUP BY el_css") && q.sql.contains("event_name = '$click'"));
    assert_eq!(b["data"]["elements"][0]["clicks"], 9);
}

// ---- AC0.5.7 funnel ---------------------------------------------------------

#[tokio::test]
async fn ac_0_5_7_a_missing_window_seconds_is_400() {
    let h = qharness(FakeReader::with(&[]));
    let (st, b) = h.get("/api/v1/stats/funnel?steps=a,b", Some("key-a")).await;
    assert_eq!(st, 400);
    assert!(b["error"].as_str().unwrap().contains("window_seconds"));
    assert!(h.reader.calls().is_empty());
    for bad in ["0", "-5", "abc", "99999999"] {
        assert_eq!(
            h.get(
                &format!("/api/v1/stats/funnel?steps=a,b&window_seconds={bad}"),
                Some("key-a")
            )
            .await
            .0,
            400,
            "{bad}"
        );
    }
}

#[tokio::test]
async fn ac_0_5_7_b_step_count_must_be_2_to_8() {
    let h = qharness(FakeReader::with(&[]));
    assert_eq!(
        h.get(
            "/api/v1/stats/funnel?steps=a&window_seconds=60",
            Some("key-a")
        )
        .await
        .0,
        400
    );
    let nine = (0..9)
        .map(|i| format!("e{i}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        h.get(
            &format!("/api/v1/stats/funnel?steps={nine}&window_seconds=60"),
            Some("key-a")
        )
        .await
        .0,
        400
    );
    assert_eq!(
        h.get(
            "/api/v1/stats/funnel?steps=a,b,c&window_seconds=60",
            Some("key-a")
        )
        .await
        .0,
        200
    );
}

#[tokio::test]
async fn ac_0_5_7_c_funnel_sql_orders_steps_within_window_per_visitor() {
    let h = qharness(FakeReader::with(&[]));
    h.get(
        "/api/v1/stats/funnel?steps=$pageview@/pricing,signup,paid&window_seconds=3600",
        Some("key-a"),
    )
    .await;
    let q = &h.reader.calls()[0];
    assert!(
        q.sql.contains("s3 AS")
            && q.sql
                .contains("JOIN events e ON e.visitor_id = a.visitor_id")
    );
    assert!(
        q.sql.contains("e.ts) > CAST(a.tp") || q.sql.contains("> CAST(a.tp AS BIGINT)"),
        "order enforced"
    );
    assert!(
        q.binds
            .iter()
            .filter(|b| **b == Bind::Int(3_600_000))
            .count()
            == 2,
        "window bound per later step"
    );
    assert!(
        q.binds.contains(&Bind::Str("/pricing".into()))
            && q.binds.contains(&Bind::Str("signup".into()))
    );
}

#[tokio::test]
async fn ac_0_5_7_d_funnel_response_conversion_rates() {
    let rows = [(
        "steps",
        vec![
            json!({"step": 2, "visitors": 40}),
            json!({"step": 1, "visitors": 100}),
            json!({"step": 3, "visitors": 10}),
        ],
    )];
    let h = qharness(FakeReader::with(&rows));
    let (_, b) = h
        .get(
            "/api/v1/stats/funnel?steps=a,b,c&window_seconds=60",
            Some("key-a"),
        )
        .await;
    let s = b["data"]["steps"].as_array().unwrap();
    assert_eq!(s[0]["step"], 1);
    assert_eq!(s[1]["conversion_from_start"], 0.4);
    assert_eq!(s[2]["conversion_from_previous"], 0.25);
    assert_eq!(b["data"]["window_seconds"], 60);
}

// ---- AC0.5.8 retention ------------------------------------------------------

#[tokio::test]
async fn ac_0_5_8_a_cohort_matrix_and_rates_from_rows() {
    let rows = [(
        "cohorts",
        vec![
            json!({"cohort_day": 20000, "day_n": 0, "visitors": 100}),
            json!({"cohort_day": 20000, "day_n": 1, "visitors": 40}),
            json!({"cohort_day": 20001, "day_n": 0, "visitors": 50}),
        ],
    )];
    let h = qharness(FakeReader::with(&rows));
    let (_, b) = h.get("/api/v1/stats/retention?days=7", Some("key-a")).await;
    let c = b["data"]["cohorts"].as_array().unwrap();
    assert_eq!(c[0]["size"], 100);
    assert_eq!(c[0]["rate"]["1"], 0.4);
    assert_eq!(c[1]["size"], 50);
    assert_eq!(c[0]["date"], "2024-10-04");
}

#[tokio::test]
async fn ac_0_5_8_b_site_timezone_offset_bound_and_days_validated() {
    let h = qharness(FakeReader::with(&[]));
    h.get("/api/v1/stats/retention", Some("key-a")).await;
    let q = &h.reader.calls()[0];
    assert!(
        q.binds.contains(&Bind::Int(8 * 3_600_000)),
        "Singapore offset"
    );
    assert!(q.sql.contains("first_seen") && q.sql.contains("visitor_hours"));
    assert_eq!(
        h.get("/api/v1/stats/retention?days=31", Some("key-a"))
            .await
            .0,
        400
    );
    assert_eq!(
        h.get("/api/v1/stats/retention?days=0", Some("key-a"))
            .await
            .0,
        400
    );
}

// ---- AC0.5.9 rage + browsers ------------------------------------------------

#[tokio::test]
async fn ac_0_5_9_a_rage_and_dead_rates_against_clicks() {
    let rows = [(
        "elements",
        vec![
            json!({"el_css": "a>b", "page": "/", "clicks": 20, "rage_clicks": 5, "dead_clicks": 2}),
        ],
    )];
    let h = qharness(FakeReader::with(&rows));
    let (_, b) = h.get("/api/v1/stats/rage-clicks", Some("key-a")).await;
    let e = &b["data"]["elements"][0];
    assert_eq!(
        (e["rage_rate"].as_f64(), e["dead_rate"].as_f64()),
        (Some(0.25), Some(0.1))
    );
}

#[tokio::test]
async fn ac_0_5_9_b_browsers_dimension_whitelist() {
    let h = qharness(FakeReader::with(&[]));
    for d in ["browser", "os", "device", "country"] {
        assert_eq!(
            h.get(
                &format!("/api/v1/stats/browsers-devices?by={d}"),
                Some("key-a")
            )
            .await
            .0,
            200
        );
    }
    let c = h.reader.calls();
    assert!(c[3].sql.contains("GROUP BY country"));
    assert_eq!(
        h.get("/api/v1/stats/browsers-devices?by=el_text", Some("key-a"))
            .await
            .0,
        400
    );
}

// ---- AC0.5.10 sessions ------------------------------------------------------

#[tokio::test]
async fn ac_0_5_10_a_session_filters_are_bound() {
    let h = qharness(FakeReader::with(&[]));
    h.get(
        "/api/v1/sessions?user_id=u_1&country=SG&device=mobile&has_rage=1&has_error=1&event=signup",
        Some("key-a"),
    )
    .await;
    let q = &h.reader.calls()[0];
    for v in ["u_1", "SG", "mobile", "signup"] {
        assert!(q.binds.contains(&Bind::Str(v.into())), "{v}");
        assert!(!q.sql.contains(v));
    }
    assert!(q.sql.contains("HAVING") && q.sql.contains("$rage_click") && q.sql.contains("$error"));
    assert!(q.sql.contains("GROUP BY session_id") && q.sql.contains("ORDER BY ended_at DESC"));
}

#[tokio::test]
async fn ac_0_5_10_b_session_detail_ordered_by_ts_then_seq_and_scoped() {
    let h = qharness(FakeReader::with(&[(
        "events",
        vec![json!({"ts": 1, "seq": 0, "event_name": "$session_start"})],
    )]));
    let (st, b) = h.get("/api/v1/sessions/abc-123", Some("key-a")).await;
    assert_eq!(st, 200);
    assert_eq!(b["data"]["session_id"], "abc-123");
    let q = &h.reader.calls()[0];
    assert!(q.sql.contains("ORDER BY ts, seq"));
    assert!(q.sql.contains("session_id = ?") && q.sql.contains("site_id = ?"));
    assert!(
        q.binds.contains(&Bind::Str("abc-123".into())) && q.binds.contains(&Bind::Str("s1".into()))
    );
}

#[tokio::test]
async fn ac_0_5_10_c_other_sites_session_id_cannot_be_read_with_this_key() {
    let h = qharness(FakeReader::with(&[]));
    // Even when the id is known, the site bind is always the authorised site.
    h.get("/api/v1/sessions/sess_of_site_b", Some("key-a"))
        .await;
    assert!(h.reader.calls()[0].binds.contains(&Bind::Str("s1".into())));
    assert!(!h.reader.calls()[0].binds.contains(&Bind::Str("s2".into())));
}

// ---- AC0.5.11 events + usage ------------------------------------------------

#[tokio::test]
async fn ac_0_5_11_a_events_cursor_pagination() {
    let rows: Vec<_> = (0..3)
        .map(|i| json!({"ts": 1000 - i, "event_name": "$click"}))
        .collect();
    let h = qharness(FakeReader::with(&[("events", rows)]));
    let (_, b) = h.get("/api/v1/events?limit=3", Some("key-a")).await;
    assert_eq!(b["data"]["next_before"], 998, "full page ⇒ cursor");
    let (_, b) = h.get("/api/v1/events?limit=50", Some("key-a")).await;
    assert!(b["data"]["next_before"].is_null(), "short page ⇒ no cursor");
    h.get("/api/v1/events?before=998", Some("key-a")).await;
    let q = h.reader.calls().pop().unwrap();
    assert!(q.sql.contains("ts < ?") && q.binds.contains(&Bind::Int(998)));
    assert_eq!(
        h.get("/api/v1/events?before=abc", Some("key-a")).await.0,
        400
    );
}

#[tokio::test]
async fn ac_0_5_11_b_event_filters_are_bound() {
    let h = qharness(FakeReader::with(&[]));
    h.get(
        "/api/v1/events?event_name=signup&path=/p&visitor_id=v1&user_id=u1",
        Some("key-a"),
    )
    .await;
    let q = &h.reader.calls()[0];
    assert_eq!(q.sql.matches(" = ?").count(), 5);
    assert!(q.sql.contains("ORDER BY ts DESC"));
}

#[tokio::test]
async fn ac_0_5_11_c_usage_shape_and_scoped_to_callers_sites() {
    let h = qharness(FakeReader::with(&[]));
    let day = greprum::spool::drainer::day_of(NOW);
    h.state.metrics.usage_add("s1", &day, |u| {
        u.accepted = 5;
        u.dropped = 1;
    });
    h.state.metrics.usage_add("s2", &day, |u| u.accepted = 99);
    let (st, b) = h.get("/api/v1/usage", Some("key-a")).await;
    assert_eq!(st, 200);
    let rows = b["data"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0]["site"].as_str(),
            rows[0]["accepted"].as_u64(),
            rows[0]["dropped"].as_u64()
        ),
        (Some("s1"), Some(5), Some(1))
    );
    assert!(
        h.reader.calls().is_empty(),
        "usage comes from gateway metrics, not the DB"
    );
}

// ---- AC0.5.12 scaled + unscaled ---------------------------------------------

#[tokio::test]
async fn ac_0_5_12_a_sampled_rate_quarter_scales_four_times() {
    let rows = [("series", vec![json!({"t": 0, "count": 10, "scaled": 40.0})])];
    let h = qharness(FakeReader::with(&rows));
    let (_, b) = h.get("/api/v1/stats/timeseries", Some("key-a")).await;
    let p = &b["data"]["series"][0];
    assert_eq!(
        p["scaled"].as_f64().unwrap(),
        p["count"].as_f64().unwrap() * 4.0
    );
}

#[tokio::test]
async fn ac_0_5_12_b_every_count_query_selects_both_columns() {
    let h = qharness(FakeReader::with(&[]));
    for p in [
        "stats/overview",
        "stats/timeseries",
        "stats/top-pages",
        "stats/entry-pages",
        "stats/journeys",
        "stats/top-elements",
    ] {
        h.get(&format!("/api/v1/{p}"), Some("key-a")).await;
    }
    for q in h.reader.calls() {
        if q.name == "visitors"
            || q.name == "identified"
            || ["dau", "wau", "mau"].contains(&q.name)
            || q.name == "identified_prev"
            || q.name == "visitors"
        {
            continue; // distinct counts are not sampled sums
        }
        assert!(
            q.sql.contains("scaled"),
            "{} lacks a scaled column: {}",
            q.name,
            q.sql
        );
    }
}
