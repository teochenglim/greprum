//! `SqlxSink`: multi-row `INSERT` with bound parameters over GreptimeDB's MySQL protocol.

use super::{Sink, SinkError};
use crate::model::StoredEvent;
use async_trait::async_trait;
use sqlx::{MySql, MySqlPool, QueryBuilder, mysql::MySqlPoolOptions};

/// Column list in `StoredEvent` order. Reserved words are quoted (ADR 0028).
pub const INSERT_COLUMNS: &[&str] = &[
    "ts",
    "site_id",
    "event_name",
    "visitor_id",
    "session_id",
    "user_id",
    "is_identified",
    "is_new_visitor",
    "seq",
    "is_late",
    "`path`",
    "page_group",
    "parent_path",
    "referrer_host",
    "el_tag",
    "el_id",
    "el_text",
    "el_css",
    "el_attrs",
    "props",
    "ctx",
    "env",
    "`release`",
    "sample_rate",
    "device",
    "browser",
    "os",
    "country",
];

/// MySQL protocol allows 65 535 placeholders per statement.
pub const MAX_ROWS_PER_INSERT: usize = 65_535 / 28 - 1;

pub struct SqlxSink {
    pool: MySqlPool,
}

impl SqlxSink {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let pool = MySqlPoolOptions::new()
            .max_connections(4)
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    /// Does not connect now: the gateway must start (and keep acknowledging into the spool)
    /// while GreptimeDB is down. Failures surface per write as `SinkError::Transient`.
    pub fn lazy(url: &str) -> anyhow::Result<Self> {
        let pool = MySqlPoolOptions::new()
            .max_connections(4)
            .acquire_timeout(std::time::Duration::from_secs(5))
            .connect_lazy(url)?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: MySqlPool) -> Self {
        Self { pool }
    }
}

fn nz(s: &str) -> Option<&str> {
    if s.is_empty() { None } else { Some(s) }
}

pub fn build_insert<'a>(rows: &'a [StoredEvent]) -> QueryBuilder<'a, MySql> {
    let mut qb: QueryBuilder<MySql> = QueryBuilder::new(format!(
        "INSERT INTO events ({}) ",
        INSERT_COLUMNS.join(", ")
    ));
    qb.push_values(rows, |mut b, e| {
        b.push_bind(e.ts)
            .push_bind(&e.site_id)
            .push_bind(&e.event_name)
            .push_bind(&e.visitor_id)
            .push_bind(&e.session_id)
            .push_bind(nz(&e.user_id))
            .push_bind(e.is_identified)
            .push_bind(e.is_new_visitor)
            .push_bind(e.seq)
            .push_bind(e.is_late)
            .push_bind(&e.path)
            .push_bind(nz(&e.page_group))
            .push_bind(nz(&e.parent_path))
            .push_bind(nz(&e.referrer_host))
            .push_bind(nz(&e.el_tag))
            .push_bind(nz(&e.el_id))
            .push_bind(nz(&e.el_text))
            .push_bind(nz(&e.el_css))
            .push_bind(nz(&e.el_attrs))
            .push_bind(nz(&e.props))
            .push_bind(nz(&e.ctx))
            .push_bind(nz(&e.env))
            .push_bind(nz(&e.release))
            .push_bind(e.sample_rate)
            .push_bind(&e.device)
            .push_bind(&e.browser)
            .push_bind(&e.os)
            .push_bind(nz(&e.country));
    });
    qb
}

fn classify(e: sqlx::Error) -> SinkError {
    match &e {
        sqlx::Error::Database(db) => {
            // Data errors the database will never accept are permanent; others retry.
            let code = db.code().map(|c| c.to_string()).unwrap_or_default();
            let msg = db.message().to_string();
            if matches!(code.as_str(), "1265" | "1366" | "1292" | "1048" | "1406") {
                SinkError::Permanent(msg)
            } else {
                SinkError::Transient(msg)
            }
        }
        _ => SinkError::Transient(e.to_string()),
    }
}

#[async_trait]
impl Sink for SqlxSink {
    async fn write(&self, events: &[StoredEvent]) -> Result<(), SinkError> {
        for chunk in events.chunks(MAX_ROWS_PER_INSERT) {
            let mut qb = build_insert(chunk);
            qb.build().execute(&self.pool).await.map_err(classify)?;
        }
        Ok(())
    }
}
