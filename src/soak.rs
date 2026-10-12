//! Success-metric maths (PRD §10): 99.9 % of accepted events reach GreptimeDB within 5 s.

pub const SLO_MS: u64 = 5_000;
pub const SLO_FRACTION: f64 = 0.999;

/// Fraction of latencies at or under `slo_ms`. Empty input is 1.0 (nothing was late).
pub fn within(latencies_ms: &[u64], slo_ms: u64) -> f64 {
    if latencies_ms.is_empty() {
        return 1.0;
    }
    latencies_ms.iter().filter(|l| **l <= slo_ms).count() as f64 / latencies_ms.len() as f64
}

#[derive(Debug, PartialEq)]
pub struct Verdict {
    pub fraction: f64,
    pub pass: bool,
}

pub fn verdict(latencies_ms: &[u64]) -> Verdict {
    let fraction = within(latencies_ms, SLO_MS);
    Verdict {
        fraction,
        pass: fraction >= SLO_FRACTION,
    }
}
