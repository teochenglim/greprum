//! Ingest pipeline: validate, normalise, enrich, classify (PRD §3 caps, §7 step 2).
//!
//! Pure functions over bytes + a clock value, so every rule is unit-testable.

pub mod dedup;
pub mod identity;
pub mod ua;

use crate::{
    config::{IngestConfig, SiteConfig},
    model::{BUILTIN_EVENTS, Priority, StoredEvent},
};
use serde::Deserialize;
use serde_json::{Map, Value};

pub const ID_MAX: usize = 200;
pub const PROPS_MAX_BYTES: usize = 4096;
pub const CTX_MAX_KEYS: usize = 10;
pub const CTX_KEY_MAX: usize = 32;
pub const CTX_VAL_MAX: usize = 200;
pub const SHORT_MAX: usize = 64;
pub const TEXT_MAX: usize = 50;
pub const ATTRS_MAX_BYTES: usize = 2048;
pub const PATH_MAX: usize = 512;

/// Why a whole batch is refused (nothing accepted).
#[derive(Debug, PartialEq, Eq)]
pub enum BatchError {
    Malformed,
    TooManyEvents,
    BadIds,
    /// Site setting honours Do Not Track and the request carried `DNT: 1`.
    DoNotTrack,
    Bot,
}

/// Per-request facts the gateway derives from headers (never the IP itself).
#[derive(Debug, Clone, Default)]
pub struct RequestInfo {
    pub user_agent: String,
    pub country: String,
    pub dnt: bool,
}

#[derive(Debug, Default)]
pub struct Processed {
    pub events: Vec<StoredEvent>,
    /// `(reason, count)` of events dropped by validation.
    pub dropped: Vec<(&'static str, u64)>,
    pub late: u64,
}

impl Processed {
    fn drop_one(&mut self, reason: &'static str) {
        if let Some(e) = self.dropped.iter_mut().find(|(r, _)| *r == reason) {
            e.1 += 1;
        } else {
            self.dropped.push((reason, 1));
        }
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped.iter().map(|(_, n)| n).sum()
    }
}

#[derive(Deserialize)]
struct RawBatch {
    token: Option<String>,
    visitor_id: Option<String>,
    session_id: Option<String>,
    sent_at: Option<i64>,
    sample_rate: Option<f64>,
    env: Option<String>,
    release: Option<String>,
    ctx: Option<Map<String, Value>>,
    events: Vec<Value>,
}

pub fn valid_custom_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b':' | b'-'))
}

/// Drop query + hash, ensure leading slash, cap length. Never keeps secrets in URLs.
pub fn normalise_path(raw: &str) -> String {
    let mut p = raw;
    // Accept full URLs defensively: keep only the path.
    if let Some(i) = p.find("://").filter(|_| !p.starts_with('/')) {
        let rest = &p[i + 3..];
        p = match rest.find('/') {
            Some(j) => &rest[j..],
            None => "/",
        };
    }
    let end = p.find(['?', '#']).unwrap_or(p.len());
    let mut s = p[..end].to_string();
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    cut(&s, PATH_MAX)
}

fn looks_like_id(seg: &str) -> bool {
    let all = |s: &str, f: fn(char) -> bool| !s.is_empty() && s.chars().all(f);
    if all(seg, |c| c.is_ascii_digit()) {
        return true;
    }
    let uuid = seg.len() == 36
        && seg.chars().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        });
    uuid || (seg.len() >= 16 && all(seg, |c| c.is_ascii_hexdigit()))
}

/// Apply a site's route templates to an already normalised path (R-G18).
pub fn template_path(site: &SiteConfig, path: &str) -> String {
    for r in &site.routes {
        let pat: Vec<&str> = r.pattern.trim_matches('/').split('/').collect();
        let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
        if pat.len() == seg.len() && pat.iter().zip(&seg).all(|(p, s)| *p == "*" || p == s) {
            return r.template.clone();
        }
    }
    if site.template_ids && path != "/" {
        let t: Vec<String> = path
            .trim_matches('/')
            .split('/')
            .map(|s| {
                if looks_like_id(s) {
                    ":id".to_string()
                } else {
                    s.to_string()
                }
            })
            .collect();
        return format!("/{}", t.join("/"));
    }
    path.to_string()
}

pub fn referrer_host(raw: &str) -> String {
    url::Url::parse(raw)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .unwrap_or_default()
}

/// Truncate on a char boundary.
pub fn cut(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn str_of(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn clean_ctx(ctx: Option<Map<String, Value>>) -> String {
    let Some(m) = ctx else { return String::new() };
    let mut out = Map::new();
    for (k, v) in m.into_iter().take(CTX_MAX_KEYS) {
        if k.is_empty() || k.len() > CTX_KEY_MAX {
            continue;
        }
        let val = match v {
            Value::String(s) => cut(&s, CTX_VAL_MAX),
            Value::Number(_) | Value::Bool(_) => cut(&v.to_string(), CTX_VAL_MAX),
            _ => continue,
        };
        out.insert(k, Value::String(val));
    }
    if out.is_empty() {
        String::new()
    } else {
        Value::Object(out).to_string()
    }
}

/// Sample rate rule: greater than 0 and up to 1, else stored as 1.
pub fn clean_sample_rate(r: Option<f64>) -> f64 {
    match r {
        Some(x) if x.is_finite() && x > 0.0 && x <= 1.0 => x,
        _ => 1.0,
    }
}

/// Clock fix (ADR 0009). Returns `(ts, is_late)` or `None` when too old.
pub fn fix_clock(
    ts: i64,
    sent_at: Option<i64>,
    received_at: i64,
    cfg: &IngestConfig,
) -> Option<(i64, bool)> {
    let mut t = match sent_at {
        Some(s) => received_at - (s - ts),
        None => ts,
    };
    if t > received_at + cfg.future_clamp_secs * 1000 {
        t = received_at;
    }
    let age = received_at - t;
    if age > cfg.drop_secs * 1000 {
        return None;
    }
    Some((t, age > cfg.late_secs * 1000))
}

/// Validate and normalise one batch. `received_at` is epoch ms.
pub fn process(
    body: &[u8],
    site: &SiteConfig,
    req: &RequestInfo,
    cfg: &IngestConfig,
    received_at: i64,
) -> Result<Processed, BatchError> {
    if site.honour_dnt && req.dnt {
        return Err(BatchError::DoNotTrack);
    }
    let agent = ua::parse(&req.user_agent);
    if cfg.drop_bots && agent.bot {
        return Err(BatchError::Bot);
    }
    let batch: RawBatch = serde_json::from_slice(body).map_err(|_| BatchError::Malformed)?;
    if batch.events.len() > cfg.max_batch_events {
        return Err(BatchError::TooManyEvents);
    }
    let visitor = batch.visitor_id.unwrap_or_default();
    let session = batch.session_id.unwrap_or_default();
    // Ids are never truncated: truncation would merge different ids (ADR 0015).
    if visitor.is_empty() || session.is_empty() || visitor.len() > ID_MAX || session.len() > ID_MAX
    {
        return Err(BatchError::BadIds);
    }
    let user_id = batch
        .token
        .as_deref()
        .and_then(|t| identity::verify(t, &site.hmac_secret, site.max_token_ttl_secs));
    let rate = clean_sample_rate(batch.sample_rate);
    let env = cut(batch.env.as_deref().unwrap_or(""), SHORT_MAX);
    let release = cut(batch.release.as_deref().unwrap_or(""), SHORT_MAX);
    let ctx = clean_ctx(batch.ctx);

    let mut out = Processed::default();
    for ev in &batch.events {
        let name = str_of(ev, "name");
        if name.starts_with('$') {
            if !BUILTIN_EVENTS.contains(&name.as_str()) {
                out.drop_one("unknown_builtin");
                continue;
            }
        } else if !valid_custom_name(&name) {
            out.drop_one("bad_name");
            continue;
        }
        let seq = match ev.get("seq").and_then(Value::as_i64) {
            Some(s) if (0..=i32::MAX as i64).contains(&s) => s as i32,
            _ => {
                out.drop_one("bad_seq");
                continue;
            }
        };
        let Some(ts) = ev.get("ts").and_then(Value::as_i64) else {
            out.drop_one("bad_ts");
            continue;
        };
        let Some((ts, is_late)) = fix_clock(ts, batch.sent_at, received_at, cfg) else {
            out.drop_one("too_old");
            continue;
        };
        let props = match ev.get("props") {
            Some(Value::Object(m)) if !m.is_empty() => {
                let mut m = m.clone();
                if name == "$session_start" {
                    m.retain(|k, _| {
                        matches!(k.as_str(), "utm_source" | "utm_medium" | "utm_campaign")
                    });
                }
                if m.is_empty() {
                    String::new()
                } else {
                    Value::Object(m).to_string()
                }
            }
            _ => String::new(),
        };
        if props.len() > PROPS_MAX_BYTES {
            out.drop_one("props_too_large");
            continue;
        }
        let el = ev.get("element");
        let el_s = |k: &str| el.map(|e| str_of(e, k)).unwrap_or_default();
        let el_attrs = match el.and_then(|e| e.get("attrs")) {
            Some(Value::Object(m)) => {
                let mut m = m.clone();
                m.retain(|k, _| !site.drop_attrs.iter().any(|d| d.eq_ignore_ascii_case(k)));
                if m.is_empty() {
                    String::new()
                } else {
                    cut(&Value::Object(m).to_string(), ATTRS_MAX_BYTES)
                }
            }
            _ => String::new(),
        };
        let el_text = if site.store_text {
            cut(&el_s("text"), TEXT_MAX)
        } else {
            String::new()
        };
        if is_late {
            out.late += 1;
        }
        out.events.push(StoredEvent {
            ts,
            site_id: site.id.clone(),
            event_name: name.clone(),
            visitor_id: visitor.clone(),
            session_id: session.clone(),
            user_id: user_id.clone().unwrap_or_default(),
            is_identified: user_id.is_some(),
            is_new_visitor: name == "$session_start"
                && ev
                    .get("is_new_visitor")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            seq,
            is_late,
            path: template_path(site, &normalise_path(&str_of(ev, "path"))),
            page_group: cut(&str_of(ev, "page_group"), SHORT_MAX),
            parent_path: match str_of(ev, "parent_path") {
                p if p.is_empty() => String::new(),
                p => template_path(site, &normalise_path(&p)),
            },
            referrer_host: referrer_host(&str_of(ev, "referrer")),
            el_tag: cut(&el_s("tag"), 32),
            el_id: cut(&el_s("id"), 128),
            el_text,
            el_css: cut(&el_s("css"), 256),
            el_attrs,
            props,
            ctx: ctx.clone(),
            env: env.clone(),
            release: release.clone(),
            sample_rate: rate,
            device: agent.device.into(),
            browser: agent.browser.into(),
            os: agent.os.into(),
            country: cut(&req.country.to_ascii_uppercase(), 2),
        });
    }
    Ok(out)
}

/// Decide what the spool fill level allows (PRD §7 priority table).
#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    Admit,
    Shed,
    /// Spool is full: P0 gets `503`.
    Full,
}

pub fn admission(p: Priority, fill_pct: f64, shed_p2: u8, shed_p1: u8) -> Admission {
    match p {
        Priority::P0 if fill_pct >= 100.0 => Admission::Full,
        Priority::P0 => Admission::Admit,
        Priority::P1 if fill_pct >= shed_p1 as f64 => Admission::Shed,
        Priority::P2 if fill_pct >= shed_p2 as f64 => Admission::Shed,
        _ => Admission::Admit,
    }
}
