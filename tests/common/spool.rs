#![allow(dead_code)]
use greprum::{
    config::{IngestConfig, SpoolConfig},
    metrics::Metrics,
    model::{SpoolBatch, StoredEvent},
    sink::MemorySink,
    spool::{Spool, drainer::Drainer},
};
use std::{path::Path, sync::Arc};

pub fn spool_cfg() -> SpoolConfig {
    SpoolConfig {
        fsync_ms: 5,
        ..SpoolConfig::default()
    }
}

pub fn event(session: &str, seq: i32) -> StoredEvent {
    StoredEvent {
        ts: 1_760_236_800_000 + seq as i64,
        site_id: "s1".into(),
        event_name: "$pageview".into(),
        visitor_id: "v".into(),
        session_id: session.into(),
        seq,
        path: "/".into(),
        sample_rate: 1.0,
        ..Default::default()
    }
}

pub fn batch(session: &str, from: i32, n: i32, received_at: i64) -> SpoolBatch {
    SpoolBatch {
        site_id: "s1".into(),
        received_at,
        events: (from..from + n).map(|s| event(session, s)).collect(),
    }
}

pub fn open(dir: &Path, cfg: greprum::config::SpoolConfig) -> (Arc<Spool>, Arc<Metrics>) {
    let m = Arc::new(Metrics::default());
    (Spool::open(dir, cfg, m.clone()).unwrap(), m)
}

pub fn drainer(
    spool: &Arc<Spool>,
    sink: &Arc<MemorySink>,
    m: &Arc<Metrics>,
    cfg: IngestConfig,
) -> Drainer {
    Drainer::new(spool.clone(), sink.clone(), cfg, m.clone())
}

/// Drain both lanes until idle (bounded, so a bug cannot hang the suite).
pub async fn drain_all(d: &mut Drainer) {
    use greprum::spool::drainer::Step;
    for _ in 0..10_000 {
        let f = d.step_fresh().await;
        if f == Step::Idle && d.step_backfill().await == Step::Idle {
            return;
        }
    }
    panic!("drain did not converge");
}
