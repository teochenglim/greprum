//! `Sink` trait (ADR 0018): the only place events leave the gateway for the database.

use crate::model::StoredEvent;
use async_trait::async_trait;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

pub mod sqlx_sink;
pub use sqlx_sink::SqlxSink;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum SinkError {
    /// Retry later: connection lost, timeout, database busy.
    #[error("transient: {0}")]
    Transient(String),
    /// The database refuses this data for good: move the frame to `dead/`.
    #[error("permanent: {0}")]
    Permanent(String),
}

#[async_trait]
pub trait Sink: Send + Sync {
    async fn write(&self, events: &[StoredEvent]) -> Result<(), SinkError>;
}

/// In-memory sink with failure injection, used by component and fault tests.
#[derive(Default)]
pub struct MemorySink {
    pub rows: Mutex<Vec<StoredEvent>>,
    /// Number of upcoming writes to fail transiently.
    pub fail_transient: AtomicUsize,
    /// Fail permanently whenever a batch contains this `session_id`.
    pub poison_session: Mutex<Option<String>>,
    /// When true, every write fails transiently (simulated outage).
    pub outage: std::sync::atomic::AtomicBool,
    pub calls: AtomicUsize,
    pub max_batch: AtomicUsize,
}

impl MemorySink {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn len(&self) -> usize {
        self.rows.lock().map(|r| r.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn snapshot(&self) -> Vec<StoredEvent> {
        self.rows.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

#[async_trait]
impl Sink for MemorySink {
    async fn write(&self, events: &[StoredEvent]) -> Result<(), SinkError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.max_batch.fetch_max(events.len(), Ordering::SeqCst);
        if self.outage.load(Ordering::SeqCst) {
            return Err(SinkError::Transient("outage".into()));
        }
        if self
            .fail_transient
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(SinkError::Transient("injected".into()));
        }
        if let Ok(p) = self.poison_session.lock()
            && let Some(s) = p.as_ref()
            && events.iter().any(|e| &e.session_id == s)
        {
            return Err(SinkError::Permanent("poisoned".into()));
        }
        if let Ok(mut r) = self.rows.lock() {
            r.extend_from_slice(events);
        }
        Ok(())
    }
}
