//! Pure SQL builders. Rules (ADR 0024, AGENTS.md):
//! * table names and column/dimension names come from fixed `match`es, never request text;
//! * every request-derived value is a bound `?`;
//! * every query has `site_id = ?` bound to the authorised site.

use super::{ApiError, Bind, DAY_MS, DEFAULT_SESSIONS_RANGE_MS, Kind, MAX_RAW_RANGE_MS, Q};
use crate::config::SiteConfig;
use chrono::{Offset, TimeZone};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

pub const MAX_LIMIT: i64 = 500;
pub const SESSIONS_MAX_LIMIT: i64 = 200;
pub const MAX_FUNNEL_WINDOW_SECS: i64 = 30 * 86_400;

#[derive(Debug, Clone)]
pub struct Params {
    pub site: String,
    pub tz: chrono_tz::Tz,
    pub from: i64,
    pub to: i64,
    pub limit: i64,
    pub id: Option<String>,
    pub raw: HashMap<String, String>,
}

impl Params {
    pub fn get(&self, k: &str) -> Option<&str> {
        self.raw
            .get(k)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// UTC offset of the site time zone at `to`, in ms.
    pub fn tz_offset_ms(&self) -> i64 {
        self.tz
            .offset_from_utc_datetime(
                &chrono::DateTime::from_timestamp_millis(self.to)
                    .unwrap_or_default()
                    .naive_utc(),
            )
            .fix()
            .local_minus_utc() as i64
            * 1000
    }

    /// Parse and validate. Time caps differ by endpoint.
    pub fn parse(
        kind: Kind,
        site: &SiteConfig,
        q: &HashMap<String, String>,
        id: Option<String>,
        now: i64,
    ) -> Result<Params, ApiError> {
        let tz: chrono_tz::Tz = site.timezone.parse().unwrap_or(chrono_tz::UTC);
        let to = match q.get("to") {
            Some(v) => parse_time(v, &tz, true)?,
            None => now,
        };
        let default_span = match kind {
            Kind::Sessions | Kind::Events | Kind::RageClicks | Kind::TopElements => {
                DEFAULT_SESSIONS_RANGE_MS
            }
            _ => 7 * DAY_MS,
        };
        let from = match q.get("from") {
            Some(v) => parse_time(v, &tz, false)?,
            None => to - default_span,
        };
        if from >= to {
            return Err(ApiError::bad("`from` must be before `to`"));
        }
        // Raw-event endpoints are capped at 31 days; rollup endpoints at 400 days.
        let cap = if reads_raw_events(kind) {
            MAX_RAW_RANGE_MS
        } else {
            400 * DAY_MS
        };
        if to - from > cap {
            return Err(ApiError::bad(format!(
                "time range exceeds the {} day cap for this endpoint",
                cap / DAY_MS
            )));
        }
        let max_limit = if kind == Kind::Sessions {
            SESSIONS_MAX_LIMIT
        } else {
            MAX_LIMIT
        };
        let limit = match q.get("limit") {
            Some(v) => v
                .parse::<i64>()
                .map_err(|_| ApiError::bad("`limit` must be an integer"))?
                .clamp(1, max_limit),
            None => {
                if kind == Kind::Sessions {
                    SESSIONS_MAX_LIMIT
                } else {
                    50
                }
            }
        };
        Ok(Params {
            site: site.id.clone(),
            tz,
            from,
            to,
            limit,
            id,
            raw: q.clone(),
        })
    }
}

pub fn reads_raw_events(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::TopElements
            | Kind::Funnel
            | Kind::RageClicks
            | Kind::BrowsersDevices
            | Kind::Sessions
            | Kind::SessionDetail
            | Kind::Events
            | Kind::TopPages
    )
}

/// Accepts epoch ms, RFC 3339, or `YYYY-MM-DD` (site time zone midnight; `to` = end of that day).
pub fn parse_time(v: &str, tz: &chrono_tz::Tz, end_of_day: bool) -> Result<i64, ApiError> {
    if let Ok(n) = v.parse::<i64>() {
        return Ok(n);
    }
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(v) {
        return Ok(d.timestamp_millis());
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d") {
        let d = if end_of_day {
            d + chrono::Duration::days(1)
        } else {
            d
        };
        if let Some(t) = tz
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default())
            .earliest()
        {
            return Ok(t.timestamp_millis());
        }
    }
    Err(ApiError::bad(
        "time must be epoch ms, RFC 3339 or YYYY-MM-DD",
    ))
}

pub fn day_in_range(day: &str, from: i64, to: i64) -> bool {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|d| {
            let s = d.and_utc().timestamp_millis();
            s + DAY_MS > from && s < to
        })
        .unwrap_or(false)
}

/// Start/end (ms) of the site-local day containing `to - 1ms`.
pub fn day_bounds(p: &Params) -> (i64, i64) {
    let off = p.tz_offset_ms();
    let local = p.to - 1 + off;
    let start_local = local.div_euclid(DAY_MS) * DAY_MS;
    (start_local - off, start_local - off + DAY_MS)
}

fn q(name: &'static str, sql: impl Into<String>, binds: Vec<Bind>) -> Q {
    Q {
        name,
        sql: sql.into(),
        binds,
    }
}
fn s(v: &str) -> Bind {
    Bind::Str(v.to_string())
}

/// `site_id = ? AND <col> >= ? AND <col> < ?` and its binds. `col` is a fixed literal.
fn scoped(col: &'static str, p: &Params) -> (String, Vec<Bind>) {
    (
        format!("site_id = ? AND {col} >= ? AND {col} < ?"),
        vec![s(&p.site), Bind::Int(p.from), Bind::Int(p.to)],
    )
}

fn scoped_range(col: &'static str, p: &Params, from: i64, to: i64) -> (String, Vec<Bind>) {
    (
        format!("site_id = ? AND {col} >= ? AND {col} < ?"),
        vec![s(&p.site), Bind::Int(from), Bind::Int(to)],
    )
}

/// Distinct-count window over `visitor_hours` / `user_hours`.
fn distinct_q(
    name: &'static str,
    table: &'static str,
    col: &'static str,
    p: &Params,
    from: i64,
    to: i64,
) -> Q {
    let (w, b) = scoped_range("time_window", p, from, to);
    q(
        name,
        format!("SELECT CAST(count(DISTINCT {col}) AS BIGINT) AS n FROM {table} WHERE {w}"),
        b,
    )
}

fn totals(p: &Params, from: i64, to: i64, suffix: &'static str) -> Vec<Q> {
    let (w, b) = scoped_range("time_window", p, from, to);
    let mk = |name: &'static str, table: &'static str, cnt: &'static str, scaled: &'static str| {
        q(
            name,
            format!(
                "SELECT CAST(coalesce(sum({cnt}), 0) AS BIGINT) AS n, coalesce(sum({scaled}), 0.0) AS scaled FROM {table} WHERE {w}"
            ),
            b.clone(),
        )
    };
    let mut v = vec![
        mk(
            "sessions",
            "session_starts_1h",
            "sessions",
            "sessions_scaled",
        ),
        mk(
            "clicks",
            "click_stats_1h",
            "click_count",
            "click_count_scaled",
        ),
        mk("pageviews", "page_edges_1h_tbl", "views", "views_scaled"),
        distinct_q("identified", "user_hours", "user_id", p, from, to),
    ];
    if !suffix.is_empty() {
        for x in &mut v {
            x.name = match (x.name, suffix) {
                ("sessions", "_prev") => "sessions_prev",
                ("clicks", "_prev") => "clicks_prev",
                ("pageviews", "_prev") => "pageviews_prev",
                _ => "identified_prev",
            };
        }
    }
    v
}

// ---- planning ---------------------------------------------------------------

pub fn plan(kind: Kind, p: &Params) -> Result<Vec<Q>, ApiError> {
    match kind {
        Kind::Overview => {
            let (ds, de) = day_bounds(p);
            let mut v = totals(p, p.from, p.to, "");
            let span = p.to - p.from;
            v.extend(totals(p, p.from - span, p.from, "_prev"));
            v.push(distinct_q("dau", "visitor_hours", "visitor_id", p, ds, de));
            v.push(distinct_q(
                "wau",
                "visitor_hours",
                "visitor_id",
                p,
                de - 7 * DAY_MS,
                de,
            ));
            v.push(distinct_q(
                "mau",
                "visitor_hours",
                "visitor_id",
                p,
                de - 30 * DAY_MS,
                de,
            ));
            v.push(distinct_q(
                "visitors",
                "visitor_hours",
                "visitor_id",
                p,
                p.from,
                p.to,
            ));
            Ok(v)
        }
        Kind::Timeseries => {
            // Fixed literals only: the request value selects a row of this table.
            let (bucket_lit, minute) = match p.get("bucket").unwrap_or("1h") {
                "1m" => ("1 minute", true),
                "1h" => ("1 hour", false),
                "1d" => ("1 day", false),
                _ => return Err(ApiError::bad("bucket must be 1m, 1h or 1d")),
            };
            let (w, b) = scoped("time_window", p);
            let (table, cnt, scaled) = match (p.get("metric").unwrap_or("clicks"), minute) {
                ("clicks", true) => ("click_stats_1m", "click_count", "click_count_scaled"),
                ("clicks", false) => ("click_stats_1h", "click_count", "click_count_scaled"),
                ("pageviews", false) => ("page_edges_1h_tbl", "views", "views_scaled"),
                ("pageviews", true) => {
                    return Err(ApiError::bad("pageviews are available at 1h or 1d buckets"));
                }
                _ => return Err(ApiError::bad("metric must be clicks or pageviews")),
            };
            Ok(vec![q(
                "series",
                format!(
                    "SELECT CAST(date_bin('{bucket_lit}'::INTERVAL, time_window) AS BIGINT) AS t, \
                     CAST(sum({cnt}) AS BIGINT) AS count, sum({scaled}) AS scaled \
                     FROM {table} WHERE {w} GROUP BY t ORDER BY t"
                ),
                b,
            )])
        }
        Kind::TopPages => {
            let (we, be) = scoped("ts", p);
            let (wr, br) = scoped("time_window", p);
            match p.get("by").unwrap_or("path") {
                "path" => Ok(vec![
                    q(
                        "views",
                        format!(
                            "SELECT `path` AS page, CAST(sum(views) AS BIGINT) AS views, sum(views_scaled) AS views_scaled FROM page_edges_1h_tbl WHERE {wr} GROUP BY `path` ORDER BY views DESC LIMIT ?"
                        ),
                        [br.clone(), vec![Bind::Int(p.limit)]].concat(),
                    ),
                    q(
                        "clicks",
                        format!(
                            "SELECT `path` AS page, CAST(sum(click_count) AS BIGINT) AS clicks, sum(click_count_scaled) AS clicks_scaled FROM click_stats_1h WHERE {wr} GROUP BY `path`"
                        ),
                        br,
                    ),
                    q(
                        "visitors",
                        format!(
                            "SELECT `path` AS page, CAST(count(DISTINCT visitor_id) AS BIGINT) AS visitors FROM events WHERE {we} AND event_name = '$pageview' GROUP BY `path`"
                        ),
                        be,
                    ),
                ]),
                "page_group" => Ok(vec![q(
                    "groups",
                    format!(
                        "SELECT page_group AS page, \
                         CAST(sum(CASE WHEN event_name = '$pageview' THEN 1 ELSE 0 END) AS BIGINT) AS views, \
                         sum(CASE WHEN event_name = '$pageview' THEN 1.0 / sample_rate ELSE 0.0 END) AS views_scaled, \
                         CAST(sum(CASE WHEN event_name = '$click' THEN 1 ELSE 0 END) AS BIGINT) AS clicks, \
                         sum(CASE WHEN event_name = '$click' THEN 1.0 / sample_rate ELSE 0.0 END) AS clicks_scaled, \
                         CAST(count(DISTINCT visitor_id) AS BIGINT) AS visitors \
                         FROM events WHERE {we} AND page_group IS NOT NULL GROUP BY page_group ORDER BY views DESC LIMIT ?"
                    ),
                    [be, vec![Bind::Int(p.limit)]].concat(),
                )]),
                _ => Err(ApiError::bad("by must be path or page_group")),
            }
        }
        Kind::EntryPages => {
            let (w, b) = scoped("time_window", p);
            Ok(vec![q(
                "entries",
                format!(
                    "SELECT `path` AS page, CAST(sum(sessions) AS BIGINT) AS sessions, sum(sessions_scaled) AS sessions_scaled, \
                     CAST(sum(CASE WHEN is_new_visitor THEN sessions ELSE 0 END) AS BIGINT) AS new_visitors, \
                     CAST(sum(CASE WHEN is_new_visitor THEN 0 ELSE sessions END) AS BIGINT) AS returning_visitors \
                     FROM session_starts_1h WHERE {w} GROUP BY `path` ORDER BY sessions DESC LIMIT ?"
                ),
                [b, vec![Bind::Int(p.limit)]].concat(),
            )])
        }
        Kind::Journeys => {
            let (w, mut b) = scoped("time_window", p);
            let mut cond = w;
            if let Some(from_page) = p.get("from_page") {
                cond.push_str(" AND parent_path = ?");
                b.push(s(from_page));
            }
            b.push(Bind::Int(p.limit));
            Ok(vec![q(
                "edges",
                format!(
                    "SELECT parent_path AS from_page, `path` AS to_page, CAST(sum(views) AS BIGINT) AS views, sum(views_scaled) AS views_scaled \
                     FROM page_edges_1h_tbl WHERE {cond} GROUP BY parent_path, `path` ORDER BY views DESC LIMIT ?"
                ),
                b,
            )])
        }
        Kind::TopElements => {
            let (w, mut b) = scoped("ts", p);
            b.push(Bind::Int(p.limit));
            Ok(vec![q(
                "elements",
                format!(
                    "SELECT el_css, `path` AS page, max(el_tag) AS tag, max(el_text) AS text, \
                     CAST(count(*) AS BIGINT) AS clicks, sum(1.0 / sample_rate) AS clicks_scaled \
                     FROM events WHERE {w} AND event_name = '$click' AND el_css IS NOT NULL \
                     GROUP BY el_css, `path` ORDER BY clicks DESC LIMIT ?"
                ),
                b,
            )])
        }
        Kind::Funnel => funnel(p),
        Kind::Retention => {
            let days: i64 = match p.get("days") {
                None => 7,
                Some(v) => v
                    .parse()
                    .map_err(|_| ApiError::bad("days must be an integer"))?,
            };
            if !(1..=30).contains(&days) {
                return Err(ApiError::bad("days must be 1..=30"));
            }
            let off = p.tz_offset_ms();
            Ok(vec![q(
                "cohorts",
                "SELECT f.cohort_day AS cohort_day, v.act_day - f.cohort_day AS day_n, \
                 CAST(count(DISTINCT f.visitor_id) AS BIGINT) AS visitors \
                 FROM (SELECT visitor_id, CAST(floor((CAST(first_seen_ts AS BIGINT) + ?) / 86400000) AS BIGINT) AS cohort_day \
                       FROM first_seen WHERE site_id = ? AND first_seen_ts >= ? AND first_seen_ts < ?) f \
                 JOIN (SELECT visitor_id, CAST(floor((CAST(time_window AS BIGINT) + ?) / 86400000) AS BIGINT) AS act_day \
                       FROM visitor_hours WHERE site_id = ? AND time_window >= ?) v ON v.visitor_id = f.visitor_id \
                 WHERE v.act_day >= f.cohort_day AND v.act_day - f.cohort_day <= ? \
                 GROUP BY f.cohort_day, day_n ORDER BY f.cohort_day, day_n"
                    .to_string(),
                vec![
                    Bind::Int(off),
                    s(&p.site),
                    Bind::Int(p.from),
                    Bind::Int(p.to),
                    Bind::Int(off),
                    s(&p.site),
                    Bind::Int(p.from),
                    Bind::Int(days),
                ],
            )])
        }
        Kind::RageClicks => {
            let (w, mut b) = scoped("ts", p);
            b.push(Bind::Int(p.limit));
            Ok(vec![q(
                "elements",
                format!(
                    "SELECT el_css, `path` AS page, \
                     CAST(sum(CASE WHEN event_name = '$click' THEN 1 ELSE 0 END) AS BIGINT) AS clicks, \
                     CAST(sum(CASE WHEN event_name = '$rage_click' THEN 1 ELSE 0 END) AS BIGINT) AS rage_clicks, \
                     CAST(sum(CASE WHEN event_name = '$dead_click' THEN 1 ELSE 0 END) AS BIGINT) AS dead_clicks \
                     FROM events WHERE {w} AND event_name IN ('$click', '$rage_click', '$dead_click') AND el_css IS NOT NULL \
                     GROUP BY el_css, `path` \
                     ORDER BY (sum(CASE WHEN event_name = '$rage_click' THEN 1 ELSE 0 END) \
                             + sum(CASE WHEN event_name = '$dead_click' THEN 1 ELSE 0 END)) DESC LIMIT ?"
                ),
                b,
            )])
        }
        Kind::BrowsersDevices => {
            // Dimension is a column name: mapped through a fixed match.
            let col = match p.get("by").unwrap_or("browser") {
                "browser" => "browser",
                "os" => "os",
                "device" => "device",
                "country" => "country",
                _ => return Err(ApiError::bad("by must be browser, os, device or country")),
            };
            let (w, mut b) = scoped("ts", p);
            b.push(Bind::Int(p.limit));
            Ok(vec![q(
                "dimension",
                format!(
                    "SELECT {col} AS value, CAST(count(DISTINCT visitor_id) AS BIGINT) AS visitors, \
                     CAST(count(DISTINCT session_id) AS BIGINT) AS sessions \
                     FROM events WHERE {w} GROUP BY {col} ORDER BY visitors DESC LIMIT ?"
                ),
                b,
            )])
        }
        Kind::Sessions => sessions(p),
        Kind::SessionDetail => {
            let id =
                p.id.clone()
                    .ok_or_else(|| ApiError::bad("session id required"))?;
            let (w, mut b) = scoped("ts", p);
            // site_id stays first; the id is bound second.
            b.insert(1, Bind::Str(id));
            let w = w.replacen("site_id = ?", "site_id = ? AND session_id = ?", 1);
            Ok(vec![q(
                "events",
                format!(
                    "SELECT CAST(ts AS BIGINT) AS ts, seq, event_name, `path` AS page, page_group, el_tag, el_id, el_text, el_css, props, user_id, is_late \
                     FROM events WHERE {w} ORDER BY ts, seq LIMIT 1000"
                ),
                b,
            )])
        }
        Kind::Events => events(p),
        Kind::Usage => Ok(vec![]),
    }
}

fn funnel(p: &Params) -> Result<Vec<Q>, ApiError> {
    let window: i64 = p
        .get("window_seconds")
        .ok_or_else(|| ApiError::bad("`window_seconds` is required"))?
        .parse()
        .map_err(|_| ApiError::bad("`window_seconds` must be an integer"))?;
    if !(1..=MAX_FUNNEL_WINDOW_SECS).contains(&window) {
        return Err(ApiError::bad("`window_seconds` must be 1..=2592000"));
    }
    let raw = p
        .get("steps")
        .ok_or_else(|| ApiError::bad("`steps` is required"))?;
    let steps: Vec<(&str, Option<&str>)> = raw
        .split(',')
        .map(|t| match t.split_once('@') {
            Some((n, path)) => (n, Some(path)),
            None => (t, None),
        })
        .collect();
    if !(2..=8).contains(&steps.len()) {
        return Err(ApiError::bad("a funnel needs 2 to 8 steps"));
    }
    if steps.iter().any(|(n, _)| n.is_empty() || n.len() > 64) {
        return Err(ApiError::bad("invalid step name"));
    }
    let wms = window * 1000;
    let mut ctes = vec![];
    let mut binds = vec![];
    for (i, (name, path)) in steps.iter().enumerate() {
        let n = i + 1;
        let path_cond = if path.is_some() {
            " AND `path` = ?"
        } else {
            ""
        };
        if i == 0 {
            ctes.push(format!(
                "s1 AS (SELECT visitor_id, min(ts) AS t1, min(ts) AS tp FROM events \
                 WHERE site_id = ? AND ts >= ? AND ts < ? AND event_name = ?{path_cond} GROUP BY visitor_id)"
            ));
        } else {
            let prev = n - 1;
            let epath = if path.is_some() {
                " AND e.`path` = ?"
            } else {
                ""
            };
            ctes.push(format!(
                "s{n} AS (SELECT a.visitor_id AS visitor_id, a.t1 AS t1, min(e.ts) AS tp FROM s{prev} a \
                 JOIN events e ON e.visitor_id = a.visitor_id \
                 WHERE e.site_id = ? AND e.ts >= ? AND e.ts < ? AND e.event_name = ?{epath} \
                 AND CAST(e.ts AS BIGINT) > CAST(a.tp AS BIGINT) \
                 AND CAST(e.ts AS BIGINT) <= CAST(a.t1 AS BIGINT) + ? \
                 GROUP BY a.visitor_id, a.t1)"
            ));
        }
        binds.extend([s(&p.site), Bind::Int(p.from), Bind::Int(p.to), s(name)]);
        if let Some(path) = path {
            binds.push(s(path));
        }
        if i > 0 {
            binds.push(Bind::Int(wms));
        }
    }
    let selects: Vec<String> = (1..=steps.len())
        .map(|n| format!("SELECT {n} AS step, CAST(count(*) AS BIGINT) AS visitors FROM s{n}"))
        .collect();
    let sql = format!("WITH {} {}", ctes.join(", "), selects.join(" UNION ALL "));
    Ok(vec![q("steps", sql, binds)])
}

fn sessions(p: &Params) -> Result<Vec<Q>, ApiError> {
    let (w, mut b) = scoped("ts", p);
    let mut where_ = w;
    // Optional equality filters: column from a fixed list, value bound.
    for (param, col) in [
        ("user_id", "user_id"),
        ("country", "country"),
        ("device", "device"),
        ("browser", "browser"),
        ("visitor_id", "visitor_id"),
    ] {
        if let Some(v) = p.get(param) {
            where_.push_str(&format!(" AND {col} = ?"));
            b.push(s(v));
        }
    }
    let mut having: Vec<&str> = vec![];
    if p.get("has_rage") == Some("1") {
        having.push("sum(CASE WHEN event_name = '$rage_click' THEN 1 ELSE 0 END) > 0");
    }
    if p.get("has_error") == Some("1") {
        having.push("sum(CASE WHEN event_name = '$error' THEN 1 ELSE 0 END) > 0");
    }
    let mut having_sql = having.join(" AND ");
    if let Some(ev) = p.get("event") {
        if !having_sql.is_empty() {
            having_sql.push_str(" AND ");
        }
        having_sql.push_str("sum(CASE WHEN event_name = ? THEN 1 ELSE 0 END) > 0");
        b.push(s(ev));
    }
    let having_clause = if having_sql.is_empty() {
        String::new()
    } else {
        format!(" HAVING {having_sql}")
    };
    b.push(Bind::Int(p.limit));
    Ok(vec![q(
        "sessions",
        format!(
            "SELECT session_id, CAST(min(ts) AS BIGINT) AS started_at, CAST(max(ts) AS BIGINT) AS ended_at, \
             CAST(count(*) AS BIGINT) AS events, max(visitor_id) AS visitor_id, max(user_id) AS user_id, \
             max(country) AS country, max(device) AS device, max(browser) AS browser, \
             min(CASE WHEN event_name = '$session_start' THEN `path` END) AS entry_path, \
             CAST(sum(CASE WHEN event_name = '$rage_click' THEN 1 ELSE 0 END) AS BIGINT) AS rage_clicks, \
             CAST(sum(CASE WHEN event_name = '$error' THEN 1 ELSE 0 END) AS BIGINT) AS errors \
             FROM events WHERE {where_} GROUP BY session_id{having_clause} ORDER BY ended_at DESC LIMIT ?"
        ),
        b,
    )])
}

fn events(p: &Params) -> Result<Vec<Q>, ApiError> {
    let (w, mut b) = scoped("ts", p);
    let mut where_ = w;
    for (param, col) in [
        ("event_name", "event_name"),
        ("path", "`path`"),
        ("visitor_id", "visitor_id"),
        ("user_id", "user_id"),
        ("session_id", "session_id"),
    ] {
        if let Some(v) = p.get(param) {
            where_.push_str(&format!(" AND {col} = ?"));
            b.push(s(v));
        }
    }
    // Cursor pagination: `before` is the `ts` of the last row of the previous page.
    if let Some(before) = p.get("before") {
        let ts: i64 = before
            .parse()
            .map_err(|_| ApiError::bad("`before` must be epoch ms"))?;
        where_.push_str(" AND ts < ?");
        b.push(Bind::Int(ts));
    }
    b.push(Bind::Int(p.limit));
    Ok(vec![q(
        "events",
        format!(
            "SELECT CAST(ts AS BIGINT) AS ts, event_name, session_id, visitor_id, user_id, seq, `path` AS page, page_group, \
             el_tag, el_id, el_text, el_css, props, country, device, browser, os, sample_rate, is_late \
             FROM events WHERE {where_} ORDER BY ts DESC LIMIT ?"
        ),
        b,
    )])
}

// ---- shaping ----------------------------------------------------------------

fn first_num(rows: Option<&Vec<Value>>, key: &str) -> Value {
    rows.and_then(|r| r.first())
        .and_then(|r| r.get(key))
        .cloned()
        .unwrap_or(json!(0))
}

fn pair(r: &HashMap<&'static str, Vec<Value>>, name: &str) -> Value {
    json!({"count": first_num(r.get(name), "n"), "scaled": first_num(r.get(name), "scaled")})
}

pub fn shape(kind: Kind, p: &Params, r: &HashMap<&'static str, Vec<Value>>) -> Value {
    let rows = |n: &str| Value::Array(r.get(n).cloned().unwrap_or_default());
    match kind {
        Kind::Overview => json!({
            "dau": first_num(r.get("dau"), "n"),
            "wau": first_num(r.get("wau"), "n"),
            "mau": first_num(r.get("mau"), "n"),
            "visitors": first_num(r.get("visitors"), "n"),
            "identified_users": first_num(r.get("identified"), "n"),
            "sessions": pair(r, "sessions"),
            "clicks": pair(r, "clicks"),
            "pageviews": pair(r, "pageviews"),
            "previous": {
                "sessions": pair(r, "sessions_prev"),
                "clicks": pair(r, "clicks_prev"),
                "pageviews": pair(r, "pageviews_prev"),
                "identified_users": first_num(r.get("identified_prev"), "n"),
            },
            "pages_per_session": ratio(first_num(r.get("pageviews"), "n"), first_num(r.get("sessions"), "n")),
            "timezone": p.tz.name(),
        }),
        Kind::Timeseries => json!({"series": rows("series")}),
        Kind::TopPages => {
            if r.contains_key("groups") {
                return json!({"pages": rows("groups")});
            }
            let idx = |name: &str, key: &str| -> HashMap<String, Value> {
                r.get(name)
                    .map(|v| {
                        v.iter()
                            .filter_map(|x| {
                                Some((x.get("page")?.as_str()?.to_string(), x.get(key)?.clone()))
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let clicks = idx("clicks", "clicks");
            let clicks_s = idx("clicks", "clicks_scaled");
            let vis = idx("visitors", "visitors");
            let pages: Vec<Value> =
                r.get("views")
                    .map(|v| {
                        v.iter().filter_map(|x| x.as_object()).map(|o| {
                    let page = o.get("page").and_then(Value::as_str).unwrap_or("").to_string();
                    json!({
                        "page": page,
                        "views": o.get("views"), "views_scaled": o.get("views_scaled"),
                        "clicks": clicks.get(&page).cloned().unwrap_or(json!(0)),
                        "clicks_scaled": clicks_s.get(&page).cloned().unwrap_or(json!(0.0)),
                        "visitors": vis.get(&page).cloned().unwrap_or(json!(0)),
                    })
                }).collect()
                    })
                    .unwrap_or_default();
            json!({"pages": pages})
        }
        Kind::EntryPages => json!({"pages": rows("entries")}),
        Kind::Journeys => json!({"transitions": rows("edges")}),
        Kind::TopElements => json!({"elements": rows("elements")}),
        Kind::Funnel => {
            let steps: Vec<Value> = r.get("steps").cloned().unwrap_or_default();
            let mut sorted = steps.clone();
            sorted.sort_by_key(|v| v.get("step").and_then(Value::as_i64).unwrap_or(0));
            let first = sorted
                .first()
                .and_then(|v| v.get("visitors"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let mut prev = first;
            let out: Vec<Value> = sorted
                .iter()
                .map(|v| {
                    let n = v.get("visitors").and_then(Value::as_i64).unwrap_or(0);
                    let o = json!({
                        "step": v.get("step"), "visitors": n,
                        "conversion_from_start": if first > 0 { n as f64 / first as f64 } else { 0.0 },
                        "conversion_from_previous": if prev > 0 { n as f64 / prev as f64 } else { 0.0 },
                    });
                    prev = n;
                    o
                })
                .collect();
            json!({"window_seconds": p.get("window_seconds").and_then(|w| w.parse::<i64>().ok()), "steps": out})
        }
        Kind::Retention => {
            let mut cohorts: std::collections::BTreeMap<i64, Map<String, Value>> =
                Default::default();
            for row in r.get("cohorts").into_iter().flatten() {
                let (Some(c), Some(n), Some(v)) = (
                    row.get("cohort_day").and_then(Value::as_i64),
                    row.get("day_n").and_then(Value::as_i64),
                    row.get("visitors").and_then(Value::as_i64),
                ) else {
                    continue;
                };
                cohorts
                    .entry(c)
                    .or_default()
                    .insert(n.to_string(), json!(v));
            }
            let out: Vec<Value> = cohorts
                .into_iter()
                .map(|(day, m)| {
                    let size = m.get("0").and_then(Value::as_i64).unwrap_or(0);
                    let rates: Map<String, Value> = m
                        .iter()
                        .map(|(k, v)| (k.clone(), json!(if size > 0 { v.as_i64().unwrap_or(0) as f64 / size as f64 } else { 0.0 })))
                        .collect();
                    json!({"cohort_day": day, "date": day_string(day), "size": size, "retained": m, "rate": rates})
                })
                .collect();
            json!({"cohorts": out})
        }
        Kind::RageClicks => {
            let out: Vec<Value> = r
                .get("elements")
                .into_iter()
                .flatten()
                .map(|o| {
                    let g = |k: &str| o.get(k).and_then(Value::as_i64).unwrap_or(0);
                    let c = g("clicks").max(1) as f64;
                    let mut m = o.as_object().cloned().unwrap_or_default();
                    m.insert("rage_rate".into(), json!(g("rage_clicks") as f64 / c));
                    m.insert("dead_rate".into(), json!(g("dead_clicks") as f64 / c));
                    Value::Object(m)
                })
                .collect();
            json!({"elements": out})
        }
        Kind::BrowsersDevices => {
            json!({"rows": rows("dimension"), "by": p.get("by").unwrap_or("browser")})
        }
        Kind::Sessions => json!({"sessions": rows("sessions")}),
        Kind::SessionDetail => json!({"session_id": p.id, "events": rows("events")}),
        Kind::Events => {
            let ev = r.get("events").cloned().unwrap_or_default();
            let next = if ev.len() as i64 == p.limit {
                ev.last().and_then(|e| e.get("ts")).cloned()
            } else {
                None
            };
            json!({"events": ev, "next_before": next})
        }
        Kind::Usage => json!([]),
    }
}

fn ratio(n: Value, d: Value) -> f64 {
    let (n, d) = (n.as_f64().unwrap_or(0.0), d.as_f64().unwrap_or(0.0));
    if d > 0.0 { n / d } else { 0.0 }
}

fn day_string(day_index: i64) -> String {
    chrono::DateTime::from_timestamp_millis(day_index * DAY_MS)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}
