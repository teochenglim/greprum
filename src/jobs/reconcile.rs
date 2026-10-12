//! Reconcile job (PRD §6/§7 step 7): duplicates and `seq` gaps → loss estimate.
//!
//! Browser side loss = (Σ(max seq + 1) − Σ distinct seq − shed) / Σ(max seq + 1).
//! End-of-session loss is invisible (a lost tail has no later `seq` to expose it).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionAgg {
    pub max_seq: i64,
    pub distinct_seq: i64,
    pub rows: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Report {
    pub expected: i64,
    pub received: i64,
    pub missing: i64,
    /// Fraction 0..=1.
    pub loss: f64,
    pub duplicate_rate: f64,
}

/// `shed` is the count of events the gateway deliberately shed in the same range:
/// subtracted so shedding is not mistaken for browser loss.
pub fn compute(sessions: &[SessionAgg], shed: i64) -> Report {
    let expected: i64 = sessions.iter().map(|s| s.max_seq + 1).sum();
    let received: i64 = sessions.iter().map(|s| s.distinct_seq).sum();
    let rows: i64 = sessions.iter().map(|s| s.rows).sum();
    let dups: i64 = sessions.iter().map(|s| s.rows - s.distinct_seq).sum();
    let missing = (expected - received - shed).max(0);
    Report {
        expected,
        received,
        missing,
        loss: if expected > 0 {
            missing as f64 / expected as f64
        } else {
            0.0
        },
        duplicate_rate: if rows > 0 {
            dups as f64 / rows as f64
        } else {
            0.0
        },
    }
}

/// Aggregation query. Binds: `site_id`, `from_ms`, `to_ms`.
pub fn sql() -> &'static str {
    "SELECT CAST(max(seq) AS BIGINT) AS max_seq, CAST(count(DISTINCT seq) AS BIGINT) AS distinct_seq, \
            CAST(count(*) AS BIGINT) AS n \
     FROM events WHERE site_id = ? AND ts >= ? AND ts < ? GROUP BY session_id"
}

pub async fn run_once(
    pool: &sqlx::MySqlPool,
    site: &str,
    from_ms: i64,
    to_ms: i64,
    shed: i64,
) -> anyhow::Result<Report> {
    use sqlx::Row;
    let rows = sqlx::query(sql())
        .bind(site)
        .bind(from_ms)
        .bind(to_ms)
        .fetch_all(pool)
        .await?;
    let aggs: Vec<SessionAgg> = rows
        .iter()
        .map(|r| SessionAgg {
            max_seq: r.try_get::<i64, _>(0).unwrap_or(0),
            distinct_seq: r.try_get::<i64, _>(1).unwrap_or(0),
            rows: r.try_get::<i64, _>(2).unwrap_or(0),
        })
        .collect();
    Ok(compute(&aggs, shed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(max_seq: i64, distinct_seq: i64, rows: i64) -> SessionAgg {
        SessionAgg {
            max_seq,
            distinct_seq,
            rows,
        }
    }

    #[test]
    fn ac_0_4_15_a_gap_estimate_on_fixture() {
        // session 1: seq 0..=9 but 2 missing; session 2 complete (0..=4).
        let r = compute(&[s(9, 8, 8), s(4, 5, 5)], 0);
        assert_eq!((r.expected, r.received, r.missing), (15, 13, 2));
        assert!((r.loss - 2.0 / 15.0).abs() < 1e-9);
    }

    #[test]
    fn ac_0_4_15_b_shed_events_are_not_browser_loss() {
        let r = compute(&[s(9, 7, 7)], 3);
        assert_eq!(r.missing, 0);
        assert_eq!(r.loss, 0.0);
        let r = compute(&[s(9, 7, 7)], 1);
        assert_eq!(r.missing, 2);
    }

    #[test]
    fn ac_0_4_15_c_duplicate_rate_on_fixture() {
        let r = compute(&[s(9, 10, 12), s(4, 5, 8)], 0);
        // rows 20, distinct 15 → 5 duplicates
        assert!((r.duplicate_rate - 0.25).abs() < 1e-9);
    }

    #[test]
    fn ac_0_4_15_d_empty_range_is_zero_not_nan() {
        let r = compute(&[], 0);
        assert_eq!((r.loss, r.duplicate_rate, r.expected), (0.0, 0.0, 0));
    }

    #[test]
    fn ac_0_4_15_e_sql_is_site_scoped_and_bound() {
        assert!(sql().contains("site_id = ?") && sql().matches('?').count() == 3);
    }
}
