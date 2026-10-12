#![allow(dead_code)]
//! Live GreptimeDB helpers. Tests using these are `#[ignore]`d; run with `make test-db`.
use greprum::{
    config::{Config, SiteConfig},
    migrate,
    query::{self, Reader, SqlxReader},
    sink::{Sink, SqlxSink},
};
use sqlx::Executor;
use std::{sync::Arc, time::Duration};

pub fn base_url() -> String {
    std::env::var("GREPRUM_TEST_DB")
        .expect("set GREPRUM_TEST_DB=mysql://root@127.0.0.1:4002/public (make test-db)")
}

pub struct TestDb {
    pub url: String,
    pub name: String,
    pub prefix: String,
    pub reader: SqlxReader,
    pub sink: SqlxSink,
}

static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A fresh database with the full schema applied.
pub async fn fresh() -> TestDb {
    fresh_with_ttl("13 months").await
}

pub async fn fresh_with_ttl(ttl: &str) -> TestDb {
    let base = base_url();
    let admin = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(1)
        .connect(&base)
        .await
        .expect("connect admin");
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let name = format!("t{}_{}", std::process::id(), n);
    admin
        .execute(sqlx::raw_sql(&format!(
            "CREATE DATABASE IF NOT EXISTS {name}"
        )))
        .await
        .expect("create db");
    let url = format!("{}/{}", base.rsplit_once('/').unwrap().0, name);
    let prefix = format!("{name}_");
    migrate::apply(&url, ttl, &prefix).await.expect("migrate");
    TestDb {
        prefix,
        reader: SqlxReader::connect(&url).await.unwrap(),
        sink: SqlxSink::connect(&url).await.unwrap(),
        url,
        name,
    }
}

impl TestDb {
    pub async fn exec(&self, sql: &str) {
        self.reader
            .pool()
            .execute(sqlx::raw_sql(sql))
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    pub async fn rows(&self, sql: &str, binds: Vec<query::Bind>) -> Vec<serde_json::Value> {
        self.reader
            .fetch(&query::Q {
                name: "t",
                sql: sql.into(),
                binds,
            })
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
    }

    pub async fn reader_fetch(&self, sql: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        self.reader
            .fetch(&query::Q {
                name: "t",
                sql: sql.into(),
                binds: vec![],
            })
            .await
    }

    pub async fn count(&self, sql: &str) -> i64 {
        self.rows(sql, vec![]).await[0]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .as_i64()
            .unwrap()
    }

    pub async fn write(&self, events: &[greprum::model::StoredEvent]) {
        self.sink.write(events).await.expect("sink write");
    }

    /// Flush every flow, then wait until `sql` returns a non-zero first column or time runs out.
    pub async fn flush_flows(&self) {
        for f in migrate::flows_prefixed(&self.prefix) {
            let name = format!("{}{}", self.prefix, f.name);
            let _ = self
                .reader
                .pool()
                .execute(sqlx::raw_sql(&format!("ADMIN FLUSH_FLOW('{name}')")))
                .await;
        }
    }

    pub async fn wait_count(&self, sql: &str, want: i64) -> i64 {
        let mut last = -1;
        for _ in 0..40 {
            self.flush_flows().await;
            last = self.count(sql).await;
            if last >= want {
                return last;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        last
    }
}

pub fn site_cfg(id: &str) -> SiteConfig {
    let mut s = super::site();
    s.id = id.into();
    s.key = format!("pk_{id}");
    s
}

pub fn app_config(site_ids: &[&str]) -> Config {
    use greprum::config::ApiKeyConfig;
    Config {
        sites: site_ids.iter().map(|i| site_cfg(i)).collect(),
        api_keys: vec![ApiKeyConfig {
            name: "all".into(),
            sha256: query::hash_key("k"),
            sites: site_ids.iter().map(|s| s.to_string()).collect(),
        }],
        ..Config::default()
    }
}

pub fn arc<T>(t: T) -> Arc<T> {
    Arc::new(t)
}

pub mod api {
    use super::*;
    use axum::{body::Body, http::Request};
    use greprum::{
        metrics::Metrics,
        server::{self, AppState},
        spool::Spool,
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    pub struct Api {
        pub router: axum::Router,
        _dir: tempfile::TempDir,
    }

    pub fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    pub async fn api(db: &TestDb, sites: &[&str]) -> Api {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = app_config(sites);
        cfg.spool.dir = dir.path().to_string_lossy().into_owned();
        let cfg = Arc::new(cfg);
        let m = Arc::new(Metrics::default());
        let spool = Spool::open(dir.path(), cfg.spool.clone(), m.clone()).unwrap();
        let reader: Arc<dyn Reader> = Arc::new(SqlxReader::connect(&db.url).await.unwrap());
        let state = AppState::new(cfg, spool, m, Some(reader), server::system_clock());
        Api {
            router: server::router(state),
            _dir: dir,
        }
    }

    impl Api {
        pub async fn get(&self, uri: &str) -> (u16, serde_json::Value) {
            let r = Request::get(uri)
                .header("authorization", "Bearer k")
                .body(Body::empty())
                .unwrap();
            let resp = self.router.clone().oneshot(r).await.unwrap();
            let st = resp.status().as_u16();
            let b = resp.into_body().collect().await.unwrap().to_bytes();
            (
                st,
                serde_json::from_slice(&b).unwrap_or(serde_json::Value::Null),
            )
        }
    }
}
