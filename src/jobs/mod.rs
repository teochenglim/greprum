//! Background jobs: first-seen (retention, R-S5) and reconcile (loss estimate, R-G15),
//! scheduled inside `serve` (R-J1).
//!
//! Jobs are idempotent, so several gateways may run them against one database without
//! leader election: the first-seen insert is an anti-join keyed by `(site_id, visitor_id)`
//! with a deterministic timestamp, and reconcile only sets gauges.

pub mod reconcile;

use crate::metrics::Metrics;
use async_trait::async_trait;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, atomic::Ordering::Relaxed},
};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;

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

/// What the scheduler runs. `SqlJobs` talks to GreptimeDB; tests use a fake.
#[async_trait]
pub trait JobRunner: Send + Sync {
    async fn first_seen(&self, site: &str, since_ms: i64) -> anyhow::Result<()>;
    async fn reconcile(
        &self,
        site: &str,
        from_ms: i64,
        to_ms: i64,
        shed: i64,
    ) -> anyhow::Result<reconcile::Report>;
}

pub struct SqlJobs {
    pool: sqlx::MySqlPool,
}

impl SqlJobs {
    pub fn lazy(url: &str) -> anyhow::Result<Self> {
        let pool = sqlx::mysql::MySqlPoolOptions::new()
            .max_connections(2)
            .acquire_timeout(std::time::Duration::from_secs(5))
            .connect_lazy(url)?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: sqlx::MySqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl JobRunner for SqlJobs {
    async fn first_seen(&self, site: &str, since_ms: i64) -> anyhow::Result<()> {
        let (sql, _) = first_seen_sql();
        // Fixed statement from `first_seen_sql()`; values are bound.
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(site)
            .bind(since_ms)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn reconcile(
        &self,
        site: &str,
        from_ms: i64,
        to_ms: i64,
        shed: i64,
    ) -> anyhow::Result<reconcile::Report> {
        reconcile::run_once(&self.pool, site, from_ms, to_ms, shed).await
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TickReport {
    pub ran: Vec<String>,
    pub failed: Vec<String>,
}

pub struct Scheduler {
    runner: Arc<dyn JobRunner>,
    sites: Vec<String>,
    first_seen_ms: i64,
    reconcile_ms: i64,
    metrics: Arc<Metrics>,
    /// `(job, site)` → last run (epoch ms).
    last: Mutex<HashMap<(&'static str, String), i64>>,
    /// Shed total at the previous reconcile; the delta is "shed since last run".
    last_shed: Mutex<u64>,
}

impl Scheduler {
    pub fn new(
        runner: Arc<dyn JobRunner>,
        sites: Vec<String>,
        first_seen_secs: u64,
        reconcile_secs: u64,
        metrics: Arc<Metrics>,
    ) -> Scheduler {
        Scheduler {
            runner,
            sites,
            first_seen_ms: first_seen_secs as i64 * 1000,
            reconcile_ms: reconcile_secs as i64 * 1000,
            metrics,
            last: Mutex::new(HashMap::new()),
            last_shed: Mutex::new(0),
        }
    }

    fn last_run(&self, job: &'static str, site: &str) -> Option<i64> {
        self.last
            .lock()
            .ok()
            .and_then(|m| m.get(&(job, site.to_string())).copied())
    }

    fn due(&self, job: &'static str, site: &str, now: i64, every: i64) -> bool {
        self.last_run(job, site).is_none_or(|t| now - t >= every)
    }

    fn mark(&self, job: &'static str, site: &str, now: i64) {
        if let Ok(mut m) = self.last.lock() {
            m.insert((job, site.to_string()), now);
        }
        self.metrics
            .set_label("job_last_run_timestamp_seconds", job, (now / 1000) as u64);
    }

    /// Run every due job once. Failures are counted and never stop the loop.
    pub async fn tick(&self, now_ms: i64) -> TickReport {
        let mut rep = TickReport::default();
        for site in &self.sites {
            if self.due("first_seen", site, now_ms, self.first_seen_ms) {
                // First run scans everything; later runs only the last two days.
                let first = self.last_run("first_seen", site).is_none();
                let since = if first { 0 } else { now_ms - 2 * DAY_MS };
                let r = self.runner.first_seen(site, since).await;
                self.finish("first_seen", site, now_ms, r, &mut rep);
            }
            if self.due("reconcile", site, now_ms, self.reconcile_ms) {
                let to = now_ms.div_euclid(HOUR_MS) * HOUR_MS;
                let from = to - HOUR_MS;
                let total = self.metrics.label_sum("shed_total");
                let shed = match self.last_shed.lock() {
                    Ok(mut l) => {
                        let d = total.saturating_sub(*l);
                        *l = total;
                        d as i64
                    }
                    Err(_) => 0,
                };
                match self.runner.reconcile(site, from, to, shed).await {
                    Ok(r) => {
                        let ppm = |x: f64| (x * 1e6).round() as u64;
                        self.metrics
                            .set_label("site_browser_loss_ppm", site, ppm(r.loss));
                        self.metrics.set_label(
                            "site_duplicate_rate_ppm",
                            site,
                            ppm(r.duplicate_rate),
                        );
                        self.metrics.loss_estimate_ppm.store(ppm(r.loss), Relaxed);
                        self.metrics
                            .duplicate_rate_ppm
                            .store(ppm(r.duplicate_rate), Relaxed);
                        self.finish("reconcile", site, now_ms, Ok(()), &mut rep);
                    }
                    Err(e) => self.finish("reconcile", site, now_ms, Err(e), &mut rep),
                }
            }
        }
        rep
    }

    fn finish(
        &self,
        job: &'static str,
        site: &str,
        now: i64,
        r: anyhow::Result<()>,
        rep: &mut TickReport,
    ) {
        // A failed run is retried on the next interval, not in a tight loop.
        self.mark(job, site, now);
        self.metrics.inc_label("job_runs_total", job, 1);
        match r {
            Ok(()) => rep.ran.push(format!("{job}:{site}")),
            Err(e) => {
                tracing::warn!("job {job} for {site} failed: {e}");
                self.metrics.inc_label("job_failures_total", job, 1);
                rep.failed.push(format!("{job}:{site}"));
            }
        }
    }
}

// ---- run-once (CLI) ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobName {
    FirstSeen,
    Reconcile,
}

impl JobName {
    pub fn parse(s: &str) -> anyhow::Result<JobName> {
        match s {
            "first-seen" | "first_seen" => Ok(JobName::FirstSeen),
            "reconcile" => Ok(JobName::Reconcile),
            other => anyhow::bail!("unknown job `{other}` (first-seen | reconcile)"),
        }
    }

    pub fn sql(self) -> String {
        match self {
            JobName::FirstSeen => first_seen_sql().0,
            JobName::Reconcile => reconcile::sql().to_string(),
        }
    }
}

/// Run one job now for `site` and describe the result as JSON.
pub async fn run_once(
    runner: &dyn JobRunner,
    job: JobName,
    site: &str,
    now_ms: i64,
) -> anyhow::Result<serde_json::Value> {
    match job {
        JobName::FirstSeen => {
            runner.first_seen(site, 0).await?;
            Ok(serde_json::json!({"job": "first-seen", "site": site, "ok": true}))
        }
        JobName::Reconcile => {
            let to = now_ms.div_euclid(HOUR_MS) * HOUR_MS;
            let r = runner.reconcile(site, to - HOUR_MS, to, 0).await?;
            Ok(serde_json::json!({
                "job": "reconcile", "site": site, "from": to - HOUR_MS, "to": to,
                "expected": r.expected, "received": r.received, "missing": r.missing,
                "loss": r.loss, "duplicate_rate": r.duplicate_rate
            }))
        }
    }
}
