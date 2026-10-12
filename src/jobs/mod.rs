//! Background jobs: first-seen (retention, R-S5) and reconcile (loss estimate, R-G15).

pub mod reconcile;

/// Insert visitors not yet in `first_seen`, taking their earliest hour window.
/// Idempotent: the anti-join never rewrites an existing visitor. Binds: `site_id`, `since_ms`.
pub fn first_seen_sql() -> (String, usize) {
    (
        "INSERT INTO first_seen (first_seen_ts, site_id, visitor_id) \
         SELECT v.first_ts, v.site_id, v.visitor_id FROM \
           (SELECT site_id, visitor_id, min(time_window) AS first_ts FROM visitor_hours \
            WHERE site_id = ? AND time_window >= ? GROUP BY site_id, visitor_id) v \
         LEFT JOIN first_seen f ON f.site_id = v.site_id AND f.visitor_id = v.visitor_id \
         WHERE f.visitor_id IS NULL"
            .into(),
        2,
    )
}
