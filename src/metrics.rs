//! Prometheus text metrics and per-site daily usage counters (PRD §7, §8 `/usage`).

use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
};

#[derive(Default)]
pub struct Metrics {
    pub accepted: AtomicU64,
    pub rejected: AtomicU64,
    pub duplicates_dropped: AtomicU64,
    pub late_events: AtomicU64,
    pub dead_frames: AtomicU64,
    pub spool_bytes: AtomicU64,
    pub spool_cap_bytes: AtomicU64,
    pub oldest_unwritten_ms: AtomicU64,
    pub drained_fresh: AtomicU64,
    pub drained_backfill: AtomicU64,
    pub insert_latency_us_sum: AtomicU64,
    pub insert_count: AtomicU64,
    pub loss_estimate_ppm: AtomicU64,
    pub duplicate_rate_ppm: AtomicU64,
    labeled: Mutex<BTreeMap<(&'static str, String), u64>>,
    usage: Mutex<BTreeMap<(String, String), UsageDay>>,
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UsageDay {
    pub accepted: u64,
    pub rejected: u64,
    pub dropped: u64,
}

impl Metrics {
    pub fn inc(&self, c: &AtomicU64, n: u64) {
        c.fetch_add(n, Relaxed);
    }

    /// Labelled counter such as `dropped{reason="bad_seq"}`.
    pub fn inc_label(&self, name: &'static str, label: &str, n: u64) {
        if let Ok(mut m) = self.labeled.lock() {
            *m.entry((name, label.to_string())).or_default() += n;
        }
    }

    /// Gauge-style labelled value (overwrites).
    pub fn set_label(&self, name: &'static str, label: &str, v: u64) {
        if let Ok(mut m) = self.labeled.lock() {
            m.insert((name, label.to_string()), v);
        }
    }

    /// Sum of a labelled counter over all labels.
    pub fn label_sum(&self, name: &'static str) -> u64 {
        self.labeled
            .lock()
            .map(|m| {
                m.iter()
                    .filter(|((n, _), _)| *n == name)
                    .map(|(_, v)| *v)
                    .sum()
            })
            .unwrap_or(0)
    }

    pub fn label_value(&self, name: &'static str, label: &str) -> u64 {
        self.labeled
            .lock()
            .ok()
            .and_then(|m| m.get(&(name, label.to_string())).copied())
            .unwrap_or(0)
    }

    pub fn usage_add(&self, site: &str, day: &str, f: impl FnOnce(&mut UsageDay)) {
        if let Ok(mut m) = self.usage.lock() {
            f(m.entry((site.to_string(), day.to_string())).or_default());
        }
    }

    pub fn usage_snapshot(&self) -> BTreeMap<(String, String), UsageDay> {
        self.usage.lock().map(|m| m.clone()).unwrap_or_default()
    }

    pub fn usage_restore(&self, rows: BTreeMap<(String, String), UsageDay>) {
        if let Ok(mut m) = self.usage.lock() {
            for (k, v) in rows {
                m.insert(k, v);
            }
        }
    }

    pub fn set_oldest_unwritten_ms(&self, v: u64) {
        self.oldest_unwritten_ms.store(v, Relaxed);
    }

    pub fn render(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let g = |a: &AtomicU64| a.load(Relaxed);
        let mut line = |name: &str, help: &str, kind: &str, v: u64| {
            let _ = writeln!(
                out,
                "# HELP greprum_{name} {help}\n# TYPE greprum_{name} {kind}\ngreprum_{name} {v}"
            );
        };
        line(
            "accepted_total",
            "Events acknowledged and spooled",
            "counter",
            g(&self.accepted),
        );
        line(
            "rejected_total",
            "Events or batches rejected",
            "counter",
            g(&self.rejected),
        );
        line(
            "duplicates_dropped_total",
            "Events dropped by the dedup cache",
            "counter",
            g(&self.duplicates_dropped),
        );
        line(
            "late_events_total",
            "Events marked is_late",
            "counter",
            g(&self.late_events),
        );
        line(
            "dead_frames_total",
            "Frames moved to dead/",
            "counter",
            g(&self.dead_frames),
        );
        line(
            "spool_bytes",
            "Bytes in the spool",
            "gauge",
            g(&self.spool_bytes),
        );
        line(
            "spool_cap_bytes",
            "Spool disk cap",
            "gauge",
            g(&self.spool_cap_bytes),
        );
        line(
            "oldest_unwritten_event_age_ms",
            "Age of the oldest event not yet written to GreptimeDB",
            "gauge",
            g(&self.oldest_unwritten_ms),
        );
        line(
            "drained_fresh_total",
            "Events written by the fresh lane",
            "counter",
            g(&self.drained_fresh),
        );
        line(
            "drained_backfill_total",
            "Events written by the backfill lane",
            "counter",
            g(&self.drained_backfill),
        );
        line(
            "insert_latency_us_sum",
            "Sum of insert latency in microseconds",
            "counter",
            g(&self.insert_latency_us_sum),
        );
        line(
            "insert_total",
            "Number of inserts",
            "counter",
            g(&self.insert_count),
        );
        line(
            "browser_loss_estimate_ppm",
            "Estimated browser side loss, parts per million",
            "gauge",
            g(&self.loss_estimate_ppm),
        );
        line(
            "duplicate_rate_ppm",
            "Duplicate rate from the reconcile job, ppm",
            "gauge",
            g(&self.duplicate_rate_ppm),
        );
        if let Ok(m) = self.labeled.lock() {
            let mut last = "";
            for ((name, label), v) in m.iter() {
                if *name != last {
                    let _ = writeln!(out, "# TYPE greprum_{name} counter");
                    last = name;
                }
                let key = match *name {
                    "dropped_total" => "reason",
                    "shed_total" => "class",
                    "rejected_by_total" => "reason",
                    "job_runs_total" | "job_failures_total" | "job_last_run_timestamp_seconds" => {
                        "job"
                    }
                    "site_browser_loss_ppm" | "site_duplicate_rate_ppm" => "site",
                    _ => "label",
                };
                let _ = writeln!(
                    out,
                    "greprum_{name}{{{key}=\"{}\"}} {v}",
                    label.replace('"', "")
                );
            }
        }
        if let Ok(m) = self.usage.lock() {
            let _ = writeln!(out, "# TYPE greprum_site_events_total counter");
            for ((site, day), u) in m.iter() {
                for (kind, v) in [
                    ("accepted", u.accepted),
                    ("rejected", u.rejected),
                    ("dropped", u.dropped),
                ] {
                    let _ = writeln!(
                        out,
                        "greprum_site_events_total{{site=\"{}\",day=\"{day}\",kind=\"{kind}\"}} {v}",
                        site.replace('"', "")
                    );
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_counters_labels_and_usage() {
        let m = Metrics::default();
        m.inc(&m.accepted, 3);
        m.inc_label("shed_total", "p2", 2);
        m.inc_label("dropped_total", "bad_seq", 1);
        m.usage_add("s1", "2026-10-12", |u| u.accepted += 3);
        let t = m.render();
        assert!(t.contains("greprum_accepted_total 3"));
        assert!(t.contains("greprum_shed_total{class=\"p2\"} 2"));
        assert!(t.contains("greprum_dropped_total{reason=\"bad_seq\"} 1"));
        assert!(t.contains("site=\"s1\",day=\"2026-10-12\",kind=\"accepted\"} 3"));
    }
}
