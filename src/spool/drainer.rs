//! Drainer: spool → `Sink`, fresh lane first, backfill lane second (PRD §7 steps 5–6).

use super::{Pos, Read_, Spool, list_segments, read_at, seg_path};
use crate::{
    config::IngestConfig,
    metrics::Metrics,
    model::{SpoolBatch, StoredEvent},
    sink::{Sink, SinkError},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs,
    path::Path,
    sync::{Arc, atomic::Ordering::Relaxed},
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Range {
    pub start: Pos,
    pub end: Pos,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Checkpoint {
    /// Everything before this (and outside `backfill`) is written.
    pub fresh: Pos,
    /// Older unwritten ranges, oldest first.
    pub backfill: Vec<Range>,
}

impl Checkpoint {
    pub fn load(dir: &Path) -> Option<Checkpoint> {
        let s = fs::read_to_string(dir.join("checkpoint.json")).ok()?;
        serde_json::from_str(&s).ok()
    }

    /// Temp file + rename so a crash never leaves a half-written checkpoint.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let tmp = dir.join("checkpoint.json.tmp");
        fs::write(&tmp, serde_json::to_vec(self).unwrap_or_default())?;
        fs::rename(tmp, dir.join("checkpoint.json"))
    }

    fn lowest(&self) -> Pos {
        self.backfill
            .first()
            .map(|r| r.start)
            .unwrap_or(self.fresh)
            .min(self.fresh)
    }
}

pub struct Drainer {
    spool: Arc<Spool>,
    sink: Arc<dyn Sink>,
    cfg: IngestConfig,
    pub cp: Checkpoint,
    metrics: Arc<Metrics>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// This many events were written.
    Wrote(usize),
    Idle,
    /// Sink said transient: caller should back off.
    Retry,
}

impl Drainer {
    pub fn new(
        spool: Arc<Spool>,
        sink: Arc<dyn Sink>,
        cfg: IngestConfig,
        metrics: Arc<Metrics>,
    ) -> Drainer {
        let first = list_segments(spool.dir()).first().copied().unwrap_or(1);
        let cp = Checkpoint::load(spool.dir()).unwrap_or(Checkpoint {
            fresh: Pos { seg: first, off: 0 },
            backfill: vec![],
        });
        Drainer {
            spool,
            sink,
            cfg,
            cp,
            metrics,
        }
    }

    pub fn checkpoint(&self) -> &Checkpoint {
        &self.cp
    }

    /// Collect whole frames from `from` until `end` (exclusive) or `max_events`.
    fn collect(
        &self,
        from: Pos,
        end: Option<Pos>,
        max_events: usize,
        dead: &mut u64,
    ) -> (Vec<SpoolBatch>, Pos) {
        let mut pos = from;
        let mut batches = vec![];
        let mut n = 0;
        while n < max_events {
            if end.is_some_and(|e| pos >= e) {
                break;
            }
            match read_at(self.spool.dir(), pos) {
                Ok(Read_::Batch(b, next)) => {
                    if n > 0 && n + b.events.len() > max_events {
                        break;
                    }
                    n += b.events.len();
                    batches.push(*b);
                    pos = next;
                }
                Ok(Read_::Corrupt(next)) => {
                    *dead += 1;
                    pos = next;
                }
                _ => break,
            }
        }
        (batches, pos)
    }

    async fn write(&self, batches: &[SpoolBatch], lane_fresh: bool) -> Result<usize, SinkError> {
        let events: Vec<StoredEvent> = batches
            .iter()
            .flat_map(|b| b.events.iter().cloned())
            .collect();
        if events.is_empty() {
            return Ok(0);
        }
        let t = std::time::Instant::now();
        match self.sink.write(&events).await {
            Ok(()) => {}
            Err(SinkError::Permanent(_)) if batches.len() > 1 => {
                // Isolate the poisoned frame: write frame by frame.
                for b in batches {
                    match self.sink.write(&b.events).await {
                        Ok(()) => {}
                        Err(SinkError::Permanent(m)) => self.to_dead(b, &m),
                        Err(e) => return Err(e),
                    }
                }
            }
            Err(SinkError::Permanent(m)) => self.to_dead(&batches[0], &m),
            Err(e) => return Err(e),
        }
        self.metrics
            .insert_latency_us_sum
            .fetch_add(t.elapsed().as_micros() as u64, Relaxed);
        self.metrics.insert_count.fetch_add(1, Relaxed);
        let lane = if lane_fresh {
            &self.metrics.drained_fresh
        } else {
            &self.metrics.drained_backfill
        };
        lane.fetch_add(events.len() as u64, Relaxed);
        Ok(events.len())
    }

    fn to_dead(&self, b: &SpoolBatch, reason: &str) {
        let dir = self.spool.dir().join("dead");
        let name = format!(
            "{}-{}.json",
            b.received_at,
            self.metrics.dead_frames.load(Relaxed)
        );
        let body = serde_json::json!({"reason": reason, "batch": b});
        let _ = fs::write(dir.join(name), body.to_string());
        self.metrics.dead_frames.fetch_add(1, Relaxed);
        self.metrics
            .usage_add(&b.site_id, &day_of(b.received_at), |u| {
                u.dropped += b.events.len() as u64
            });
    }

    fn persist(&mut self) {
        let _ = self.cp.save(self.spool.dir());
        // Delete segments strictly behind everything still unwritten.
        let low = self.cp.lowest().seg;
        let tail_seg = self.spool.tail().seg;
        for s in list_segments(self.spool.dir()) {
            if s < low && s < tail_seg {
                let p = seg_path(self.spool.dir(), s);
                let len = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                if fs::remove_file(&p).is_ok() {
                    self.spool.add_bytes_removed(len);
                }
            }
        }
    }

    /// One fresh-lane step.
    pub async fn step_fresh(&mut self) -> Step {
        let mut dead = 0;
        let (batches, next) = self.collect(self.cp.fresh, None, self.cfg.fresh_batch, &mut dead);
        if batches.is_empty() && next == self.cp.fresh {
            return Step::Idle;
        }
        match self.write(&batches, true).await {
            Ok(n) => {
                self.metrics.dead_frames.fetch_add(dead, Relaxed);
                self.cp.fresh = next;
                self.persist();
                if n == 0 && batches.is_empty() {
                    Step::Idle
                } else {
                    Step::Wrote(n)
                }
            }
            Err(_) => Step::Retry,
        }
    }

    /// One backfill-lane step (older ranges, larger batches).
    pub async fn step_backfill(&mut self) -> Step {
        let Some(r) = self.cp.backfill.first().cloned() else {
            return Step::Idle;
        };
        let mut dead = 0;
        let (batches, next) =
            self.collect(r.start, Some(r.end), self.cfg.backfill_batch, &mut dead);
        if batches.is_empty() && next == r.start {
            self.cp.backfill.remove(0);
            self.persist();
            return Step::Idle;
        }
        match self.write(&batches, false).await {
            Ok(n) => {
                self.metrics.dead_frames.fetch_add(dead, Relaxed);
                if next >= r.end {
                    self.cp.backfill.remove(0);
                } else {
                    self.cp.backfill[0].start = next;
                }
                self.persist();
                Step::Wrote(n)
            }
            Err(_) => Step::Retry,
        }
    }

    /// Move the pending fresh range to the backfill lane and jump fresh to the tail,
    /// so new events are written first once the database is back.
    pub fn demote_fresh(&mut self) {
        let tail = self.spool.tail();
        if tail > self.cp.fresh {
            self.cp.backfill.push(Range {
                start: self.cp.fresh,
                end: tail,
            });
            self.cp.fresh = tail;
            self.persist();
        }
    }

    /// When the fresh lane has fallen further behind than `fresh_lag_secs`, hand its backlog to
    /// the backfill lane (bigger inserts) and jump to the tail. Returns whether it demoted.
    pub fn maybe_demote_lagging(&mut self, now_ms: i64) -> bool {
        let mut dead = 0;
        let (head, _) = self.collect(self.cp.fresh, None, 1, &mut dead);
        let lag = head.first().map(|b| now_ms - b.received_at).unwrap_or(0);
        if lag > self.cfg.fresh_lag_secs as i64 * 1000 && self.spool.tail() > self.cp.fresh {
            self.demote_fresh();
            return true;
        }
        false
    }

    /// Age of the oldest unwritten event in ms (0 when drained). Also sets the metric.
    pub fn oldest_unwritten_age_ms(&self, now_ms: i64) -> u64 {
        let mut dead = 0;
        let (b, _) = self.collect(self.cp.lowest(), None, 1, &mut dead);
        let age = b
            .first()
            .map(|b| (now_ms - b.received_at).max(0) as u64)
            .unwrap_or(0);
        self.metrics.set_oldest_unwritten_ms(age);
        age
    }

    /// Run until `stop` flips. Backoff with jitter on transient failures.
    pub async fn run(mut self, stop: Arc<std::sync::atomic::AtomicBool>) {
        use rand::Rng;
        let mut backoff = Duration::from_millis(100);
        let mut failing_since: Option<std::time::Instant> = None;
        let interval = Duration::from_millis(self.cfg.fresh_interval_ms.max(10));
        let mut backfill: VecDeque<()> = VecDeque::new();
        backfill.clear();
        while !stop.load(Relaxed) {
            let fresh = self.step_fresh().await;
            let mut busy = matches!(fresh, Step::Wrote(_));
            let mut retry = fresh == Step::Retry;
            if fresh == Step::Idle || fresh == Step::Retry {
                // Backfill runs only when the fresh lane has nothing to do (lower priority).
                if !retry {
                    match self.step_backfill().await {
                        Step::Wrote(_) => busy = true,
                        Step::Retry => retry = true,
                        Step::Idle => {}
                    }
                }
            }
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            self.oldest_unwritten_age_ms(now_ms);
            if !retry && self.maybe_demote_lagging(now_ms) {
                busy = true;
            }
            if retry {
                let since = *failing_since.get_or_insert_with(std::time::Instant::now);
                if since.elapsed() >= Duration::from_secs(self.cfg.fresh_stall_secs) {
                    self.demote_fresh();
                    failing_since = Some(std::time::Instant::now());
                }
                let jitter = rand::thread_rng().gen_range(0..=backoff.as_millis() as u64 / 2 + 1);
                tokio::time::sleep(backoff + Duration::from_millis(jitter)).await;
                backoff = (backoff * 2).min(Duration::from_secs(30));
            } else {
                failing_since = None;
                backoff = Duration::from_millis(100);
                if !busy {
                    tokio::time::sleep(interval).await;
                }
            }
        }
    }
}

pub fn day_of(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}
