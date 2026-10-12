//! `SqlxReader`: runs a [`Q`] over the MySQL protocol and returns JSON rows.

use super::{Bind, Q, Reader};
use async_trait::async_trait;
use serde_json::{Map, Value, json};
use sqlx::{Column, Row, TypeInfo, mysql::MySqlPoolOptions};

pub struct SqlxReader {
    pool: sqlx::MySqlPool,
}

impl SqlxReader {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        Ok(Self {
            pool: MySqlPoolOptions::new()
                .max_connections(8)
                .connect(url)
                .await?,
        })
    }

    pub fn lazy(url: &str) -> anyhow::Result<Self> {
        Ok(Self {
            pool: MySqlPoolOptions::new()
                .max_connections(8)
                .acquire_timeout(std::time::Duration::from_secs(5))
                .connect_lazy(url)?,
        })
    }

    pub fn pool(&self) -> &sqlx::MySqlPool {
        &self.pool
    }
}

fn cell(row: &sqlx::mysql::MySqlRow, i: usize) -> Value {
    let ty = row.column(i).type_info().name().to_ascii_uppercase();
    macro_rules! get {
        ($t:ty) => {
            row.try_get::<Option<$t>, _>(i)
                .ok()
                .flatten()
                .map(|v| json!(v))
                .unwrap_or(Value::Null)
        };
    }
    match ty.as_str() {
        "BOOLEAN" => get!(bool),
        "TINYINT" | "SMALLINT" | "INT" | "BIGINT" => get!(i64),
        "TINYINT UNSIGNED" | "SMALLINT UNSIGNED" | "INT UNSIGNED" | "BIGINT UNSIGNED" => get!(u64),
        "FLOAT" | "DOUBLE" => get!(f64),
        "DECIMAL" => row
            .try_get::<Option<sqlx::types::BigDecimal>, _>(i)
            .ok()
            .flatten()
            .and_then(|d| d.to_string().parse::<f64>().ok())
            .map(|v| json!(v))
            .unwrap_or(Value::Null),
        "TIMESTAMP" | "DATETIME" => row
            .try_get::<Option<chrono::NaiveDateTime>, _>(i)
            .ok()
            .flatten()
            .map(|d| json!(d.and_utc().timestamp_millis()))
            .unwrap_or(Value::Null),
        _ => get!(String),
    }
}

#[async_trait]
impl Reader for SqlxReader {
    async fn fetch(&self, q: &Q) -> anyhow::Result<Vec<Value>> {
        let mut query = sqlx::query(&q.sql);
        for b in &q.binds {
            query = match b {
                Bind::Str(s) => query.bind(s.clone()),
                Bind::Int(i) => query.bind(*i),
                Bind::Float(f) => query.bind(*f),
            };
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(|r| {
                let mut m = Map::new();
                for (i, c) in r.columns().iter().enumerate() {
                    m.insert(c.name().to_string(), cell(r, i));
                }
                Value::Object(m)
            })
            .collect())
    }
}
