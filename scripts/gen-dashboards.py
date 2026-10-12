#!/usr/bin/env python3
"""Generates ui/grafana/dashboards/*.json (one per PRD §9 screen).

Queries are plain read-only SELECTs over the fixed GrepRUM tables, scoped by the `site`
dashboard variable and Grafana's time macros. Re-run after editing; JSON is committed.
"""
import json, os

DS = {"type": "mysql", "uid": "greprum-greptime"}
OUT = os.path.join(os.path.dirname(__file__), "..", "ui", "grafana", "dashboards")
os.makedirs(OUT, exist_ok=True)

def var_site():
    return {"name": "site", "label": "Site", "type": "query", "datasource": DS,
            "query": "SELECT DISTINCT site_id FROM visitor_hours", "refresh": 2, "includeAll": False,
            "current": {}, "sort": 1}

def var_text(name, default, label=None):
    return {"name": name, "label": label or name, "type": "textbox", "query": default,
            "current": {"text": default, "value": default}, "options": []}

def var_query(name, sql, label=None):
    """Dropdown filled by a query; Grafana selects the first row (the newest session) by default."""
    return {"name": name, "label": label or name, "type": "query", "datasource": DS, "query": sql,
            "refresh": 2, "includeAll": False, "multi": False, "current": {}, "sort": 0}

def var_custom(name, values, default):
    return {"name": name, "label": name, "type": "custom", "query": ",".join(values),
            "current": {"text": default, "value": default},
            "options": [{"text": v, "value": v, "selected": v == default} for v in values]}

def prom_target(ds, expr):
    return {"refId": "A", "datasource": ds, "expr": expr, "range": True, "instant": False}

def panel(pid, title, ptype, sql, x, y, w=12, h=8, fmt="table", desc="", links=None, ds=DS, opts=None):
    p = {"id": pid, "title": title, "type": ptype, "datasource": ds, "description": desc,
         "gridPos": {"x": x, "y": y, "w": w, "h": h},
         "targets": [prom_target(ds, sql) if ds.get("type") == "prometheus" else
                     {"refId": "A", "datasource": ds, "format": fmt, "rawQuery": True, "editorMode": "code",
                      "rawSql": " ".join(sql.split())}],
         "fieldConfig": {"defaults": {}, "overrides": []}, "options": opts or {}}
    if ds.get("type") == "prometheus":
        p["timeFrom"] = "6h"      # gateway counters/gauges: recent window regardless of the dashboard range
        p["hideTimeOverride"] = False
    if links:
        p["fieldConfig"]["defaults"]["links"] = links
    return p

def dash(uid, title, panels, variables=None, tags=None):
    d = {"uid": uid, "title": title, "tags": ["greprum"] + (tags or []), "schemaVersion": 39, "version": 1,
         "editable": True, "refresh": "1m", "time": {"from": "now-7d", "to": "now"},
         "templating": {"list": [var_site()] + (variables or [])}, "panels": panels, "annotations": {"list": []}}
    with open(os.path.join(OUT, f"{uid}.json"), "w") as f:
        json.dump(d, f, indent=2); f.write("\n")

S = "site_id = '${site}'"
TW = "$__timeFilter(time_window)"
TS = "$__timeFilter(ts)"

# 1 Overview ------------------------------------------------------------------
dash("greprum-overview", "GrepRUM / Overview", [
    panel(1, "Visitors", "stat", f"SELECT count(DISTINCT visitor_id) AS visitors FROM visitor_hours WHERE {S} AND {TW}", 0, 0, 6, 4),
    panel(2, "Sessions", "stat", f"SELECT coalesce(sum(sessions),0) AS sessions FROM session_starts_1h WHERE {S} AND {TW}", 6, 0, 6, 4),
    panel(3, "Page views", "stat", f"SELECT coalesce(sum(views),0) AS page_views FROM page_edges_1h_tbl WHERE {S} AND {TW}", 12, 0, 6, 4),
    panel(4, "Clicks", "stat", f"SELECT coalesce(sum(click_count),0) AS clicks FROM click_stats_1h WHERE {S} AND {TW}", 18, 0, 6, 4),
    panel(5, "Clicks over time", "timeseries",
          f"SELECT time_window AS time, sum(click_count) AS clicks FROM click_stats_1h WHERE {S} AND {TW} GROUP BY time_window ORDER BY time_window",
          0, 4, 12, 8, fmt="time_series"),
    panel(6, "Page views over time", "timeseries",
          f"SELECT time_window AS time, sum(views) AS page_views FROM page_edges_1h_tbl WHERE {S} AND {TW} GROUP BY time_window ORDER BY time_window",
          12, 4, 12, 8, fmt="time_series"),
    panel(7, "Visitors per day", "timeseries",
          f"SELECT date_bin('1 day'::INTERVAL, time_window) AS time, count(DISTINCT visitor_id) AS visitors FROM visitor_hours WHERE {S} AND {TW} GROUP BY 1 ORDER BY 1",
          0, 12, 24, 8, fmt="time_series"),
] + [
    panel(8 + i, f"{title}: this period vs previous", "table",
          f"""SELECT cur.n AS this_period, prev.n AS previous_period,
              CASE WHEN prev.n > 0 THEN (cur.n - prev.n) * 100.0 / prev.n ELSE NULL END AS change_pct
              FROM (SELECT coalesce(sum({col}), 0) AS n FROM {table} WHERE {S} AND {TW}) cur
              CROSS JOIN (SELECT coalesce(sum({col}), 0) AS n FROM {table} WHERE {S}
                          AND time_window >= FROM_UNIXTIME($__unixEpochFrom() * 2 - $__unixEpochTo())
                          AND time_window < FROM_UNIXTIME($__unixEpochFrom())) prev""",
          (i * 8) % 24, 20, 8, 5, desc="The selected range against the range of equal length just before it (default: this week against last).")
    for i, (title, table, col) in enumerate([("Sessions", "session_starts_1h", "sessions"),
                                             ("Page views", "page_edges_1h_tbl", "views"),
                                             ("Clicks", "click_stats_1h", "click_count")])
], tags=["overview"])

# 2 Pages ---------------------------------------------------------------------
dash("greprum-pages", "GrepRUM / Pages", [
    panel(1, "Which page gets the most clicks?", "table",
          f"SELECT `path` AS page, sum(click_count) AS clicks FROM click_stats_1h WHERE {S} AND {TW} GROUP BY `path` ORDER BY clicks DESC LIMIT 20",
          0, 0, 12, 10, desc="Top pages by click count (rollup)."),
    panel(2, "Page views and visitors", "table",
          f"SELECT `path` AS page, count(*) AS page_views, count(DISTINCT visitor_id) AS visitors FROM events WHERE {S} AND event_name = '$pageview' AND {TS} GROUP BY `path` ORDER BY page_views DESC LIMIT 50",
          12, 0, 12, 10),
    panel(3, "Entry pages", "table",
          f"SELECT `path` AS page, sum(sessions) AS sessions, sum(CASE WHEN is_new_visitor THEN sessions ELSE 0 END) AS new_visitors FROM session_starts_1h WHERE {S} AND {TW} GROUP BY `path` ORDER BY sessions DESC LIMIT 20",
          0, 10, 24, 8),
], tags=["pages"])

# 3 Elements ------------------------------------------------------------------
dash("greprum-elements", "GrepRUM / Elements", [
    panel(1, "What do people click?", "table",
          f"SELECT el_css AS element, `path` AS page, max(el_text) AS text, count(*) AS clicks FROM events WHERE {S} AND event_name = '$click' AND el_css IS NOT NULL AND {TS} GROUP BY el_css, `path` ORDER BY clicks DESC LIMIT 50",
          0, 0, 24, 10),
    panel(2, "Where do people get frustrated? (rage and dead clicks)", "table",
          f"""SELECT el_css AS element, `path` AS page,
              sum(CASE WHEN event_name = '$click' THEN 1 ELSE 0 END) AS clicks,
              sum(CASE WHEN event_name = '$rage_click' THEN 1 ELSE 0 END) AS rage_clicks,
              sum(CASE WHEN event_name = '$dead_click' THEN 1 ELSE 0 END) AS dead_clicks
              FROM events WHERE {S} AND event_name IN ('$click','$rage_click','$dead_click') AND el_css IS NOT NULL AND {TS}
              GROUP BY el_css, `path` ORDER BY rage_clicks + dead_clicks DESC LIMIT 50""",
          0, 10, 24, 10),
], tags=["elements"])

# 4 Journeys ------------------------------------------------------------------
dash("greprum-journeys", "GrepRUM / Journeys", [
    panel(1, "Where do people go next? (page transitions)", "table",
          f"SELECT parent_path AS from_page, `path` AS to_page, sum(views) AS views FROM page_edges_1h_tbl WHERE {S} AND {TW} AND coalesce(parent_path, '') LIKE '${{from_page}}' GROUP BY parent_path, `path` ORDER BY views DESC LIMIT 100",
          0, 0, 24, 16, desc="Transitions table; set the from_page variable to focus on one page ('%' = all)."),
], variables=[var_text("from_page", "%", "From page")], tags=["journeys"])

# 5 Funnels -------------------------------------------------------------------
steps = ["$session_start", "$pageview", "signup", "paid"]
funnel_sql = """WITH
 s1 AS (SELECT visitor_id, min(ts) AS t1, min(ts) AS tp FROM events WHERE site_id = '${site}' AND $__timeFilter(ts) AND event_name = '${step1}' GROUP BY visitor_id),
 s2 AS (SELECT a.visitor_id AS visitor_id, a.t1 AS t1, min(e.ts) AS tp FROM s1 a JOIN events e ON e.visitor_id = a.visitor_id WHERE e.site_id = '${site}' AND $__timeFilter(e.ts) AND e.event_name = '${step2}' AND CAST(e.ts AS BIGINT) > CAST(a.tp AS BIGINT) AND CAST(e.ts AS BIGINT) <= CAST(a.t1 AS BIGINT) + ${window_seconds} * 1000 GROUP BY a.visitor_id, a.t1),
 s3 AS (SELECT a.visitor_id AS visitor_id, a.t1 AS t1, min(e.ts) AS tp FROM s2 a JOIN events e ON e.visitor_id = a.visitor_id WHERE e.site_id = '${site}' AND $__timeFilter(e.ts) AND e.event_name = '${step3}' AND CAST(e.ts AS BIGINT) > CAST(a.tp AS BIGINT) AND CAST(e.ts AS BIGINT) <= CAST(a.t1 AS BIGINT) + ${window_seconds} * 1000 GROUP BY a.visitor_id, a.t1),
 s4 AS (SELECT a.visitor_id AS visitor_id, a.t1 AS t1, min(e.ts) AS tp FROM s3 a JOIN events e ON e.visitor_id = a.visitor_id WHERE e.site_id = '${site}' AND $__timeFilter(e.ts) AND e.event_name = '${step4}' AND CAST(e.ts AS BIGINT) > CAST(a.tp AS BIGINT) AND CAST(e.ts AS BIGINT) <= CAST(a.t1 AS BIGINT) + ${window_seconds} * 1000 GROUP BY a.visitor_id, a.t1)
SELECT 1 AS step, '${step1}' AS event, count(*) AS visitors FROM s1
UNION ALL SELECT 2, '${step2}', count(*) FROM s2
UNION ALL SELECT 3, '${step3}', count(*) FROM s3
UNION ALL SELECT 4, '${step4}', count(*) FROM s4 ORDER BY step"""
_ctes, _final = funnel_sql.split("SELECT 1 AS step", 1)
funnel_chart_sql = (_ctes + "SELECT event, visitors FROM (SELECT 1 AS step" + _final.replace(" ORDER BY step", "") + ") u ORDER BY step")
dash("greprum-funnels", "GrepRUM / Funnels", [
    panel(1, "Signup funnel: where do users drop off?", "barchart",
          funnel_chart_sql, 0, 0, 12, 12,
          desc="Ordered steps per visitor within window_seconds. Choose steps with the step variables."),
    panel(2, "Funnel steps", "table", funnel_sql, 12, 0, 12, 12),
], variables=[var_custom("window_seconds", ["300", "1800", "3600", "86400"], "1800")] +
             [var_text(f"step{i+1}", s, f"Step {i+1}") for i, s in enumerate(steps)], tags=["funnels"])

# 6 Retention -----------------------------------------------------------------
ret_sql = """WITH c AS (
  SELECT f.cohort_day AS cohort_day, v.act_day - f.cohort_day AS day_n, count(DISTINCT f.visitor_id) AS visitors
  FROM (SELECT visitor_id, CAST(floor((CAST(first_seen_ts AS BIGINT) + ${tz_offset_ms}) / 86400000) AS BIGINT) AS cohort_day
        FROM first_seen WHERE site_id = '${site}' AND $__timeFilter(first_seen_ts)) f
  JOIN (SELECT visitor_id, CAST(floor((CAST(time_window AS BIGINT) + ${tz_offset_ms}) / 86400000) AS BIGINT) AS act_day
        FROM visitor_hours WHERE site_id = '${site}') v ON v.visitor_id = f.visitor_id
  WHERE v.act_day >= f.cohort_day AND v.act_day - f.cohort_day <= 14
  GROUP BY f.cohort_day, day_n)
SELECT date_format(from_unixtime(c.cohort_day * 86400), '%Y-%m-%d') AS cohort, s.visitors AS cohort_size,
       c.day_n AS days_later, c.visitors AS returned,
       round(c.visitors * 100.0 / s.visitors, 1) AS retention_pct
FROM c JOIN (SELECT cohort_day, visitors FROM c WHERE day_n = 0) s ON s.cohort_day = c.cohort_day
ORDER BY c.cohort_day, c.day_n"""
dash("greprum-retention", "GrepRUM / Retention", [
    panel(1, "Who comes back? (visitors by cohort day and days since first visit)", "table", ret_sql, 0, 0, 24, 14),
], variables=[var_custom("tz_offset_ms", ["0", "28800000", "-18000000"], "28800000")], tags=["retention"])

# 7 Sessions ------------------------------------------------------------------
sess_sql = f"""SELECT session_id, min(ts) AS started, count(*) AS events, max(user_id) AS user_id, max(country) AS country,
 max(device) AS device, max(browser) AS browser,
 sum(CASE WHEN event_name = '$rage_click' THEN 1 ELSE 0 END) AS rage_clicks,
 sum(CASE WHEN event_name = '$error' THEN 1 ELSE 0 END) AS errors
FROM events WHERE {S} AND {TS} AND coalesce(user_id, '') LIKE '${{user_id}}' AND coalesce(country, '') LIKE '${{country}}'
GROUP BY session_id ORDER BY started DESC LIMIT 200"""
dash("greprum-sessions", "GrepRUM / Sessions", [
    panel(1, "Which sessions had rage clicks or errors?", "table", sess_sql, 0, 0, 24, 16,
          links=[{"title": "Session detail", "url": "/d/greprum-session-detail?var-site=${site}&var-session_id=${__data.fields.session_id}&${__url_time_range}"}],
          desc="Click a session id to open the session detail dashboard."),
], variables=[var_text("user_id", "%", "User id"), var_text("country", "%", "Country")], tags=["sessions"])

# 8 Session detail ------------------------------------------------------------
dash("greprum-session-detail", "GrepRUM / Session detail", [
    panel(1, "What did one visitor do, in order?", "table",
          f"SELECT ts AS time, seq, event_name, `path` AS page, el_css AS element, el_text AS text, props FROM events WHERE {S} AND session_id = '${{session_id}}' AND {TS} ORDER BY ts, seq LIMIT 1000",
          0, 0, 24, 18),
], variables=[var_query(
    "session_id",
    f"SELECT session_id FROM (SELECT session_id, max(ts) AS last_ts FROM events WHERE {S} AND {TS} "
    "GROUP BY session_id ORDER BY last_ts DESC LIMIT 200) t ORDER BY last_ts DESC",
    "Session")], tags=["sessions"])

# 9 Events --------------------------------------------------------------------
dash("greprum-events", "GrepRUM / Events", [
    panel(1, "Raw events", "table",
          f"SELECT ts AS time, event_name, session_id, visitor_id, user_id, `path` AS page, el_css AS element, props FROM events WHERE {S} AND {TS} AND event_name LIKE '${{event_name}}' ORDER BY ts DESC LIMIT 500",
          0, 0, 24, 18),
], variables=[var_text("event_name", "%", "Event name")], tags=["events"])

# 10 Browsers and devices -----------------------------------------------------
def dim(pid, title, col, x, y):
    return panel(pid, title, "piechart",
                 f"SELECT {col} AS metric, count(DISTINCT visitor_id) AS visitors FROM events WHERE {S} AND {TS} AND event_name = '$session_start' GROUP BY {col} ORDER BY visitors DESC LIMIT 15",
                 x, y, 12, 8, fmt="table")
dash("greprum-browsers-devices", "GrepRUM / Browsers and devices", [
    dim(1, "Browsers", "browser", 0, 0), dim(2, "Operating systems", "os", 12, 0),
    dim(3, "Devices", "device", 0, 8), dim(4, "Countries", "country", 12, 8),
], tags=["browsers"])

# 11 Usage --------------------------------------------------------------------
PROM = {"type": "prometheus", "uid": "greprum-prometheus"}
dash("greprum-usage", "GrepRUM / Usage", [
    panel(1, "Events per day (stored)", "timeseries",
          f"SELECT date_bin('1 day'::INTERVAL, ts) AS time, count(*) AS events FROM events WHERE {S} AND {TS} GROUP BY 1 ORDER BY 1",
          0, 0, 12, 8, fmt="time_series"),
    panel(2, "Accepted, rejected, dropped per day (gateway counters)", "timeseries",
          'sum by (day, kind) (greprum_site_events_total{site="${site}"})', 12, 0, 12, 8,
          fmt="time_series", ds=PROM),
    panel(3, "Spool fill and age of oldest unwritten event", "timeseries",
          'greprum_oldest_unwritten_event_age_ms / 1000', 0, 8, 12, 8, fmt="time_series", ds=PROM),
    panel(4, "Dropped by reason (gateway counters)", "timeseries",
          'sum by (reason) (greprum_dropped_total)', 12, 8, 12, 8, fmt="time_series", ds=PROM),
], tags=["usage"])
print("dashboards:", len(os.listdir(OUT)))
