//! One-process init + serve for the compose quickstart (ADR 0053): `playground` runs
//! `migrate --wait`, seeds guarded demo data and then serves, so no one-shot containers.

use crate::{
    config::Config,
    query::{Bind, Q, Reader},
};

/// The site `--demo-data` seeds. Seeding is skipped unless the config defines this site,
/// so a production config swapped in via `GREPRUM_CONFIG` never receives synthetic rows.
pub const DEMO_SITE: &str = "demo";

/// The site demo data may be written for: `demo`, and only when the config knows it.
pub fn seed_site(cfg: &Config) -> Option<String> {
    cfg.sites
        .iter()
        .any(|s| s.id == DEMO_SITE)
        .then(|| DEMO_SITE.to_string())
}

/// Any events for the site in `[from_ms, …]`? A restarted playground must not reseed —
/// append-mode tables reject `DELETE` (ADR 0029), duplicates would double every count.
/// Fixed SQL, bound site, capped window (ADR 0024).
pub fn has_events(site: &str, from_ms: i64) -> Q {
    Q {
        name: "playground_seeded",
        sql: "SELECT CAST(count(*) AS BIGINT) AS n FROM events WHERE site_id = ? AND ts >= ?"
            .into(),
        binds: vec![Bind::Str(site.into()), Bind::Int(from_ms)],
    }
}

/// Run the [`has_events`] guard against a reader.
pub async fn demo_data_present<R: Reader + ?Sized>(
    reader: &R,
    site: &str,
    from_ms: i64,
) -> anyhow::Result<bool> {
    let rows = reader.fetch(&has_events(site, from_ms)).await?;
    Ok(rows.first().and_then(|r| r["n"].as_i64()).unwrap_or(0) > 0)
}
