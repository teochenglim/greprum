//! HTTP surface: tracker, ingest, health, metrics, query API (PRD §8).

use crate::{
    collect::{self, Admission, BatchError, RequestInfo, dedup::Dedup},
    config::{Config, SiteConfig},
    metrics::Metrics,
    model::{Priority, SpoolBatch},
    spool::{Spool, SpoolError, drainer::day_of},
};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{any, get},
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, atomic::Ordering::Relaxed},
    time::{Duration, Instant},
};

pub static TRACKER_JS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/t.js"));

pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    })
}

pub struct AppState {
    pub cfg: Arc<Config>,
    pub spool: Arc<Spool>,
    pub metrics: Arc<Metrics>,
    pub dedup: Mutex<Dedup>,
    rate: Mutex<HashMap<String, (i64, u32)>>,
    pub clock: Clock,
    pub reader: Option<Arc<dyn crate::query::Reader>>,
    pub tracker_hash: String,
}

impl AppState {
    pub fn new(
        cfg: Arc<Config>,
        spool: Arc<Spool>,
        metrics: Arc<Metrics>,
        reader: Option<Arc<dyn crate::query::Reader>>,
        clock: Clock,
    ) -> Arc<AppState> {
        let hash = hex::encode(&Sha256::digest(TRACKER_JS)[..8]);
        let dedup = Dedup::new(
            Duration::from_secs(cfg.ingest.dedup_secs),
            cfg.ingest.dedup_max_entries,
        );
        Arc::new(AppState {
            cfg,
            spool,
            metrics,
            dedup: Mutex::new(dedup),
            rate: Mutex::new(HashMap::new()),
            clock,
            reader,
            tracker_hash: hash,
        })
    }

    pub fn hashed_tracker_path(&self) -> String {
        format!("/t-{}.js", self.tracker_hash)
    }

    fn rate_ok(&self, key: &str, now_ms: i64) -> bool {
        let minute = now_ms / 60_000;
        let Ok(mut m) = self.rate.lock() else {
            return true;
        };
        let e = m.entry(key.to_string()).or_insert((minute, 0));
        if e.0 != minute {
            *e = (minute, 0);
        }
        e.1 += 1;
        e.1 <= self.cfg.server.rate_limit_per_min
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    let cfg = state.cfg.clone();
    let hashed = state.hashed_tracker_path();
    let mut r = Router::new()
        .route(&cfg.server.tracker_path, get(tracker))
        .route(&hashed, get(tracker_immutable))
        .route(&cfg.server.collect_path, any(collect_handler))
        .route("/healthz", get(|| async { "ok" }))
        .route("/metrics", get(metrics_handler));
    r = r.merge(crate::query::router(state.clone()));
    r.with_state(state)
}

async fn tracker(State(s): State<Arc<AppState>>) -> Response {
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    h.insert(
        header::ETAG,
        HeaderValue::from_str(&format!("\"{}\"", s.tracker_hash))
            .unwrap_or(HeaderValue::from_static("\"x\"")),
    );
    if let Ok(v) = HeaderValue::from_str(&s.hashed_tracker_path()) {
        h.insert("x-greprum-tracker-path", v);
    }
    (h, TRACKER_JS).into_response()
}

async fn tracker_immutable() -> Response {
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    (h, TRACKER_JS).into_response()
}

async fn metrics_handler(State(s): State<Arc<AppState>>) -> Response {
    s.metrics.spool_bytes.store(s.spool.bytes(), Relaxed);
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4"),
    );
    (h, s.metrics.render()).into_response()
}

fn origin_allowed(site: &SiteConfig, origin: Option<&str>) -> bool {
    match origin {
        Some(o) => site.origins.iter().any(|a| a == "*" || a == o),
        None => site.origins.iter().any(|a| a == "*"),
    }
}

fn cors(mut resp: Response, origin: Option<&str>, allowed: bool) -> Response {
    if let (true, Some(o)) = (allowed, origin)
        && let Ok(v) = HeaderValue::from_str(o)
    {
        resp.headers_mut()
            .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
        resp.headers_mut()
            .insert(header::VARY, HeaderValue::from_static("Origin"));
    }
    resp
}

fn reject(s: &AppState, reason: &'static str, status: StatusCode) -> Response {
    s.metrics.inc(&s.metrics.rejected, 1);
    s.metrics.inc_label("rejected_by_total", reason, 1);
    status.into_response()
}

async fn collect_handler(
    State(s): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
    req: Request<Body>,
) -> Response {
    let (parts, body) = req.into_parts();
    let origin = parts
        .headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let origin = origin.as_deref();

    // 1 gate: key → Origin → rate → size, all before the body is read.
    let Some(site) = q.get("k").and_then(|k| s.cfg.site_by_key(k)) else {
        return reject(&s, "unknown_key", StatusCode::UNAUTHORIZED);
    };
    let allowed = origin_allowed(site, origin);

    if parts.method == Method::OPTIONS {
        let mut r = StatusCode::NO_CONTENT.into_response();
        if allowed {
            let h = r.headers_mut();
            h.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("POST, OPTIONS"),
            );
            h.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("content-type"),
            );
            h.insert(
                header::ACCESS_CONTROL_MAX_AGE,
                HeaderValue::from_static("86400"),
            );
        }
        return cors(r, origin, allowed);
    }
    if parts.method != Method::POST {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    if !allowed {
        return reject(&s, "origin", StatusCode::FORBIDDEN);
    }
    let now = (s.clock)();
    if !s.rate_ok(&site.key, now) {
        return cors(
            reject(&s, "rate_limit", StatusCode::TOO_MANY_REQUESTS),
            origin,
            true,
        );
    }
    let max = s.cfg.ingest.max_body_bytes;
    if let Some(len) = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        && len > max
    {
        return cors(
            reject(&s, "too_large", StatusCode::PAYLOAD_TOO_LARGE),
            origin,
            true,
        );
    }
    let bytes = match to_bytes(body, max).await {
        Ok(b) => b,
        Err(_) => {
            return cors(
                reject(&s, "too_large", StatusCode::PAYLOAD_TOO_LARGE),
                origin,
                true,
            );
        }
    };

    // 2 validate + enrich (the IP is never read: R-P1)
    let hdr = |n: &str| {
        parts
            .headers
            .get(n)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    };
    let info = RequestInfo {
        user_agent: hdr("user-agent"),
        country: hdr(&s.cfg.server.country_header),
        dnt: hdr("dnt") == "1" || hdr("sec-gpc") == "1",
    };
    let day = day_of(now);
    let processed = match collect::process(&bytes, site, &info, &s.cfg.ingest, now) {
        Ok(p) => p,
        Err(BatchError::Bot) => {
            s.metrics.inc_label("dropped_total", "bot", 1);
            return cors(StatusCode::NO_CONTENT.into_response(), origin, true);
        }
        Err(BatchError::DoNotTrack) => {
            s.metrics.inc_label("dropped_total", "dnt", 1);
            return cors(StatusCode::NO_CONTENT.into_response(), origin, true);
        }
        Err(BatchError::Malformed) => {
            return cors(
                reject(&s, "malformed", StatusCode::BAD_REQUEST),
                origin,
                true,
            );
        }
        Err(BatchError::TooManyEvents) => {
            return cors(
                reject(&s, "too_many_events", StatusCode::PAYLOAD_TOO_LARGE),
                origin,
                true,
            );
        }
        Err(BatchError::BadIds) => {
            return cors(reject(&s, "bad_ids", StatusCode::BAD_REQUEST), origin, true);
        }
    };
    let mut dropped = processed.dropped_count();
    for (reason, n) in &processed.dropped {
        s.metrics.inc_label("dropped_total", reason, *n);
    }
    s.metrics.inc(&s.metrics.late_events, processed.late);

    // 3 dedup + shed check
    let fill = s.spool.fill_pct();
    let mut keep = Vec::with_capacity(processed.events.len());
    let mono = Instant::now();
    let mut full = false;
    for e in processed.events {
        let dup = s
            .dedup
            .lock()
            .map(|mut d| d.contains(&e.site_id, &e.session_id, e.seq, mono))
            .unwrap_or(false);
        if dup {
            s.metrics.inc(&s.metrics.duplicates_dropped, 1);
            dropped += 1;
            continue;
        }
        let p = Priority::of(&e.event_name);
        match collect::admission(p, fill, s.cfg.spool.shed_p2_pct, s.cfg.spool.shed_p1_pct) {
            Admission::Admit => keep.push(e),
            Admission::Shed => {
                s.metrics.inc_label("shed_total", p.as_str(), 1);
                dropped += 1;
            }
            Admission::Full => full = true,
        }
    }
    if full {
        return cors(
            reject(&s, "spool_full", StatusCode::SERVICE_UNAVAILABLE),
            origin,
            true,
        );
    }
    if keep.is_empty() {
        s.metrics
            .usage_add(&site.id, &day, |u| u.dropped += dropped);
        return cors(StatusCode::NO_CONTENT.into_response(), origin, true);
    }

    // 4 spool: acknowledge only once the frame is on disk
    let n = keep.len() as u64;
    let batch = SpoolBatch {
        site_id: site.id.clone(),
        received_at: now,
        events: keep,
    };
    match s.spool.append(&batch).await {
        Ok(()) => {}
        Err(SpoolError::Full) => {
            return cors(
                reject(&s, "spool_full", StatusCode::SERVICE_UNAVAILABLE),
                origin,
                true,
            );
        }
        Err(SpoolError::Io(_)) => {
            return cors(
                reject(&s, "spool_io", StatusCode::SERVICE_UNAVAILABLE),
                origin,
                true,
            );
        }
    }
    if let Ok(mut d) = s.dedup.lock() {
        for e in &batch.events {
            d.insert(&e.site_id, &e.session_id, e.seq, mono);
        }
    }
    s.metrics.inc(&s.metrics.accepted, n);
    s.metrics.usage_add(&site.id, &day, |u| {
        u.accepted += n;
        u.dropped += dropped;
    });
    cors(StatusCode::NO_CONTENT.into_response(), origin, true)
}

/// Persist/restore per-site usage so `/api/v1/usage` survives a restart.
pub fn save_usage(dir: &std::path::Path, m: &Metrics) -> std::io::Result<()> {
    let rows: Vec<(String, String, crate::metrics::UsageDay)> = m
        .usage_snapshot()
        .into_iter()
        .map(|((s, d), u)| (s, d, u))
        .collect();
    let tmp = dir.join("usage.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&rows).unwrap_or_default())?;
    std::fs::rename(tmp, dir.join("usage.json"))
}

pub fn load_usage(dir: &std::path::Path, m: &Metrics) {
    let Ok(s) = std::fs::read_to_string(dir.join("usage.json")) else {
        return;
    };
    if let Ok(rows) = serde_json::from_str::<Vec<(String, String, crate::metrics::UsageDay)>>(&s) {
        m.usage_restore(rows.into_iter().map(|(s, d, u)| ((s, d), u)).collect());
    }
}
