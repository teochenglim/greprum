//! Query API (PRD §8): fixed table names, bound parameters, mandatory `site_id`.
//!
//! * [`build`] — pure SQL builders (`Params` → [`Q`]); nothing from the request is ever
//!   formatted into SQL text. Dimension/bucket/metric names are mapped through fixed
//!   `match`es to literals.
//! * [`Reader`] — executes a [`Q`]; `SqlxReader` for GreptimeDB, fakes in tests.
//! * the router — auth, site scoping, time caps, response shaping.

pub mod build;
pub mod reader;

use crate::{config::SiteConfig, server::AppState};
use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc};

pub use reader::SqlxReader;

pub const DAY_MS: i64 = 86_400_000;
pub const MAX_RAW_RANGE_MS: i64 = 31 * DAY_MS;
pub const DEFAULT_SESSIONS_RANGE_MS: i64 = DAY_MS;

#[derive(Debug, Clone, PartialEq)]
pub enum Bind {
    Str(String),
    Int(i64),
    Float(f64),
}

/// A query: SQL text that contains only fixed fragments and `?`, plus its binds.
#[derive(Debug, Clone, PartialEq)]
pub struct Q {
    pub name: &'static str,
    pub sql: String,
    pub binds: Vec<Bind>,
}

#[async_trait]
pub trait Reader: Send + Sync {
    /// Rows as JSON objects keyed by column alias.
    async fn fetch(&self, q: &Q) -> anyhow::Result<Vec<Value>>;
}

#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn bad(m: impl Into<String>) -> Self {
        ApiError(StatusCode::BAD_REQUEST, m.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

/// Every `/api/v1` endpoint: `(path-pattern, kind)`. Site-isolation tests iterate this.
pub const ENDPOINTS: &[(&str, Kind)] = &[
    ("/api/v1/stats/overview", Kind::Overview),
    ("/api/v1/stats/timeseries", Kind::Timeseries),
    ("/api/v1/stats/top-pages", Kind::TopPages),
    ("/api/v1/stats/entry-pages", Kind::EntryPages),
    ("/api/v1/stats/journeys", Kind::Journeys),
    ("/api/v1/stats/top-elements", Kind::TopElements),
    ("/api/v1/stats/funnel", Kind::Funnel),
    ("/api/v1/stats/retention", Kind::Retention),
    ("/api/v1/stats/rage-clicks", Kind::RageClicks),
    ("/api/v1/stats/browsers-devices", Kind::BrowsersDevices),
    ("/api/v1/sessions", Kind::Sessions),
    ("/api/v1/sessions/{id}", Kind::SessionDetail),
    ("/api/v1/events", Kind::Events),
    ("/api/v1/usage", Kind::Usage),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Overview,
    Timeseries,
    TopPages,
    EntryPages,
    Journeys,
    TopElements,
    Funnel,
    Retention,
    RageClicks,
    BrowsersDevices,
    Sessions,
    SessionDetail,
    Events,
    Usage,
}

pub fn router(_s: Arc<AppState>) -> Router<Arc<AppState>> {
    let mut r = Router::new();
    for (path, kind) in ENDPOINTS {
        let k = *kind;
        let route = path.replace("{id}", ":id");
        r = r.route(
            &route,
            get(
                move |State(s): State<Arc<AppState>>,
                      h: HeaderMap,
                      p: Option<Path<String>>,
                      q: Query<HashMap<String, String>>| async move {
                    handle(s, h, k, p.map(|Path(i)| i), q.0).await
                },
            ),
        );
    }
    r
}

/// SHA-256 hex of an API key (what `[[api_key]].sha256` must hold).
pub fn hash_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// Authenticate and resolve the authorised site. 401 = no/unknown key, 403 = other site.
pub fn authorise<'a>(
    cfg: &'a crate::config::Config,
    headers: &HeaderMap,
    requested: Option<&str>,
) -> Result<(&'a SiteConfig, Vec<&'a str>), ApiError> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(ApiError(
            StatusCode::UNAUTHORIZED,
            "missing bearer key".into(),
        ))?;
    let h = hash_key(token);
    let key = cfg
        .api_keys
        .iter()
        .find(|k| k.sha256.eq_ignore_ascii_case(&h))
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "unknown key".into()))?;
    let allowed: Vec<&str> = key.sites.iter().map(String::as_str).collect();
    let id = match requested {
        Some(s) if allowed.contains(&s) => s,
        Some(_) => {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "site not allowed for this key".into(),
            ));
        }
        None if allowed.len() == 1 => allowed[0],
        None => {
            return Err(ApiError::bad(
                "`site` is required for keys with several sites",
            ));
        }
    };
    let site = cfg
        .site(id)
        .ok_or(ApiError(StatusCode::FORBIDDEN, "unknown site".into()))?;
    Ok((site, allowed))
}

async fn handle(
    s: Arc<AppState>,
    headers: HeaderMap,
    kind: Kind,
    id: Option<String>,
    q: HashMap<String, String>,
) -> Response {
    match run(&s, &headers, kind, id, &q).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn run(
    s: &AppState,
    headers: &HeaderMap,
    kind: Kind,
    id: Option<String>,
    q: &HashMap<String, String>,
) -> Result<Value, ApiError> {
    let (site, allowed) = authorise(&s.cfg, headers, q.get("site").map(String::as_str))?;
    let now = (s.clock)();
    let p = build::Params::parse(kind, site, q, id, now)?;

    if kind == Kind::Usage {
        let rows: Vec<Value> = s
            .metrics
            .usage_snapshot()
            .into_iter()
            .filter(|((site_id, day), _)| allowed.contains(&site_id.as_str()) && build::day_in_range(day, p.from, p.to))
            .map(|((site_id, day), u)| json!({"site": site_id, "day": day, "accepted": u.accepted, "rejected": u.rejected, "dropped": u.dropped}))
            .collect();
        return Ok(json!({"site": p.site, "from": p.from, "to": p.to, "data": rows}));
    }

    let reader = s.reader.as_ref().ok_or(ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "no database configured".into(),
    ))?;
    let plan = build::plan(kind, &p)?;
    let mut results: HashMap<&'static str, Vec<Value>> = HashMap::new();
    for qq in &plan {
        let rows = reader
            .fetch(qq)
            .await
            .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, format!("database: {e}")))?;
        results.insert(qq.name, rows);
    }
    let data = build::shape(kind, &p, &results);
    Ok(json!({"site": p.site, "from": p.from, "to": p.to, "data": data}))
}
