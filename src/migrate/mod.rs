//! DDL and flows (`greprum migrate`). The only module with literal DDL (AGENTS.md).
//!
//! Every statement here was run against the pinned GreptimeDB version (see `docs/adr/`).

use anyhow::{Context, Result, bail};

/// Column names of `events`, in `StoredEvent` field order.
pub const EVENT_COLUMNS: &[&str] = &[
    "ts",
    "site_id",
    "event_name",
    "visitor_id",
    "session_id",
    "user_id",
    "is_identified",
    "is_new_visitor",
    "seq",
    "is_late",
    "path",
    "page_group",
    "parent_path",
    "referrer_host",
    "el_tag",
    "el_id",
    "el_text",
    "el_css",
    "el_attrs",
    "props",
    "ctx",
    "env",
    "release",
    "sample_rate",
    "device",
    "browser",
    "os",
    "country",
];

/// Every table a flow or job writes. Query builders may only name these (ADR 0024).
pub const ROLLUP_TABLES: &[&str] = &[
    "click_stats_1h",
    "click_stats_1m",
    "session_starts_1h",
    "page_edges_1h_tbl",
    "visitor_hours",
    "user_hours",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stmt {
    pub name: &'static str,
    pub sql: String,
}

/// `13 months`, `30 days`, `forever`. Anything else is refused: it ends up in DDL.
pub fn validate_ttl(ttl: &str) -> Result<()> {
    let t = ttl.trim();
    if t == "forever" {
        return Ok(());
    }
    let mut parts = t.split_whitespace();
    let (Some(n), Some(unit), None) = (parts.next(), parts.next(), parts.next()) else {
        bail!("ttl must look like '13 months' or 'forever'");
    };
    let unit = unit.trim_end_matches('s');
    if n.parse::<u32>().is_err()
        || !["second", "minute", "hour", "day", "week", "month", "year"].contains(&unit)
    {
        bail!("ttl must look like '13 months' or 'forever'");
    }
    Ok(())
}

pub fn events_ddl(ttl: &str) -> Result<String> {
    validate_ttl(ttl)?;
    Ok(format!(
        "CREATE TABLE IF NOT EXISTS events (
  ts             TIMESTAMP(3) NOT NULL,
  site_id        STRING,
  event_name     STRING,
  visitor_id     STRING SKIPPING INDEX,
  session_id     STRING SKIPPING INDEX,
  user_id        STRING SKIPPING INDEX,
  is_identified  BOOLEAN,
  is_new_visitor BOOLEAN,
  seq            INT,
  is_late        BOOLEAN,
  `path`         STRING,
  page_group     STRING,
  parent_path    STRING,
  referrer_host  STRING,
  el_tag STRING, el_id STRING, el_text STRING, el_css STRING,
  el_attrs       STRING,
  props          STRING,
  ctx            STRING,
  env STRING, `release` STRING,
  sample_rate    DOUBLE,
  device STRING, browser STRING, os STRING, country STRING,
  TIME INDEX (ts),
  PRIMARY KEY (site_id, event_name)
) WITH (append_mode = 'true', ttl = '{ttl}')"
    ))
}

const EXPIRE: &str = "EXPIRE AFTER '7 days'::INTERVAL";

/// Rollup flows, all built from raw `events` (ADR 0022). `*_scaled` columns carry
/// `sum(1/sample_rate)` so the API can return scaled and unscaled counts (R-Q18).
pub fn flows() -> Vec<Stmt> {
    flows_prefixed("")
}

/// Flow names are unique per *catalog*, not per database (ADR 0032): two GrepRUM installs
/// (or test databases) sharing a catalog need distinct `prefix`es. Sink tables are unaffected.
pub fn flows_prefixed(prefix: &str) -> Vec<Stmt> {
    let mut v = vec![
        Stmt {
            name: "click_rollup_1h",
            sql: format!(
                "CREATE FLOW IF NOT EXISTS click_rollup_1h SINK TO click_stats_1h {EXPIRE} AS
SELECT site_id, `path`, count(*) AS click_count, sum(1.0 / sample_rate) AS click_count_scaled,
       date_bin('1 hour'::INTERVAL, ts) AS time_window
FROM events WHERE event_name = '$click'
GROUP BY site_id, `path`, time_window"
            ),
        },
        Stmt {
            name: "click_rollup_1m",
            sql: "CREATE FLOW IF NOT EXISTS click_rollup_1m SINK TO click_stats_1m EXPIRE AFTER '1 day'::INTERVAL AS
SELECT site_id, count(*) AS click_count, sum(1.0 / sample_rate) AS click_count_scaled,
       date_bin('1 minute'::INTERVAL, ts) AS time_window
FROM events WHERE event_name = '$click'
GROUP BY site_id, time_window"
                .into(),
        },
        Stmt {
            name: "session_start_1h",
            sql: format!(
                "CREATE FLOW IF NOT EXISTS session_start_1h SINK TO session_starts_1h {EXPIRE} AS
SELECT site_id, `path`, is_new_visitor, count(*) AS sessions, sum(1.0 / sample_rate) AS sessions_scaled,
       date_bin('1 hour'::INTERVAL, ts) AS time_window
FROM events WHERE event_name = '$session_start'
GROUP BY site_id, `path`, is_new_visitor, time_window"
            ),
        },
        Stmt {
            name: "page_edges_1h",
            sql: format!(
                "CREATE FLOW IF NOT EXISTS page_edges_1h SINK TO page_edges_1h_tbl {EXPIRE} AS
SELECT site_id, parent_path, `path`, count(*) AS views, sum(1.0 / sample_rate) AS views_scaled,
       date_bin('1 hour'::INTERVAL, ts) AS time_window
FROM events WHERE event_name = '$pageview'
GROUP BY site_id, parent_path, `path`, time_window"
            ),
        },
        // Flow has no count(DISTINCT): store distinct rows via GROUP BY (no aggregate; ADR 0030).
        Stmt {
            name: "visitor_hours_flow",
            sql: format!(
                "CREATE FLOW IF NOT EXISTS visitor_hours_flow SINK TO visitor_hours {EXPIRE} AS
SELECT site_id, visitor_id, date_bin('1 hour'::INTERVAL, ts) AS time_window
FROM events
GROUP BY site_id, visitor_id, time_window"
            ),
        },
        Stmt {
            name: "user_hours_flow",
            sql: format!(
                "CREATE FLOW IF NOT EXISTS user_hours_flow SINK TO user_hours {EXPIRE} AS
SELECT site_id, user_id, date_bin('1 hour'::INTERVAL, ts) AS time_window
FROM events WHERE is_identified = true
GROUP BY site_id, user_id, time_window"
            ),
        },
    ];
    if !prefix.is_empty() {
        for f in &mut v {
            f.sql = f.sql.replacen(
                &format!("CREATE FLOW IF NOT EXISTS {} ", f.name),
                &format!("CREATE FLOW IF NOT EXISTS {prefix}{} ", f.name),
                1,
            );
        }
    }
    v
}

/// Tables written by jobs, not flows.
pub fn job_tables() -> Vec<Stmt> {
    vec![
        Stmt {
            name: "first_seen",
            sql: "CREATE TABLE IF NOT EXISTS first_seen (
  first_seen_ts TIMESTAMP(3) NOT NULL,
  site_id    STRING,
  visitor_id STRING,
  TIME INDEX (first_seen_ts),
  PRIMARY KEY (site_id, visitor_id)
)"
            .into(),
        },
        Stmt {
            name: "sessions",
            sql: "CREATE TABLE IF NOT EXISTS sessions (
  started_at TIMESTAMP(3) NOT NULL,
  site_id    STRING,
  session_id STRING,
  visitor_id STRING,
  user_id    STRING,
  ended_at   BIGINT,
  events     BIGINT,
  entry_path STRING,
  country    STRING,
  device     STRING,
  browser    STRING,
  rage_clicks BIGINT,
  errors     BIGINT,
  TIME INDEX (started_at),
  PRIMARY KEY (site_id, session_id)
)"
            .into(),
        },
    ]
}

/// Everything `greprum migrate` runs, in order.
pub fn plan(ttl: &str) -> Result<Vec<Stmt>> {
    plan_prefixed(ttl, "")
}

pub fn validate_prefix(prefix: &str) -> Result<()> {
    if prefix.len() > 32
        || !prefix
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
    {
        bail!("flow_prefix must match [a-z0-9_]{{0,32}}");
    }
    Ok(())
}

pub fn plan_prefixed(ttl: &str, prefix: &str) -> Result<Vec<Stmt>> {
    validate_prefix(prefix)?;
    let mut v = vec![Stmt {
        name: "events",
        sql: events_ddl(ttl)?,
    }];
    v.extend(job_tables());
    v.extend(flows_prefixed(prefix));
    Ok(v)
}

/// Apply the plan on `url` (MySQL protocol). Idempotent: every statement is `IF NOT EXISTS`.
pub async fn apply(url: &str, ttl: &str, flow_prefix: &str) -> Result<Vec<&'static str>> {
    use sqlx::Executor;
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .context("connecting")?;
    let mut done = vec![];
    for s in plan_prefixed(ttl, flow_prefix)? {
        // Text protocol: DDL is not prepared.
        pool.execute(sqlx::raw_sql(&s.sql))
            .await
            .with_context(|| format!("applying {}", s.name))?;
        done.push(s.name);
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::StoredEvent;

    #[test]
    fn ac_0_3_1_a_columns_equal_stored_event_fields_in_order() {
        let ddl = events_ddl("13 months").unwrap();
        let mut want = EVENT_COLUMNS.iter();
        let mut next = want.next();
        for tok in ddl.split(|c: char| c.is_whitespace() || c == ',') {
            if next.is_some_and(|n| tok.trim_matches(|c| c == '"' || c == '`') == *n) {
                next = want.next();
            }
        }
        assert!(
            next.is_none(),
            "columns missing or out of order, stuck at {next:?}"
        );
        let v = serde_json::to_value(StoredEvent::default()).unwrap();
        let mut fields: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        let mut cols: Vec<_> = EVENT_COLUMNS.iter().map(|s| s.to_string()).collect();
        fields.sort();
        cols.sort();
        assert_eq!(fields, cols);
    }

    #[test]
    fn ac_0_3_1_b_time_index_pk_skipping_and_append_mode() {
        let d = events_ddl("13 months").unwrap();
        assert!(d.contains("TIME INDEX (ts)"));
        assert!(d.contains("PRIMARY KEY (site_id, event_name)"));
        for id in ["visitor_id", "session_id", "user_id"] {
            assert!(
                d.contains(&format!("{id}     STRING SKIPPING INDEX"))
                    || d.contains(&format!("{id}      STRING SKIPPING INDEX"))
                    || d.contains(&format!("{id}     STRING SKIPPING INDEX"))
                    || d.lines()
                        .any(|l| l.contains(id) && l.contains("SKIPPING INDEX")),
                "{id}"
            );
        }
        assert!(!d.contains("INVERTED"));
        assert!(d.contains("append_mode = 'true'"));
    }

    #[test]
    fn ac_0_3_1_c_reserved_words_quoted() {
        let d = events_ddl("13 months").unwrap();
        assert!(d.contains("`path`") && d.contains("`release`"));
        for f in flows() {
            if f.sql.contains("path") {
                assert!(f.sql.contains("`path`"), "{}", f.name);
            }
        }
    }

    #[test]
    fn ac_0_3_2_a_six_flows_named_per_prd() {
        let names: Vec<_> = flows().iter().map(|f| f.name).collect();
        for want in [
            "click_rollup_1h",
            "session_start_1h",
            "page_edges_1h",
            "visitor_hours_flow",
            "user_hours_flow",
            "click_rollup_1m",
        ] {
            assert!(names.contains(&want), "{want}");
        }
        assert_eq!(names.len(), 6);
    }

    #[test]
    fn ac_0_3_2_b_every_flow_reads_raw_events_only() {
        for f in flows() {
            assert!(f.sql.contains("FROM events"), "{}", f.name);
            for t in ROLLUP_TABLES {
                assert!(
                    !f.sql.contains(&format!("FROM {t}")),
                    "{} chains from {t}",
                    f.name
                );
            }
        }
    }

    #[test]
    fn ac_0_3_2_c_count_flows_carry_scaled_columns() {
        for f in flows() {
            if f.sql.contains("count(*)") {
                assert!(f.sql.contains("sum(1.0 / sample_rate)"), "{}", f.name);
            }
        }
    }

    #[test]
    fn ac_0_3_2_d_distinct_tables_use_group_by_without_aggregate() {
        for f in flows().iter().filter(|f| f.name.ends_with("_hours_flow")) {
            assert!(!f.sql.contains("DISTINCT"), "{}", f.name);
            assert!(!f.sql.contains("count("), "{}", f.name);
            assert!(f.sql.contains("GROUP BY"));
        }
    }

    #[test]
    fn ac_0_3_3_a_plan_is_pure_and_idempotent_statements() {
        let p = plan("13 months").unwrap();
        assert!(p.len() >= 9);
        for s in &p {
            assert!(s.sql.contains("IF NOT EXISTS"), "{} not idempotent", s.name);
        }
        assert_eq!(p[0].name, "events", "events first, flows after");
    }

    #[test]
    fn ac_0_3_3_e_flow_prefix_renames_flows_only_and_is_validated() {
        let p = plan_prefixed("13 months", "acme_").unwrap();
        assert!(
            p.iter().any(|s| s
                .sql
                .contains("CREATE FLOW IF NOT EXISTS acme_click_rollup_1h SINK TO click_stats_1h"))
        );
        assert!(
            p.iter().all(|s| !s.sql.contains("SINK TO acme_")),
            "sink tables keep their names"
        );
        for bad in ["A", "x y", "a;b", &"a".repeat(33)] {
            assert!(plan_prefixed("13 months", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ac_0_3_4_a_ttl_rendered_into_ddl() {
        assert!(events_ddl("30 days").unwrap().contains("ttl = '30 days'"));
        assert!(events_ddl("forever").unwrap().contains("ttl = 'forever'"));
    }

    #[test]
    fn ac_0_3_4_b_invalid_ttl_refused_no_sql_injection() {
        for bad in [
            "'; DROP TABLE events; --",
            "13",
            "months 13",
            "13 fortnights",
            "",
            "1 day 2 hours",
        ] {
            assert!(events_ddl(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ac_0_3_5_a_first_seen_ddl_has_site_visitor_key() {
        let t = job_tables();
        let f = t.iter().find(|s| s.name == "first_seen").unwrap();
        assert!(f.sql.contains("PRIMARY KEY (site_id, visitor_id)"));
        assert!(t.iter().any(|s| s.name == "sessions"));
    }

    #[test]
    fn ac_0_3_5_b_first_seen_insert_is_incremental_and_bound() {
        let (sql, _) = crate::jobs::first_seen_sql();
        assert!(sql.contains("first_seen") && sql.contains("visitor_hours"));
        assert!(
            sql.contains("IS NULL"),
            "anti-join keeps earliest, never rewrites"
        );
        assert!(!sql.contains("SELECT *"));
    }
}
