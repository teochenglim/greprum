//! AC0.10.4 compose stack with Grafana + Prometheus, AC0.10.5 screenshots and walkthrough.
mod common;
use common::ops;
use serde_json::Value;
use std::{fs, path::Path};

fn read(p: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(p))
        .unwrap_or_else(|e| panic!("{p}: {e}"))
}

// ---- AC0.10.4 ------------------------------------------------------------------

#[test]
fn ac_0_10_4_a_compose_declares_grafana_and_prometheus_with_provisioning_volumes() {
    let c = read("docker-compose.yml");
    for s in ["  greptime:", "  greprum:", "  prometheus:", "  grafana:"] {
        assert!(c.contains(s), "{s}");
    }
    assert!(c.contains("./ui/grafana/provisioning:/etc/grafana/provisioning:ro"));
    assert!(c.contains("./ui/grafana/dashboards:/var/lib/grafana/dashboards/greprum:ro"));
    assert!(c.contains("./ui/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml:ro"));
    assert!(
        c.contains("127.0.0.1:${GRAFANA_PORT:-3000}:3000"),
        "loopback by default"
    );
}

#[test]
fn ac_0_10_4_b_images_are_pinned_and_prometheus_scrapes_the_gateway() {
    let c = read("docker-compose.yml");
    for img in [
        "prom/prometheus:v",
        "grafana/grafana:1",
        "greptime/greptimedb:v1.2.1",
    ] {
        assert!(c.contains(img), "{img}");
    }
    assert!(!c.contains("grafana/grafana:latest") && !c.contains("prometheus:latest"));
    let p = read("ui/prometheus/prometheus.yml");
    assert!(p.contains("greprum:8080") && p.contains("/metrics"));
    let ds = read("ui/grafana/provisioning/datasources/greprum.yml");
    assert!(ds.contains("greptime:4002") && ds.contains("http://prometheus:9090"));
}

#[tokio::test]
#[ignore]
async fn ac_0_10_4_c_live_grafana_lists_all_eleven_dashboards_and_prometheus_scrapes_greprum() {
    let gp: u16 = std::env::var("GRAFANA_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    let pp: u16 = std::env::var("PROMETHEUS_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(9090);
    let (st, body) = ops::get(gp, "/api/search?tag=greprum")
        .await
        .expect("grafana reachable");
    assert_eq!(st, 200);
    let v: Vec<Value> = serde_json::from_str(&body).unwrap();
    assert_eq!(v.len(), 11, "{body}");
    let (_, t) = ops::get(pp, "/api/v1/targets")
        .await
        .expect("prometheus reachable");
    let t: Value = serde_json::from_str(&t).unwrap();
    let up = t["data"]["activeTargets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["labels"]["job"] == "greprum" && x["health"] == "up");
    assert!(up, "{t}");
}

// ---- AC0.10.5 ------------------------------------------------------------------

const SCREENS: &[&str] = &[
    "overview",
    "pages",
    "elements",
    "journeys",
    "funnels",
    "retention",
    "sessions",
    "session_detail",
    "events",
    "browsers_devices",
    "usage",
];

#[test]
fn ac_0_10_5_a_eleven_png_screenshots_exist_under_images() {
    for s in SCREENS {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("images/grafana_{s}.png"));
        let b = fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n", "{s} is not a PNG");
        assert!(b.len() > 5_000, "{s} looks empty ({} B)", b.len());
    }
}

#[test]
fn ac_0_10_5_b_walkthrough_cites_panels_that_exist_and_both_prd_questions() {
    let w = read("docs/product-owner-walkthrough.md");
    assert!(
        w.contains("Which page gets the most clicks?")
            && w.contains("Where do users drop off in the signup funnel?")
    );
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/grafana/dashboards");
    let mut titles = vec![];
    for e in fs::read_dir(dir).unwrap().flatten() {
        let d: Value = serde_json::from_str(&fs::read_to_string(e.path()).unwrap()).unwrap();
        titles.extend(
            d["panels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["title"].as_str().unwrap().to_string()),
        );
    }
    for cited in [
        "Which page gets the most clicks?",
        "Signup funnel: where do users drop off?",
        "Funnel steps",
        "Page views and visitors",
        "Entry pages",
        "What do people click?",
        "Where do people go next? (page transitions)",
        "Who comes back?",
        "Which sessions had rage clicks or errors?",
        "What did one visitor do, in order?",
        "Spool fill and age of oldest unwritten event",
    ] {
        assert!(w.contains(cited), "walkthrough does not mention {cited}");
        assert!(
            titles
                .iter()
                .any(|t| t.starts_with(cited.trim_end_matches('?').split(" (").next().unwrap())),
            "no panel titled {cited}"
        );
    }
}

#[test]
fn ac_0_10_5_c_walkthrough_embeds_the_screenshots() {
    let w = read("docs/product-owner-walkthrough.md");
    for s in ["pages", "funnels", "overview", "session_detail"] {
        assert!(w.contains(&format!("../images/grafana_{s}.png")), "{s}");
        assert!(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("images/grafana_{s}.png"))
                .exists()
        );
    }
}
