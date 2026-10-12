//! L2: HTTP behaviour via the real router — AC0.4.1 – AC0.4.3, AC0.4.14, plus shed/503 wiring (AC0.4.10).
mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use common::{app::*, *};
use http_body_util::BodyExt;
use serde_json::json;

async fn body_bytes(r: axum::response::Response) -> Vec<u8> {
    r.into_body().collect().await.unwrap().to_bytes().to_vec()
}

// ---- AC0.4.1 /t.js ----------------------------------------------------------

#[tokio::test]
async fn ac_0_4_1_a_tracker_served_with_js_type_and_cache_headers() {
    let h = harness();
    let r = h
        .send(Request::get("/t.js").body(Body::empty()).unwrap())
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(
        r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/javascript")
    );
    assert!(
        r.headers()["cache-control"]
            .to_str()
            .unwrap()
            .contains("max-age")
    );
    assert!(r.headers().contains_key("etag"));
}

#[tokio::test]
async fn ac_0_4_1_b_hashed_url_serves_same_bytes_immutable() {
    let h = harness();
    let plain = body_bytes(
        h.send(Request::get("/t.js").body(Body::empty()).unwrap())
            .await,
    )
    .await;
    let r = h
        .send(
            Request::get(h.state.hashed_tracker_path().as_str())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(
        r.headers()["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    assert_eq!(body_bytes(r).await, plain);
    assert!(h.state.hashed_tracker_path().starts_with("/t-") && h.state.tracker_hash.len() == 16);
}

#[tokio::test]
async fn ac_0_4_1_c_custom_paths_honoured_default_absent() {
    let h = harness_with(|c| {
        c.server.tracker_path = "/assets/rum.js".into();
        c.server.collect_path = "/assets/ev".into();
    });
    assert_eq!(
        h.send(Request::get("/assets/rum.js").body(Body::empty()).unwrap())
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        h.send(Request::get("/t.js").body(Body::empty()).unwrap())
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let r = h
        .post(
            "/assets/ev?k=pk_test",
            Some("https://example.com"),
            batch(vec![ev("$pageview", 0)]).to_string(),
        )
        .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        h.post(
            "/collect?k=pk_test",
            Some("https://example.com"),
            "{}".into()
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

// ---- AC0.4.2 /collect protocol ----------------------------------------------

#[tokio::test]
async fn ac_0_4_2_a_text_plain_body_returns_204_and_is_spooled() {
    let h = harness();
    let r = h
        .post(
            "/collect?k=pk_test",
            Some("https://example.com"),
            batch(vec![ev("$pageview", 0)]).to_string(),
        )
        .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        r.headers()["access-control-allow-origin"],
        "https://example.com"
    );
    assert!(h.state.spool.bytes() > 0, "204 implies on spool");
    assert_eq!(
        h.state
            .metrics
            .accepted
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn ac_0_4_2_b_application_json_also_accepted() {
    let h = harness();
    let req = Request::post("/collect?k=pk_test")
        .header("content-type", "application/json")
        .header("origin", "https://example.com")
        .header("user-agent", UA)
        .body(Body::from(batch(vec![ev("$click", 0)]).to_string()))
        .unwrap();
    assert_eq!(h.send(req).await.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn ac_0_4_2_c_preflight_answered_for_allowed_origin_only() {
    let h = harness();
    let pre = |o: &str| {
        Request::builder()
            .method("OPTIONS")
            .uri("/collect?k=pk_test")
            .header("origin", o)
            .header("access-control-request-method", "POST")
            .body(Body::empty())
            .unwrap()
    };
    let r = h.send(pre("https://example.com")).await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert!(
        r.headers()["access-control-allow-headers"]
            .to_str()
            .unwrap()
            .contains("content-type")
    );
    assert_eq!(
        r.headers()["access-control-allow-origin"],
        "https://example.com"
    );
    let r = h.send(pre("https://evil.test")).await;
    assert!(!r.headers().contains_key("access-control-allow-origin"));
}

#[tokio::test]
async fn ac_0_4_2_d_malformed_json_is_400_not_204() {
    let h = harness();
    let r = h
        .post(
            "/collect?k=pk_test",
            Some("https://example.com"),
            "{not json".into(),
        )
        .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(h.state.spool.bytes(), 0);
}

// ---- AC0.4.3 gate -----------------------------------------------------------

#[tokio::test]
async fn ac_0_4_3_a_unknown_key_401_without_reading_body() {
    let h = harness();
    // A body that is never read cannot be parsed: use an endless-looking oversized payload
    // and assert the verdict is 401, not 413/400 (key is checked first).
    let big = "x".repeat(200_000);
    let r = h
        .post("/collect?k=pk_nope", Some("https://example.com"), big)
        .await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = h
        .post("/collect", Some("https://example.com"), "{}".into())
        .await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn ac_0_4_3_b_origin_not_in_allowlist_403() {
    let h = harness();
    let r = h
        .post(
            "/collect?k=pk_test",
            Some("https://evil.test"),
            batch(vec![ev("$pageview", 0)]).to_string(),
        )
        .await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(h.state.spool.bytes(), 0);
}

#[tokio::test]
async fn ac_0_4_3_c_missing_origin_403_unless_wildcard() {
    let h = harness();
    let b = batch(vec![ev("$pageview", 0)]).to_string();
    assert_eq!(
        h.post("/collect?k=pk_test", None, b.clone()).await.status(),
        StatusCode::FORBIDDEN
    );
    let h2 = harness_with(|c| c.sites[0].origins = vec!["*".into()]);
    assert_eq!(
        h2.post("/collect?k=pk_test", None, b.clone())
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h2.post("/collect?k=pk_test", Some("https://any.test"), b)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn ac_0_4_3_d_empty_origin_list_denies_everyone() {
    let h = harness_with(|c| c.sites[0].origins = vec![]);
    let r = h
        .post(
            "/collect?k=pk_test",
            Some("https://example.com"),
            batch(vec![ev("$pageview", 0)]).to_string(),
        )
        .await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn ac_0_4_3_e_rate_limit_429_then_new_minute_recovers() {
    let h = harness_with(|c| c.server.rate_limit_per_min = 3);
    for i in 0..3 {
        assert_eq!(
            h.collect(&batch(vec![ev("$pageview", i)])).await,
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(
        h.collect(&batch(vec![ev("$pageview", 9)])).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    h.now.fetch_add(61_000, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        h.collect(&batch(vec![ev("$pageview", 10)])).await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn ac_0_4_3_f_oversize_body_413_and_too_many_events_413() {
    let h = harness();
    let big =
        json!({"visitor_id":"v","session_id":"s","events":[{"name":"x","pad":"y".repeat(70_000)}]});
    assert_eq!(h.collect(&big).await, StatusCode::PAYLOAD_TOO_LARGE);
    let many = batch((0..51).map(|i| ev("$pageview", i)).collect());
    assert_eq!(h.collect(&many).await, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(h.state.spool.bytes(), 0);
}

// ---- AC0.4.10 wiring: shed counted by class, 503 when full -------------------

#[tokio::test]
async fn ac_0_4_10_d_p2_shed_counted_by_class_and_p0_503_when_full() {
    let h = harness_with(|c| c.spool.cap_bytes = 4000);
    let mut accepted = 0;
    // Fill past 50 %: P2 shed, P0 still accepted.
    while h.state.spool.fill_pct() < 55.0 {
        assert_eq!(
            h.collect(&batch(vec![ev("$pageview", accepted)])).await,
            StatusCode::NO_CONTENT
        );
        accepted += 1;
    }
    let before = h.state.spool.bytes();
    assert_eq!(
        h.collect(&batch(vec![ev("$scroll", 900)])).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(h.state.spool.bytes(), before, "P2 shed, nothing spooled");
    assert_eq!(h.state.metrics.label_value("shed_total", "p2"), 1);
    // Fill to the cap: P0 gets 503.
    let mut got_503 = false;
    for i in 0..200 {
        if h.collect(&batch(vec![ev("$pageview", 1000 + i)])).await
            == StatusCode::SERVICE_UNAVAILABLE
        {
            got_503 = true;
            break;
        }
    }
    assert!(got_503);
}

#[tokio::test]
async fn ac_0_4_10_e_p1_shed_at_80_percent_p0_still_admitted() {
    let h = harness_with(|c| c.spool.cap_bytes = 6000);
    let mut i = 0;
    while h.state.spool.fill_pct() < 82.0 {
        assert_eq!(
            h.collect(&batch(vec![ev("$pageview", i)])).await,
            StatusCode::NO_CONTENT
        );
        i += 1;
    }
    let before = h.state.spool.bytes();
    h.collect(&batch(vec![ev("$rage_click", 800)])).await;
    assert_eq!(h.state.spool.bytes(), before);
    assert_eq!(h.state.metrics.label_value("shed_total", "p1"), 1);
    assert_eq!(
        h.collect(&batch(vec![ev("$click", 801)])).await,
        StatusCode::NO_CONTENT
    );
}

// ---- dedup wiring (AC0.4.9) -------------------------------------------------

#[tokio::test]
async fn ac_0_4_9_d_retried_batch_acknowledged_but_not_spooled_twice() {
    let h = harness();
    let b = batch(vec![ev("$pageview", 0), ev("$click", 1)]);
    assert_eq!(h.collect(&b).await, StatusCode::NO_CONTENT);
    let bytes = h.state.spool.bytes();
    assert_eq!(h.collect(&b).await, StatusCode::NO_CONTENT);
    assert_eq!(h.state.spool.bytes(), bytes);
    assert_eq!(
        h.state
            .metrics
            .duplicates_dropped
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
}

// ---- AC0.4.14 health, metrics, usage ----------------------------------------

#[tokio::test]
async fn ac_0_4_14_a_healthz_200() {
    let h = harness();
    let r = h
        .send(Request::get("/healthz").body(Body::empty()).unwrap())
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(body_bytes(r).await, b"ok");
}

#[tokio::test]
async fn ac_0_4_14_b_all_prd_metrics_exposed() {
    let h = harness();
    h.collect(&batch(vec![ev("$pageview", 0), ev("$bogus", 1)]))
        .await;
    let r = h
        .send(Request::get("/metrics").body(Body::empty()).unwrap())
        .await;
    let t = String::from_utf8(body_bytes(r).await).unwrap();
    for m in [
        "accepted_total",
        "rejected_total",
        "dropped_total",
        "spool_bytes",
        "spool_cap_bytes",
        "oldest_unwritten_event_age_ms",
        "drained_fresh_total",
        "drained_backfill_total",
        "insert_latency_us_sum",
        "duplicates_dropped_total",
        "late_events_total",
        "dead_frames_total",
        "browser_loss_estimate_ppm",
        "duplicate_rate_ppm",
        "site_events_total",
    ] {
        assert!(t.contains(&format!("greprum_{m}")), "missing {m}\n{t}");
    }
    assert!(t.contains("reason=\"unknown_builtin\""));
}

#[tokio::test]
async fn ac_0_4_14_c_usage_survives_restart() {
    let h = harness();
    h.collect(&batch(vec![ev("$pageview", 0), ev("$click", 1)]))
        .await;
    greprum::server::save_usage(h.dir.path(), &h.state.metrics).unwrap();
    let fresh = greprum::metrics::Metrics::default();
    greprum::server::load_usage(h.dir.path(), &fresh);
    let u = fresh.usage_snapshot();
    assert_eq!(u.values().map(|d| d.accepted).sum::<u64>(), 2);
}

#[tokio::test]
async fn ac_0_4_14_d_rejections_and_drops_land_in_usage() {
    let h = harness();
    h.collect(&batch(vec![ev("$bogus", 0), ev("$pageview", 1)]))
        .await;
    let u: Vec<_> = h.state.metrics.usage_snapshot().into_values().collect();
    assert_eq!((u[0].accepted, u[0].dropped), (1, 1));
}
