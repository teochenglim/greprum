//! AC0.6.x: dashboards as data. Static checks run everywhere; one live test executes
//! every panel query on a real GreptimeDB (`make test-db`).
mod common;
use serde_json::Value;
use std::{collections::HashSet, fs, path::Path};

fn dashboards() -> Vec<(String, Value)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/grafana/dashboards");
    let mut v: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let j: Value = serde_json::from_str(&fs::read_to_string(e.path()).unwrap())
                .unwrap_or_else(|x| panic!("{name}: {x}"));
            (name, j)
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn sqls(d: &Value) -> Vec<(String, String, String)> {
    d["panels"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| {
            p["targets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| {
                    (
                        p["title"].as_str().unwrap().to_string(),
                        p["datasource"]["type"].as_str().unwrap_or("").to_string(),
                        t["rawSql"]
                            .as_str()
                            .or(t["expr"].as_str())
                            .unwrap_or("")
                            .to_string(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

const SCREENS: &[&str] = &[
    "overview",
    "pages",
    "elements",
    "journeys",
    "funnels",
    "retention",
    "sessions",
    "session-detail",
    "events",
    "browsers-devices",
    "usage",
];

/// Grafana macro expansion as the MySQL datasource does it.
fn expand(sql: &str, from_ms: i64, to_ms: i64, vars: &[(String, String)]) -> String {
    let mut s = sql.to_string();
    while let Some(i) = s.find("$__timeFilter(") {
        let j = s[i..].find(')').unwrap() + i;
        let col = s[i + 14..j].to_string();
        s.replace_range(
            i..=j,
            &format!(
                "{col} BETWEEN FROM_UNIXTIME({}) AND FROM_UNIXTIME({})",
                from_ms / 1000,
                to_ms / 1000
            ),
        );
    }
    s = s.replace("$__unixEpochFrom()", &(from_ms / 1000).to_string());
    s = s.replace("$__unixEpochTo()", &(to_ms / 1000).to_string());
    for (k, v) in vars {
        s = s.replace(&format!("${{{k}}}"), v);
    }
    s
}

// ---- AC0.6.1 ------------------------------------------------------------------

#[test]
fn ac_0_6_1_a_one_dashboard_per_prd_screen() {
    let names: Vec<_> = dashboards().into_iter().map(|(n, _)| n).collect();
    for s in SCREENS {
        assert!(names.contains(&format!("greprum-{s}.json")), "missing {s}");
    }
    assert_eq!(names.len(), 11);
}

#[test]
fn ac_0_6_1_b_uids_unique_and_match_file_names() {
    let mut seen = HashSet::new();
    for (name, d) in dashboards() {
        let uid = d["uid"].as_str().unwrap();
        assert!(seen.insert(uid.to_string()), "duplicate uid {uid}");
        assert_eq!(format!("{uid}.json"), name);
    }
}

#[test]
fn ac_0_6_1_c_every_dashboard_has_panels_and_site_variable() {
    for (name, d) in dashboards() {
        assert!(!d["panels"].as_array().unwrap().is_empty(), "{name}");
        let vars = d["templating"]["list"].as_array().unwrap();
        assert!(
            vars.iter().any(|v| v["name"] == "site"),
            "{name} lacks the site variable"
        );
    }
}

#[test]
fn ac_0_6_1_d_provisioning_files_reference_the_dashboard_directory_and_datasource_uids() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/grafana/provisioning");
    let ds = fs::read_to_string(root.join("datasources/greprum.yml")).unwrap();
    assert!(ds.contains("uid: greprum-greptime") && ds.contains("type: mysql"));
    let dp = fs::read_to_string(root.join("dashboards/greprum.yml")).unwrap();
    assert!(dp.contains("/var/lib/grafana/dashboards/greprum"));
}

// ---- AC0.6.2 ------------------------------------------------------------------

#[test]
fn ac_0_6_2_a_no_hard_coded_site_ids() {
    for (name, d) in dashboards() {
        for (title, ty, sql) in sqls(&d) {
            if ty != "mysql" {
                continue;
            }
            assert!(
                sql.contains("site_id = '${site}'"),
                "{name}/{title}: not scoped by the site variable"
            );
            let low = sql.to_lowercase();
            for bad in ["site_id = 'demo'", "site_id = 'web'", "site_id = 's1'"] {
                assert!(!low.contains(bad), "{name}/{title}: {bad}");
            }
        }
    }
}

#[test]
fn ac_0_6_2_b_only_read_only_selects_on_known_tables() {
    let ok: HashSet<&str> = greprum::migrate::ROLLUP_TABLES
        .iter()
        .copied()
        .chain(["events", "first_seen", "sessions"])
        .collect();
    for (name, d) in dashboards() {
        for (title, ty, sql) in sqls(&d) {
            if ty != "mysql" {
                continue;
            }
            let low = sql.trim_start().to_lowercase();
            assert!(
                low.starts_with("select") || low.starts_with("with"),
                "{name}/{title}"
            );
            for bad in [
                "insert ",
                "update ",
                "delete ",
                "drop ",
                "alter ",
                "create ",
                "truncate ",
            ] {
                assert!(!low.contains(bad), "{name}/{title}: {bad}");
            }
            let toks: Vec<&str> = sql
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .collect();
            // CTE names: `<name> AS (`
            let ctes: HashSet<&str> = toks
                .windows(2)
                .filter(|w| w[1].eq_ignore_ascii_case("as"))
                .map(|w| w[0])
                .collect();
            for w in toks.windows(2) {
                if w[0].eq_ignore_ascii_case("from") || w[0].eq_ignore_ascii_case("join") {
                    let t = w[1];
                    let cte = ctes.contains(t) || (t.starts_with('s') && t.len() <= 2) || t == "u";
                    assert!(
                        ok.contains(t) || cte || t.is_empty(),
                        "{name}/{title}: unexpected table {t}"
                    );
                }
            }
        }
    }
}

#[test]
fn ac_0_6_2_c_time_scoped_panels_use_grafana_time_macros() {
    for (name, d) in dashboards() {
        for (title, ty, sql) in sqls(&d) {
            if ty != "mysql"
                || name.contains("retention")
                    && sql.contains("visitor_hours WHERE")
                    && !sql.contains("first_seen")
            {
                continue;
            }
            assert!(
                sql.contains("$__timeFilter(") || sql.contains("$__unixEpochFrom()"),
                "{name}/{title}: no time macro"
            );
        }
    }
}

// ---- AC0.6.3 ------------------------------------------------------------------

fn all_titles() -> Vec<String> {
    dashboards()
        .iter()
        .flat_map(|(_, d)| {
            d["panels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["title"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn ac_0_6_3_a_most_clicked_page_panel_exists_and_reads_click_rollup() {
    let (_, d) = dashboards()
        .into_iter()
        .find(|(n, _)| n == "greprum-pages.json")
        .unwrap();
    let p = d["panels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["title"] == "Which page gets the most clicks?")
        .expect("panel");
    let sql = p["targets"][0]["rawSql"].as_str().unwrap();
    assert!(sql.contains("click_stats_1h") && sql.contains("ORDER BY clicks DESC"));
}

#[test]
fn ac_0_6_3_b_signup_funnel_panel_uses_step_and_window_variables() {
    let (_, d) = dashboards()
        .into_iter()
        .find(|(n, _)| n == "greprum-funnels.json")
        .unwrap();
    assert!(
        all_titles()
            .iter()
            .any(|t| t == "Signup funnel: where do users drop off?")
    );
    let vars: Vec<_> = d["templating"]["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    for v in ["step1", "step2", "step3", "step4", "window_seconds"] {
        assert!(vars.contains(&v.to_string()), "{v}");
    }
    let sql = d["panels"][0]["targets"][0]["rawSql"].as_str().unwrap();
    assert!(
        sql.contains("${window_seconds}") && sql.contains("${step1}") && sql.contains("${step4}")
    );
}

#[test]
fn ac_0_6_3_c_funnel_sql_enforces_order_and_window_like_the_api() {
    let (_, d) = dashboards()
        .into_iter()
        .find(|(n, _)| n == "greprum-funnels.json")
        .unwrap();
    let sql = d["panels"][0]["targets"][0]["rawSql"].as_str().unwrap();
    assert!(
        sql.matches("> CAST(a.tp AS BIGINT)").count() == 3,
        "each later step must be after the previous"
    );
    assert!(
        sql.matches("${window_seconds} * 1000").count() == 3,
        "window applied to every later step"
    );
    // the API builder is the reference: same shape
    assert!(sql.contains("JOIN events e ON e.visitor_id = a.visitor_id"));
}

// ---- AC0.6.4 ------------------------------------------------------------------

#[test]
fn ac_0_6_4_a_sessions_table_links_to_session_detail_with_session_id() {
    let (_, d) = dashboards()
        .into_iter()
        .find(|(n, _)| n == "greprum-sessions.json")
        .unwrap();
    let links = d["panels"][0]["fieldConfig"]["defaults"]["links"]
        .as_array()
        .expect("links");
    let url = links[0]["url"].as_str().unwrap();
    assert!(url.starts_with("/d/greprum-session-detail"));
    assert!(
        url.contains("var-session_id=${__data.fields.session_id}")
            && url.contains("var-site=${site}")
    );
    let (_, det) = dashboards()
        .into_iter()
        .find(|(n, _)| n == "greprum-session-detail.json")
        .unwrap();
    assert!(
        det["templating"]["list"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"] == "session_id")
    );
}

#[test]
fn ac_0_6_4_b_journeys_are_a_transitions_table() {
    let (_, d) = dashboards()
        .into_iter()
        .find(|(n, _)| n == "greprum-journeys.json")
        .unwrap();
    let p = &d["panels"][0];
    assert_eq!(p["type"], "table");
    assert!(
        p["targets"][0]["rawSql"]
            .as_str()
            .unwrap()
            .contains("page_edges_1h_tbl")
    );
}

// ---- AC0.6.5 ------------------------------------------------------------------

fn gap_doc() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/grafana-gap-analysis.md"))
        .unwrap()
}

#[test]
fn ac_0_6_5_a_gap_doc_covers_the_three_prd_questions() {
    let g = gap_doc();
    for h in [
        "## Funnel steps",
        "## Session drill-down",
        "## Journey diagram",
        "## Recommendation",
    ] {
        assert!(g.contains(h), "{h}");
    }
}

#[test]
fn ac_0_6_5_b_every_gap_has_evidence_and_a_verdict() {
    let g = gap_doc();
    assert_eq!(g.matches("Evidence:").count(), 3);
    assert_eq!(g.matches("Verdict:").count(), 3);
    for (_, d) in dashboards() {
        // evidence references existing dashboards
        let uid = d["uid"].as_str().unwrap();
        if ["greprum-funnels", "greprum-sessions", "greprum-journeys"].contains(&uid) {
            assert!(g.contains(uid), "{uid} not cited");
        }
    }
}

// ---- live: every panel query executes -----------------------------------------

#[tokio::test]
#[ignore]
async fn ac_0_6_2_d_every_dashboard_query_executes_on_live_greptimedb() {
    use common::db::*;
    let db = fresh().await;
    let now = api::now_ms();
    let from = now - 3 * 86_400_000;
    let mut n = 0;
    for (name, d) in dashboards() {
        let mut vars: Vec<(String, String)> = vec![("site".into(), "gf".into())];
        for v in d["templating"]["list"].as_array().unwrap() {
            if v["name"] == "site" {
                continue;
            }
            let value = if v["type"] == "query" {
                // Grafana picks the first row of a query variable.
                let q = expand(v["query"].as_str().unwrap(), from, now, &vars);
                db.reader_fetch(&q)
                    .await
                    .unwrap_or_else(|e| panic!("{name}: variable {}: {e}\n{q}", v["name"]))
                    .first()
                    .and_then(|r| r.as_object().and_then(|o| o.values().next().cloned()))
                    .and_then(|x| x.as_str().map(str::to_string))
                    .unwrap_or_default()
            } else {
                v["current"]["value"]
                    .as_str()
                    .or(v["query"].as_str())
                    .unwrap_or("")
                    .to_string()
            };
            vars.push((v["name"].as_str().unwrap().into(), value));
        }
        for (title, ty, sql) in sqls(&d) {
            if ty != "mysql" {
                continue;
            }
            let s = expand(&sql, from, now, &vars);
            assert!(
                !s.contains("${"),
                "{name}/{title}: unresolved variable in {s}"
            );
            db.reader_fetch(&s)
                .await
                .unwrap_or_else(|e| panic!("{name}/{title}: {e}\n{s}"));
            n += 1;
        }
    }
    assert!(n >= 18, "{n}");
}

// ---- AC0.9.6 week against last ------------------------------------------------

fn overview_compare_panels() -> Vec<Value> {
    let (_, d) = dashboards()
        .into_iter()
        .find(|(n, _)| n == "greprum-overview.json")
        .unwrap();
    d["panels"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| {
            p["title"]
                .as_str()
                .unwrap()
                .ends_with("this period vs previous")
        })
        .cloned()
        .collect()
}

#[test]
fn ac_0_9_6_a_overview_has_a_previous_period_panel_for_sessions_page_views_and_clicks() {
    let p = overview_compare_panels();
    let titles: Vec<_> = p
        .iter()
        .map(|x| x["title"].as_str().unwrap().to_string())
        .collect();
    for t in ["Sessions", "Page views", "Clicks"] {
        assert!(
            titles.contains(&format!("{t}: this period vs previous")),
            "{t}"
        );
    }
    for x in &p {
        let sql = x["targets"][0]["rawSql"].as_str().unwrap();
        assert!(
            sql.contains("$__unixEpochFrom() * 2 - $__unixEpochTo()"),
            "window of equal length before the range"
        );
        assert!(sql.contains("change_pct") && sql.contains("site_id = '${site}'"));
    }
}

#[tokio::test]
#[ignore]
async fn ac_0_9_6_b_previous_period_numbers_are_right_on_live_data() {
    use common::db::*;
    let db = fresh().await;
    let now = api::now_ms();
    let day = 86_400_000;
    let ev = |s: &str, seq: i32, ts: i64| greprum::model::StoredEvent {
        site_id: "wk".into(),
        event_name: "$session_start".into(),
        visitor_id: format!("v{seq}"),
        session_id: s.into(),
        seq,
        ts,
        path: "/".into(),
        sample_rate: 1.0,
        ..Default::default()
    };
    // 3 sessions in the last 7 days arrive fresh and go through the flow.
    let evs: Vec<_> = (0..3)
        .map(|i| ev(&format!("c{i}"), i, now - day - i as i64 * 1000))
        .collect();
    db.write(&evs).await;
    db.wait_count(
        "SELECT CAST(coalesce(sum(sessions),0) AS BIGINT) FROM session_starts_1h WHERE site_id = 'wk'",
        3,
    )
    .await;
    // The previous week's rollups were written when that data was fresh; flows ignore data older
    // than EXPIRE AFTER (ADR 0041), so seed the sink table the way it looks after ageing.
    let w = ((now - 9 * day) / 3_600_000) * 3_600_000;
    db.exec(&format!(
        "INSERT INTO session_starts_1h (site_id, `path`, is_new_visitor, sessions, sessions_scaled, time_window) \
         VALUES ('wk', '/', true, 2, 2.0, {w})"
    ))
    .await;
    let p = overview_compare_panels()
        .into_iter()
        .find(|x| x["title"] == "Sessions: this period vs previous")
        .unwrap();
    let sql = expand(
        p["targets"][0]["rawSql"].as_str().unwrap(),
        now - 7 * day,
        now,
        &[("site".into(), "wk".into())],
    );
    let rows = db.reader_fetch(&sql).await.unwrap();
    let r = &rows[0];
    assert_eq!(r["this_period"].as_i64(), Some(3), "{r}");
    assert_eq!(r["previous_period"].as_i64(), Some(2));
    assert!((r["change_pct"].as_f64().unwrap() - 50.0).abs() < 1e-6);
}
