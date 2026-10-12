//! Wire and storage types shared by the gateway, spool, sink and query layers.

use serde::{Deserialize, Serialize};

/// Built-in event names (PRD §3). The gateway drops every other `$` name.
pub const BUILTIN_EVENTS: &[&str] = &[
    "$session_start",
    "$pageview",
    "$pageleave",
    "$click",
    "$identify",
    "$form_submit",
    "$rage_click",
    "$dead_click",
    "$scroll",
    "$web_vital",
    "$error",
];

/// Spool priority class (PRD §7). P0 survives longest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Priority {
    P0,
    P1,
    P2,
}

impl Priority {
    pub fn of(event_name: &str) -> Priority {
        match event_name {
            "$rage_click" | "$dead_click" | "$error" => Priority::P1,
            "$scroll" | "$web_vital" | "$pageleave" => Priority::P2,
            _ => Priority::P0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Priority::P0 => "p0",
            Priority::P1 => "p1",
            Priority::P2 => "p2",
        }
    }
}

/// One row of the `events` table, in column order of the DDL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StoredEvent {
    /// Epoch milliseconds (corrected).
    pub ts: i64,
    pub site_id: String,
    pub event_name: String,
    pub visitor_id: String,
    pub session_id: String,
    pub user_id: String,
    pub is_identified: bool,
    pub is_new_visitor: bool,
    pub seq: i32,
    pub is_late: bool,
    pub path: String,
    pub page_group: String,
    pub parent_path: String,
    pub referrer_host: String,
    pub el_tag: String,
    pub el_id: String,
    pub el_text: String,
    pub el_css: String,
    pub el_attrs: String,
    pub props: String,
    pub ctx: String,
    pub env: String,
    pub release: String,
    pub sample_rate: f64,
    pub device: String,
    pub browser: String,
    pub os: String,
    pub country: String,
}

/// One accepted batch as written to the spool: one frame per batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpoolBatch {
    pub site_id: String,
    /// Gateway receive time, epoch ms.
    pub received_at: i64,
    pub events: Vec<StoredEvent>,
}
