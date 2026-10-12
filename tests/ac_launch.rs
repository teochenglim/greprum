//! AC1.0.x launch hardening. Tests marked `#[ignore]` are the v1.0.0 *release gate*:
//! they need a human decision (licence) or a soak run and are run by `make release-gate`.
mod common;
use common::app::*;
use greprum::soak;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn read(p: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(p))
        .unwrap_or_else(|e| panic!("{p}: {e}"))
}

// ---- AC1.0.1 embedded UI decision --------------------------------------------

#[test]
fn ac_1_0_1_a_decision_adr_exists_accepted_and_cites_the_gap_analysis() {
    let adr = read("docs/adr/0044-embedded-ui-go-no-go.md");
    assert!(adr.contains("Status: Accepted"));
    assert!(adr.contains("docs/grafana-gap-analysis.md"));
    assert!(adr.contains("No-go") || adr.contains("Go for"));
}

#[test]
fn ac_1_0_1_b_a_go_decision_requires_a_follow_up_release_file() {
    let adr = read("docs/adr/0044-embedded-ui-go-no-go.md");
    if adr.contains("Go for") {
        assert!(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("RELEASE/v1.1.0.md")
                .exists()
        );
    } else {
        assert!(
            !Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("ui/app")
                .exists(),
            "no-go means no embedded UI sources"
        );
    }
}

// ---- AC1.0.2 licence (needs the owner's decision) -----------------------------

#[test]
#[ignore = "release gate: ADR 0027 is the owner's decision"]
fn ac_1_0_2_a_licence_adr_is_accepted() {
    let a = read("docs/adr/0027-licence-decision.md");
    assert!(a.contains("Status: Accepted"), "ADR 0027 is still Open");
}

#[test]
#[ignore = "release gate: LICENSE is added with the decision"]
fn ac_1_0_2_b_license_file_present_and_non_empty() {
    assert!(read("LICENSE").len() > 500);
}

#[test]
fn ac_1_0_2_c_notice_states_original_tracker_and_no_agpl_code() {
    let n = read("NOTICE");
    assert!(n.contains("original code") && n.contains("no AGPL-3 code"));
    assert!(
        !Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tracker/vendor")
            .exists()
    );
}

// ---- AC1.0.3 privacy / erasure docs -------------------------------------------

#[test]
fn ac_1_0_3_a_privacy_doc_has_required_sections() {
    let d = read("docs/privacy.md");
    for h in [
        "## PDPA checklist",
        "## Retention",
        "## Erasure",
        "## What is never stored",
    ] {
        assert!(d.contains(h), "{h}");
    }
}

#[test]
fn ac_1_0_3_b_erasure_section_cites_spike_one_and_default_retention_is_stated() {
    let d = read("docs/privacy.md");
    assert!(d.contains("adr/0029-delete-from-append-only-table.md"));
    assert!(d.contains("13 months"));
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("docs/adr/0029-delete-from-append-only-table.md")
            .exists()
    );
}

// ---- AC1.0.4 frozen contract ---------------------------------------------------

#[tokio::test]
async fn ac_1_0_4_a_collect_payload_fixture_is_accepted_and_stored_by_the_gateway() {
    let fixture: Value = serde_json::from_str(&read("tests/golden/collect_payload.json")).unwrap();
    let h = harness();
    assert_eq!(h.collect(&fixture).await.as_u16(), 204);
    // The fixture's own events all passed validation: nothing dropped.
    let u: Vec<_> = h.state.metrics.usage_snapshot().into_values().collect();
    assert_eq!(
        u[0].accepted as usize,
        fixture["events"].as_array().unwrap().len()
    );
    assert_eq!(u[0].dropped, 0);
}

#[test]
fn ac_1_0_4_b_fixture_keys_match_what_the_tracker_documents() {
    let f: Value = serde_json::from_str(&read("tests/golden/collect_payload.json")).unwrap();
    for k in [
        "token",
        "visitor_id",
        "session_id",
        "sent_at",
        "sample_rate",
        "env",
        "release",
        "ctx",
        "events",
    ] {
        assert!(f.get(k).is_some(), "{k}");
    }
    let readme = read("tracker/README.md");
    for k in [
        "visitor_id",
        "session_id",
        "sent_at",
        "sample_rate",
        "seq",
        "parent_path",
    ] {
        assert!(readme.contains(k), "tracker README does not document {k}");
    }
}

fn shape(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), shape(x))).collect()),
        Value::Array(a) => Value::Array(a.first().map(|x| vec![shape(x)]).unwrap_or_default()),
        Value::Null => json!("null"),
        Value::Bool(_) => json!("bool"),
        Value::Number(_) => json!("number"),
        Value::String(_) => json!("string"),
    }
}

#[tokio::test]
async fn ac_1_0_4_c_every_endpoint_response_shape_matches_the_golden_snapshot() {
    use common::qapp::*;
    let rows: Vec<(&str, Vec<Value>)> = vec![
        ("dau", vec![json!({"n": 1})]),
        ("wau", vec![json!({"n": 1})]),
        ("mau", vec![json!({"n": 1})]),
        ("visitors", vec![json!({"n": 1})]),
        ("identified", vec![json!({"n": 1})]),
        (
            "sessions",
            vec![
                json!({"session_id": "s", "started_at": 1, "ended_at": 2, "events": 3, "visitor_id": "v", "user_id": "u", "country": "SG", "device": "d", "browser": "b", "entry_path": "/", "rage_clicks": 0, "errors": 0, "n": 1, "scaled": 1.0}),
            ],
        ),
        ("pageviews", vec![json!({"n": 1, "scaled": 1.0})]),
        ("clicks", vec![json!({"n": 1, "scaled": 1.0})]),
        ("sessions_prev", vec![json!({"n": 1, "scaled": 1.0})]),
        ("clicks_prev", vec![json!({"n": 1, "scaled": 1.0})]),
        ("pageviews_prev", vec![json!({"n": 1, "scaled": 1.0})]),
        ("identified_prev", vec![json!({"n": 1})]),
        ("series", vec![json!({"t": 1, "count": 1, "scaled": 1.0})]),
        (
            "views",
            vec![json!({"page": "/", "views": 1, "views_scaled": 1.0})],
        ),
        (
            "clicks",
            vec![json!({"page": "/", "clicks": 1, "clicks_scaled": 1.0, "n": 1, "scaled": 1.0})],
        ),
        (
            "visitors",
            vec![json!({"page": "/", "visitors": 1, "n": 1})],
        ),
        (
            "entries",
            vec![
                json!({"page": "/", "sessions": 1, "sessions_scaled": 1.0, "new_visitors": 1, "returning_visitors": 0}),
            ],
        ),
        (
            "edges",
            vec![json!({"from_page": "/", "to_page": "/a", "views": 1, "views_scaled": 1.0})],
        ),
        (
            "elements",
            vec![
                json!({"el_css": "a", "page": "/", "tag": "a", "text": "t", "clicks": 1, "clicks_scaled": 1.0, "rage_clicks": 0, "dead_clicks": 0}),
            ],
        ),
        (
            "steps",
            vec![
                json!({"step": 1, "visitors": 2}),
                json!({"step": 2, "visitors": 1}),
            ],
        ),
        (
            "cohorts",
            vec![json!({"cohort_day": 20000, "day_n": 0, "visitors": 2})],
        ),
        (
            "dimension",
            vec![json!({"value": "Chrome", "visitors": 1, "sessions": 1})],
        ),
        (
            "events",
            vec![json!({"ts": 1, "event_name": "$click", "seq": 0, "page": "/"})],
        ),
    ];
    let h = qharness(FakeReader::with(&rows));
    let mut got = serde_json::Map::new();
    for (path, kind) in greprum::query::ENDPOINTS {
        let uri = path.replace("{id}", "sess_1") + "?steps=a,b&window_seconds=60";
        let (st, body) = h.get(&uri, Some("key-a")).await;
        assert_eq!(st, 200, "{path}");
        got.insert(format!("{kind:?}"), shape(&body["data"]));
    }
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/api_shapes.json");
    if std::env::var("BLESS").is_ok() {
        fs::write(
            &golden_path,
            serde_json::to_string_pretty(&got).unwrap() + "\n",
        )
        .unwrap();
    }
    let want: Value = serde_json::from_str(&fs::read_to_string(&golden_path).unwrap()).unwrap();
    assert_eq!(
        Value::Object(got),
        want,
        "response shape changed: this is a v1.0 contract break (BLESS=1 to accept)"
    );
}

// ---- AC1.0.5 success metric ----------------------------------------------------

#[test]
fn ac_1_0_5_a_slo_maths_99_9_percent_within_5_seconds() {
    let ok: Vec<u64> = (0..10_000)
        .map(|i| if i < 9 { 6_000 } else { 100 })
        .collect();
    assert!(soak::verdict(&ok).pass, "9 late of 10 000 = 99.91 %");
    let bad: Vec<u64> = (0..10_000)
        .map(|i| if i < 20 { 6_000 } else { 100 })
        .collect();
    assert!(!soak::verdict(&bad).pass);
    assert_eq!(soak::within(&[], 5_000), 1.0);
    assert_eq!(soak::within(&[5_000, 5_001], 5_000), 0.5);
}

#[tokio::test]
async fn ac_1_0_5_b_in_process_soak_spool_to_sink_meets_the_slo() {
    use async_trait::async_trait;
    use greprum::{
        config::IngestConfig,
        model::StoredEvent,
        sink::{Sink, SinkError},
        spool::drainer::{Drainer, Step},
    };
    use std::sync::{Arc, Mutex};

    struct Timing(Mutex<Vec<u64>>);
    #[async_trait]
    impl Sink for Timing {
        async fn write(&self, ev: &[StoredEvent]) -> Result<(), SinkError> {
            let now = common::db::api::now_ms();
            self.0
                .lock()
                .unwrap()
                .extend(ev.iter().map(|e| (now - e.ts).max(0) as u64));
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let m = Arc::new(greprum::metrics::Metrics::default());
    let spool =
        greprum::spool::Spool::open(dir.path(), common::spool::spool_cfg(), m.clone()).unwrap();
    let sink = Arc::new(Timing(Mutex::new(vec![])));
    let mut d = Drainer::new(spool.clone(), sink.clone(), IngestConfig::default(), m);
    for i in 0..400 {
        let now = common::db::api::now_ms();
        let mut b = common::spool::batch("soak", i * 50, 50, now);
        b.events.iter_mut().for_each(|e| e.ts = now);
        spool.append(&b).await.unwrap();
        if i % 4 == 0 {
            while d.step_fresh().await != Step::Idle {}
        }
    }
    while d.step_fresh().await != Step::Idle {}
    let lat = sink.0.lock().unwrap().clone();
    assert_eq!(lat.len(), 20_000);
    let v = soak::verdict(&lat);
    assert!(v.pass, "{v:?}");
}

#[test]
#[ignore = "release gate: needs a built tracker (make tracker)"]
fn ac_1_0_5_c_tracker_budgets_hold_in_the_built_artifact() {
    let out = std::process::Command::new("node")
        .arg("tracker/size.mjs")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("node");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}
