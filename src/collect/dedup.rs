//! Bounded in-memory cache of recent `(site, session_id, seq)` (PRD §7, ADR 0008).

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

pub struct Dedup {
    ttl: Duration,
    max: usize,
    seen: HashMap<String, Instant>,
    order: VecDeque<(Instant, String)>,
}

impl Dedup {
    pub fn new(ttl: Duration, max: usize) -> Self {
        Self {
            ttl,
            max,
            seen: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn key(site: &str, session: &str, seq: i32) -> String {
        format!("{site}\u{0}{session}\u{0}{seq}")
    }

    fn expire(&mut self, now: Instant) {
        while let Some((t, _)) = self.order.front() {
            if now.duration_since(*t) < self.ttl && self.order.len() <= self.max {
                break;
            }
            if let Some((t, k)) = self.order.pop_front() {
                // Only remove if the map entry is the one this queue slot created.
                if self.seen.get(&k) == Some(&t) {
                    self.seen.remove(&k);
                }
            }
        }
    }

    /// True if this event was already seen within the window.
    pub fn contains(&mut self, site: &str, session: &str, seq: i32, now: Instant) -> bool {
        self.expire(now);
        self.seen.contains_key(&Self::key(site, session, seq))
    }

    pub fn insert(&mut self, site: &str, session: &str, seq: i32, now: Instant) {
        let k = Self::key(site, session, seq);
        self.seen.insert(k.clone(), now);
        self.order.push_back((now, k));
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_then_expire() {
        let t0 = Instant::now();
        let mut d = Dedup::new(Duration::from_secs(600), 100);
        assert!(!d.contains("s", "a", 1, t0));
        d.insert("s", "a", 1, t0);
        assert!(d.contains("s", "a", 1, t0 + Duration::from_secs(10)));
        assert!(!d.contains("s", "a", 2, t0));
        assert!(!d.contains("other", "a", 1, t0));
        assert!(!d.contains("s", "a", 1, t0 + Duration::from_secs(601)));
        assert!(d.is_empty());
    }

    #[test]
    fn bounded_by_max_entries() {
        let t0 = Instant::now();
        let mut d = Dedup::new(Duration::from_secs(600), 3);
        for i in 0..10 {
            d.insert("s", "a", i, t0);
        }
        d.contains("s", "a", 99, t0);
        assert!(d.len() <= 3);
    }
}
