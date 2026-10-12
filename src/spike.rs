//! Spike harness (PRD §10 phase 0): time the queries that decide the architecture and
//! compare p95 against the gates.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Gate {
    pub name: &'static str,
    pub p95_limit_ms: f64,
}

pub const GATES: &[Gate] = &[
    Gate {
        name: "rollup_overview",
        p95_limit_ms: 2_000.0,
    },
    Gate {
        name: "rollup_top_pages",
        p95_limit_ms: 2_000.0,
    },
    Gate {
        name: "rollup_journeys",
        p95_limit_ms: 2_000.0,
    },
    Gate {
        name: "funnel",
        p95_limit_ms: 10_000.0,
    },
    Gate {
        name: "retention",
        p95_limit_ms: 10_000.0,
    },
    Gate {
        name: "sessions_24h",
        p95_limit_ms: 10_000.0,
    },
    Gate {
        name: "count_distinct_raw",
        p95_limit_ms: 10_000.0,
    },
];

/// Nearest-rank percentile (`p` in 0..=100). Empty input yields 0.
pub fn percentile(samples: &[f64], p: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut s = samples.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = ((p / 100.0) * s.len() as f64).ceil().max(1.0) as usize;
    s[rank.min(s.len()) - 1]
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Verdict {
    pub name: String,
    pub p95_ms: f64,
    pub limit_ms: f64,
    pub pass: bool,
    /// When a raw-events query misses its gate the PRD adopts the summary-table jobs.
    pub adopt_summary_job: bool,
}

pub fn evaluate(name: &str, samples_ms: &[f64]) -> Option<Verdict> {
    let g = GATES.iter().find(|g| g.name == name)?;
    let p95 = percentile(samples_ms, 95.0);
    let pass = p95 <= g.p95_limit_ms;
    Some(Verdict {
        name: name.into(),
        p95_ms: p95,
        limit_ms: g.p95_limit_ms,
        pass,
        adopt_summary_job: !pass && !name.starts_with("rollup_"),
    })
}
