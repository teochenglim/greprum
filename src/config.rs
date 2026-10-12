//! Config file (TOML). Site settings live here; there is no settings screen in v1.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{collections::HashSet, path::Path};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub greptime: GreptimeConfig,
    #[serde(default)]
    pub spool: SpoolConfig,
    #[serde(default)]
    pub ingest: IngestConfig,
    #[serde(default, rename = "site")]
    pub sites: Vec<SiteConfig>,
    #[serde(default, rename = "api_key")]
    pub api_keys: Vec<ApiKeyConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub listen: String,
    /// Tracker path, served from the customer's domain (ADR 0025).
    pub tracker_path: String,
    pub collect_path: String,
    /// Header carrying an ISO country code set by a trusted proxy (e.g. `CF-IPCountry`).
    pub country_header: String,
    /// Header carrying the client IP set by a trusted proxy. Empty = use the socket peer.
    pub ip_header: String,
    pub rate_limit_per_min: u32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8080".into(),
            tracker_path: "/t.js".into(),
            collect_path: "/collect".into(),
            country_header: "cf-ipcountry".into(),
            ip_header: String::new(),
            rate_limit_per_min: 600,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GreptimeConfig {
    pub write_url: String,
    /// Falls back to `write_url` when empty (separate read user, spike item 6).
    pub read_url: String,
    pub ttl: String,
    /// Prepended to flow names: flows are unique per catalog (ADR 0032).
    pub flow_prefix: String,
}

impl Default for GreptimeConfig {
    fn default() -> Self {
        Self {
            write_url: "mysql://root@127.0.0.1:4002/public".into(),
            read_url: String::new(),
            ttl: "13 months".into(),
            flow_prefix: String::new(),
        }
    }
}

impl GreptimeConfig {
    pub fn read_url(&self) -> &str {
        if self.read_url.is_empty() {
            &self.write_url
        } else {
            &self.read_url
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SpoolConfig {
    pub dir: String,
    pub segment_bytes: u64,
    pub cap_bytes: u64,
    pub fsync_ms: u64,
    pub fsync_bytes: u64,
    pub strict_fsync: bool,
    pub shed_p2_pct: u8,
    pub shed_p1_pct: u8,
}

impl Default for SpoolConfig {
    fn default() -> Self {
        Self {
            dir: "./spool".into(),
            segment_bytes: 64 * 1024 * 1024,
            cap_bytes: 2 * 1024 * 1024 * 1024,
            fsync_ms: 100,
            fsync_bytes: 1024 * 1024,
            strict_fsync: false,
            shed_p2_pct: 50,
            shed_p1_pct: 80,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct IngestConfig {
    pub dedup_secs: u64,
    pub dedup_max_entries: usize,
    pub future_clamp_secs: i64,
    pub late_secs: i64,
    pub drop_secs: i64,
    pub fresh_batch: usize,
    pub fresh_interval_ms: u64,
    pub backfill_batch: usize,
    /// Fresh lane failing this long demotes its pending range to the backfill lane.
    pub fresh_stall_secs: u64,
    pub max_batch_events: usize,
    pub max_body_bytes: usize,
    pub drop_bots: bool,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            dedup_secs: 600,
            dedup_max_entries: 2_000_000,
            future_clamp_secs: 300,
            late_secs: 86_400,
            drop_secs: 7 * 86_400,
            fresh_batch: 500,
            fresh_interval_ms: 1000,
            backfill_batch: 5000,
            fresh_stall_secs: 30,
            max_batch_events: 50,
            max_body_bytes: 64 * 1024,
            drop_bots: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SiteConfig {
    pub id: String,
    /// Public write-only site key, `pk_xxx`.
    pub key: String,
    #[serde(default)]
    pub origins: Vec<String>,
    /// Per-site HMAC secret for `identify(token)` JWTs.
    #[serde(default)]
    pub hmac_secret: String,
    #[serde(default = "default_tz")]
    pub timezone: String,
    /// Attribute names dropped from `el_attrs`.
    #[serde(default)]
    pub drop_attrs: Vec<String>,
    /// Keep element text (cut to 50 chars). Set false to store none.
    #[serde(default = "yes")]
    pub store_text: bool,
    /// Honour Do Not Track on the server side too.
    #[serde(default)]
    pub honour_dnt: bool,
}

fn default_tz() -> String {
    "UTC".into()
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiKeyConfig {
    pub name: String,
    /// Lower-case hex SHA-256 of the key. Keys are never stored in clear.
    pub sha256: String,
    pub sites: Vec<String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&text).context("parsing config")?;
        cfg.apply_env(|k| std::env::var(k).ok());
        cfg.validate()?;
        Ok(cfg)
    }

    /// Container-friendly overrides. Only listen address, spool dir and DB URLs: secrets
    /// and sites stay in the config file.
    pub fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) {
        let nonempty = |k: &str| get(k).filter(|v| !v.is_empty());
        if let Some(v) = nonempty("GREPRUM_LISTEN") {
            self.server.listen = v;
        }
        if let Some(v) = nonempty("GREPRUM_SPOOL_DIR") {
            self.spool.dir = v;
        }
        if let Some(v) = nonempty("GREPRUM_GREPTIME_WRITE_URL") {
            self.greptime.write_url = v;
        }
        if let Some(v) = nonempty("GREPRUM_GREPTIME_READ_URL") {
            self.greptime.read_url = v;
        }
    }

    pub fn parse(text: &str) -> Result<Config> {
        let cfg: Config = toml::from_str(text).context("parsing config")?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<()> {
        if self.server.listen.parse::<std::net::SocketAddr>().is_err() {
            bail!(
                "server.listen must be host:port, got {}",
                self.server.listen
            );
        }
        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        for s in &self.sites {
            if s.id.is_empty() || s.id.len() > 64 {
                bail!("site id must be 1..=64 chars");
            }
            if !s.key.starts_with("pk_") {
                bail!("site {}: key must start with pk_", s.id);
            }
            if !ids.insert(&s.id) || !keys.insert(&s.key) {
                bail!("duplicate site id or key: {}", s.id);
            }
            if s.timezone.parse::<chrono_tz::Tz>().is_err() {
                bail!("site {}: unknown timezone {}", s.id, s.timezone);
            }
        }
        for k in &self.api_keys {
            if k.sha256.len() != 64 || !k.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("api_key {}: sha256 must be 64 hex chars", k.name);
            }
            for s in &k.sites {
                if !ids.contains(s) {
                    bail!("api_key {}: unknown site {}", k.name, s);
                }
            }
        }
        crate::migrate::validate_prefix(&self.greptime.flow_prefix)?;
        crate::migrate::validate_ttl(&self.greptime.ttl)?;
        if self.spool.shed_p2_pct > self.spool.shed_p1_pct || self.spool.shed_p1_pct > 100 {
            bail!("spool shed thresholds must satisfy p2 <= p1 <= 100");
        }
        Ok(())
    }

    pub fn site_by_key(&self, key: &str) -> Option<&SiteConfig> {
        self.sites.iter().find(|s| s.key == key)
    }

    pub fn site(&self, id: &str) -> Option<&SiteConfig> {
        self.sites.iter().find(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ac_0_1_1_a_defaults_match_prd() {
        let c = Config::parse("").unwrap();
        assert_eq!(c.spool.segment_bytes, 64 * 1024 * 1024);
        assert_eq!(c.spool.cap_bytes, 2 * 1024 * 1024 * 1024);
        assert_eq!(c.spool.fsync_ms, 100);
        assert_eq!(c.spool.shed_p2_pct, 50);
        assert_eq!(c.spool.shed_p1_pct, 80);
        assert_eq!(c.ingest.dedup_secs, 600);
        assert_eq!(c.ingest.fresh_batch, 500);
        assert_eq!(c.ingest.backfill_batch, 5000);
        assert_eq!(c.ingest.future_clamp_secs, 300);
        assert_eq!(c.ingest.late_secs, 86_400);
        assert_eq!(c.ingest.drop_secs, 7 * 86_400);
        assert_eq!(c.ingest.max_body_bytes, 64 * 1024);
        assert_eq!(c.ingest.max_batch_events, 50);
        assert_eq!(c.server.listen, "127.0.0.1:8080");
    }

    #[test]
    fn ac_0_1_1_b_rejects_bad_site_key_and_unknown_site_ref() {
        assert!(Config::parse("[[site]]\nid='a'\nkey='nope'").is_err());
        let bad = "[[site]]\nid='a'\nkey='pk_a'\n[[api_key]]\nname='k'\nsha256='".to_string()
            + &"0".repeat(64)
            + "'\nsites=['zzz']";
        assert!(Config::parse(&bad).is_err());
    }

    #[test]
    fn ac_0_1_1_c_rejects_unknown_timezone_and_bad_thresholds() {
        assert!(Config::parse("[[site]]\nid='a'\nkey='pk_a'\ntimezone='Mars/Base'").is_err());
        assert!(Config::parse("[spool]\nshed_p2_pct=90\nshed_p1_pct=80").is_err());
    }

    #[test]
    fn ac_0_1_1_d_binds_loopback_by_default() {
        let c = Config::parse("").unwrap();
        assert!(c.server.listen.starts_with("127.0.0.1:"));
        assert!(c.greptime.write_url.contains("127.0.0.1"));
    }
}

#[cfg(test)]
mod env_tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn ac_0_7_3_a_env_overrides_listen_spool_and_db_urls() {
        let mut c = Config::default();
        c.apply_env(env(&[
            ("GREPRUM_LISTEN", "0.0.0.0:9000"),
            ("GREPRUM_SPOOL_DIR", "/data/spool"),
            ("GREPRUM_GREPTIME_WRITE_URL", "mysql://w@db:4002/public"),
            ("GREPRUM_GREPTIME_READ_URL", "mysql://r@db:4002/public"),
        ]));
        assert_eq!(c.server.listen, "0.0.0.0:9000");
        assert_eq!(c.spool.dir, "/data/spool");
        assert_eq!(c.greptime.write_url, "mysql://w@db:4002/public");
        assert_eq!(c.greptime.read_url(), "mysql://r@db:4002/public");
    }

    #[test]
    fn ac_0_7_3_b_empty_env_values_do_not_override_and_bad_listen_is_refused() {
        let mut c = Config::default();
        c.apply_env(env(&[("GREPRUM_LISTEN", ""), ("GREPRUM_SPOOL_DIR", "")]));
        assert_eq!(c.server.listen, "127.0.0.1:8080");
        c.apply_env(env(&[("GREPRUM_LISTEN", "not-an-address")]));
        assert!(c.validate().is_err());
    }
}
