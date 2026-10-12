#![allow(dead_code)]
pub mod db;
pub mod ops;
pub mod spool;
use greprum::config::{IngestConfig, SiteConfig};
use serde_json::{Value, json};

pub fn site() -> SiteConfig {
    SiteConfig {
        id: "s1".into(),
        key: "pk_test".into(),
        origins: vec!["https://example.com".into()],
        hmac_secret: "secret".into(),
        timezone: "UTC".into(),
        drop_attrs: vec![],
        store_text: true,
        honour_dnt: false,
        ..Default::default()
    }
}

pub fn cfg() -> IngestConfig {
    IngestConfig::default()
}

pub const NOW: i64 = 1_760_236_800_000;
pub const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/17.0 Safari/605.1.15";

pub fn ev(name: &str, seq: i64) -> Value {
    json!({"ts": NOW, "seq": seq, "name": name, "path": "/pricing"})
}

pub fn batch(events: Vec<Value>) -> Value {
    json!({"visitor_id": "v_1", "session_id": "s_1", "sent_at": NOW, "events": events})
}

pub fn req() -> greprum::collect::RequestInfo {
    greprum::collect::RequestInfo {
        user_agent: UA.into(),
        country: "sg".into(),
        dnt: false,
    }
}

pub fn run(b: &Value) -> Result<greprum::collect::Processed, greprum::collect::BatchError> {
    greprum::collect::process(b.to_string().as_bytes(), &site(), &req(), &cfg(), NOW)
}

pub mod app {
    use super::*;
    use axum::{Router, body::Body, http::Request};
    use greprum::{
        config::Config,
        metrics::Metrics,
        server::{self, AppState},
        spool::Spool,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    };
    use tower::ServiceExt;

    pub struct Harness {
        pub router: Router,
        pub state: Arc<AppState>,
        pub dir: tempfile::TempDir,
        pub now: Arc<AtomicI64>,
    }

    pub fn harness_with(mutate: impl FnOnce(&mut Config)) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.spool.dir = dir.path().to_string_lossy().into_owned();
        cfg.spool.fsync_ms = 5;
        cfg.sites.push(site());
        mutate(&mut cfg);
        let cfg = Arc::new(cfg);
        let m = Arc::new(Metrics::default());
        let spool = Spool::open(dir.path(), cfg.spool.clone(), m.clone()).unwrap();
        let now = Arc::new(AtomicI64::new(NOW));
        let n2 = now.clone();
        let clock: server::Clock = Arc::new(move || n2.load(Ordering::SeqCst));
        let state = AppState::new(cfg, spool, m, None, clock);
        Harness {
            router: server::router(state.clone()),
            state,
            dir,
            now,
        }
    }

    pub fn harness() -> Harness {
        harness_with(|_| {})
    }

    impl Harness {
        pub async fn send(&self, req: Request<Body>) -> axum::response::Response {
            self.router.clone().oneshot(req).await.unwrap()
        }

        pub async fn post(
            &self,
            uri: &str,
            origin: Option<&str>,
            body: String,
        ) -> axum::response::Response {
            let mut r = Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "text/plain")
                .header("user-agent", UA);
            if let Some(o) = origin {
                r = r.header("origin", o);
            }
            self.send(r.body(Body::from(body)).unwrap()).await
        }

        pub async fn collect(&self, b: &serde_json::Value) -> axum::http::StatusCode {
            self.post(
                "/collect?k=pk_test",
                Some("https://example.com"),
                b.to_string(),
            )
            .await
            .status()
        }
    }
}

pub mod qapp {
    use super::*;
    use async_trait::async_trait;
    use axum::{body::Body, http::Request};
    use greprum::{
        config::{ApiKeyConfig, Config},
        metrics::Metrics,
        query::{Q, Reader, hash_key},
        server::{self, AppState},
        spool::Spool,
    };
    use http_body_util::BodyExt;
    use serde_json::Value;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    #[derive(Default)]
    pub struct FakeReader {
        pub calls: Mutex<Vec<Q>>,
        pub rows: Mutex<std::collections::HashMap<String, Vec<Value>>>,
    }

    impl FakeReader {
        pub fn with(rows: &[(&str, Vec<Value>)]) -> Arc<Self> {
            let r = FakeReader::default();
            for (k, v) in rows {
                r.rows.lock().unwrap().insert(k.to_string(), v.clone());
            }
            Arc::new(r)
        }
        pub fn calls(&self) -> Vec<Q> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Reader for FakeReader {
        async fn fetch(&self, q: &Q) -> anyhow::Result<Vec<Value>> {
            self.calls.lock().unwrap().push(q.clone());
            Ok(self
                .rows
                .lock()
                .unwrap()
                .get(q.name)
                .cloned()
                .unwrap_or_default())
        }
    }

    pub struct QHarness {
        pub router: axum::Router,
        pub reader: Arc<FakeReader>,
        pub state: Arc<AppState>,
        _dir: tempfile::TempDir,
    }

    pub fn site_b() -> greprum::config::SiteConfig {
        let mut s = site();
        s.id = "s2".into();
        s.key = "pk_b".into();
        s
    }

    pub fn qharness(reader: Arc<FakeReader>) -> QHarness {
        qharness_with(reader, |_| {})
    }

    pub fn qharness_with(reader: Arc<FakeReader>, f: impl FnOnce(&mut Config)) -> QHarness {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.spool.dir = dir.path().to_string_lossy().into_owned();
        let mut a = site();
        a.timezone = "Asia/Singapore".into();
        cfg.sites = vec![a, site_b()];
        cfg.api_keys = vec![
            ApiKeyConfig {
                name: "a".into(),
                sha256: hash_key("key-a"),
                sites: vec!["s1".into()],
            },
            ApiKeyConfig {
                name: "b".into(),
                sha256: hash_key("key-b"),
                sites: vec!["s2".into()],
            },
            ApiKeyConfig {
                name: "ab".into(),
                sha256: hash_key("key-ab"),
                sites: vec!["s1".into(), "s2".into()],
            },
        ];
        f(&mut cfg);
        let cfg = Arc::new(cfg);
        let m = Arc::new(Metrics::default());
        let spool = Spool::open(dir.path(), cfg.spool.clone(), m.clone()).unwrap();
        let r: Arc<dyn Reader> = reader.clone();
        let clock: server::Clock = Arc::new(|| NOW);
        let state = AppState::new(cfg, spool, m, Some(r), clock);
        QHarness {
            router: server::router(state.clone()),
            reader,
            state,
            _dir: dir,
        }
    }

    impl QHarness {
        pub async fn get(&self, uri: &str, key: Option<&str>) -> (u16, Value) {
            let mut r = Request::get(uri);
            if let Some(k) = key {
                r = r.header("authorization", format!("Bearer {k}"));
            }
            let resp = self
                .router
                .clone()
                .oneshot(r.body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = resp.status().as_u16();
            let b = resp.into_body().collect().await.unwrap().to_bytes();
            (status, serde_json::from_slice(&b).unwrap_or(Value::Null))
        }
    }
}
