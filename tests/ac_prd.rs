//! AC0.9.3 route templates, AC0.9.4 GeoIP, AC0.9.5 token lifetime cap.
mod common;
use axum::{body::Body, http::Request};
use common::{app::*, *};
use greprum::{
    collect,
    config::{Config, RouteRule},
    spool::{self, Pos, Read_},
};
use serde_json::json;

fn site_with(routes: &[(&str, &str)], template_ids: bool) -> greprum::config::SiteConfig {
    let mut s = site();
    s.routes = routes
        .iter()
        .map(|(p, t)| RouteRule {
            pattern: p.to_string(),
            template: t.to_string(),
        })
        .collect();
    s.template_ids = template_ids;
    s
}

fn first_event_with(
    s: &greprum::config::SiteConfig,
    path: &str,
    parent: &str,
) -> greprum::model::StoredEvent {
    let mut e = ev("$pageview", 0);
    e["path"] = json!(path);
    e["parent_path"] = json!(parent);
    let b = batch(vec![e]).to_string();
    collect::process(b.as_bytes(), s, &req(), &cfg(), NOW)
        .unwrap()
        .events
        .remove(0)
}

// ---- AC0.9.3 route templates --------------------------------------------------

#[test]
fn ac_0_9_3_a_patterns_and_id_segments_are_templated() {
    let s = site_with(&[("/users/*/orders", "/users/:id/orders")], false);
    assert_eq!(
        collect::template_path(&s, "/users/42/orders"),
        "/users/:id/orders"
    );
    assert_eq!(
        collect::template_path(&s, "/users/alice/orders"),
        "/users/:id/orders"
    );
    let ids = site_with(&[], true);
    assert_eq!(collect::template_path(&ids, "/orders/12345"), "/orders/:id");
    assert_eq!(
        collect::template_path(&ids, "/o/550e8400-e29b-41d4-a716-446655440000/items"),
        "/o/:id/items"
    );
    assert_eq!(
        collect::template_path(&ids, "/s/0123456789abcdef0123"),
        "/s/:id"
    );
    assert_eq!(
        collect::template_path(&ids, "/blog/2024-review"),
        "/blog/2024-review",
        "words with digits stay"
    );
}

#[test]
fn ac_0_9_3_b_non_matching_paths_are_untouched() {
    let s = site_with(&[("/users/*/orders", "/users/:id/orders")], false);
    for p in [
        "/",
        "/users",
        "/users/42",
        "/users/42/orders/7",
        "/other/42/orders",
    ] {
        assert_eq!(collect::template_path(&s, p), p);
    }
    assert_eq!(
        collect::template_path(&site_with(&[], false), "/orders/12345"),
        "/orders/12345",
        "ids off by default"
    );
}

#[test]
fn ac_0_9_3_c_first_match_wins_and_invalid_patterns_refused_at_config_load() {
    let s = site_with(&[("/a/*", "/first"), ("/a/b", "/second")], false);
    assert_eq!(collect::template_path(&s, "/a/b"), "/first");
    let bad = "[[site]]\nid='a'\nkey='pk_a'\n[[site.routes]]\npattern='users/*'\ntemplate='/u'";
    assert!(Config::parse(bad).is_err());
    let ok = "[[site]]\nid='a'\nkey='pk_a'\n[[site.routes]]\npattern='/users/*'\ntemplate='/u/:id'";
    assert_eq!(Config::parse(ok).unwrap().sites[0].routes.len(), 1);
}

#[test]
fn ac_0_9_3_d_stored_path_and_parent_path_carry_the_template_so_rollups_group_by_it() {
    let s = site_with(&[("/users/*/orders", "/users/:id/orders")], true);
    let e = first_event_with(&s, "/users/42/orders?x=1#h", "/items/99");
    assert_eq!(e.path, "/users/:id/orders");
    assert_eq!(e.parent_path, "/items/:id");
}

// ---- AC0.9.4 GeoIP ------------------------------------------------------------

const MMDB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/country.mmdb");

#[test]
fn ac_0_9_4_a_lookup_against_the_fixture_database() {
    let g = greprum::geoip::GeoIp::open(MMDB).unwrap();
    assert_eq!(g.country("203.0.113.9".parse().unwrap()), "SG");
    assert_eq!(g.country("198.51.100.1".parse().unwrap()), "DE");
    assert_eq!(
        g.country("192.0.2.1".parse().unwrap()),
        "",
        "not in the database"
    );
}

#[test]
fn ac_0_9_4_b_missing_database_falls_back_to_header_country_only() {
    assert!(greprum::geoip::GeoIp::open("/nonexistent/db.mmdb").is_err());
    let h = harness_with(|c| c.geoip.database = "/nonexistent/db.mmdb".into());
    assert!(
        h.state.geoip.is_none(),
        "unusable database disables lookup, gateway still starts"
    );
}

#[tokio::test]
async fn ac_0_9_4_c_ip_is_never_stored_spooled_and_header_country_wins() {
    let h = harness_with(|c| {
        c.geoip.database = MMDB.into();
        c.server.ip_header = "x-forwarded-for".into();
    });
    let post = |ip: &str, country: Option<&str>| {
        let mut r = Request::post("/collect?k=pk_test")
            .header("origin", "https://example.com")
            .header("user-agent", UA)
            .header("x-forwarded-for", ip);
        if let Some(c) = country {
            r = r.header("cf-ipcountry", c);
        }
        r.body(Body::from(batch(vec![ev("$pageview", 0)]).to_string()))
            .unwrap()
    };
    assert_eq!(
        h.send(post("203.0.113.9, 10.0.0.1", None)).await.status(),
        204
    );
    let mut ev2 = ev("$pageview", 1);
    ev2["seq"] = json!(1);
    let r = Request::post("/collect?k=pk_test")
        .header("origin", "https://example.com")
        .header("user-agent", UA)
        .header("x-forwarded-for", "203.0.113.77")
        .header("cf-ipcountry", "us")
        .body(Body::from(batch(vec![ev2]).to_string()))
        .unwrap();
    assert_eq!(h.send(r).await.status(), 204);

    let mut pos = Pos { seg: 1, off: 0 };
    let mut countries = vec![];
    while let Read_::Batch(b, next) = spool::read_at(h.dir.path(), pos).unwrap() {
        countries.extend(b.events.iter().map(|e| e.country.clone()));
        pos = next;
    }
    assert_eq!(
        countries,
        vec!["SG", "US"],
        "GeoIP when no header, trusted header wins"
    );
    let raw = std::fs::read(spool::seg_path(h.dir.path(), 1)).unwrap();
    let text = String::from_utf8_lossy(&raw);
    assert!(!text.contains("203.0.113"), "IP must not reach the spool");
}

#[test]
fn ac_0_9_4_d_private_loopback_and_garbage_addresses_have_no_country() {
    let g = greprum::geoip::GeoIp::open(MMDB).unwrap();
    for ip in [
        "10.1.2.3",
        "192.168.0.5",
        "127.0.0.1",
        "169.254.1.1",
        "::1",
        "fd00::1",
    ] {
        assert_eq!(g.country(ip.parse().unwrap()), "", "{ip}");
    }
    assert!(greprum::geoip::first_ip("not-an-ip").is_none());
    assert_eq!(
        greprum::geoip::first_ip(" 198.51.100.1 , 10.0.0.1")
            .unwrap()
            .to_string(),
        "198.51.100.1"
    );
}

// ---- AC0.9.5 token lifetime cap ----------------------------------------------

fn token_exp(secret: &str, secs_from_now: i64) -> String {
    let real = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &json!({"sub": "u_9", "exp": real + secs_from_now}),
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap()
}

fn identified(site: &greprum::config::SiteConfig, token: &str) -> bool {
    let mut b = batch(vec![ev("$identify", 0)]);
    b["token"] = json!(token);
    collect::process(b.to_string().as_bytes(), site, &req(), &cfg(), NOW)
        .unwrap()
        .events[0]
        .is_identified
}

#[test]
fn ac_0_9_5_a_exp_within_the_cap_is_accepted() {
    let mut s = site();
    s.max_token_ttl_secs = 600;
    assert!(identified(&s, &token_exp("secret", 300)));
    assert!(identified(&s, &token_exp("secret", 590)));
}

#[test]
fn ac_0_9_5_b_exp_beyond_the_cap_is_stored_anonymous() {
    let mut s = site();
    s.max_token_ttl_secs = 600;
    assert!(!identified(&s, &token_exp("secret", 3600)));
    assert!(
        !identified(&s, &token_exp("secret", 365 * 86_400)),
        "a year-long token is refused"
    );
}

#[test]
fn ac_0_9_5_c_default_cap_is_one_hour_when_unset() {
    let c = Config::parse("[[site]]\nid='a'\nkey='pk_a'\nhmac_secret='secret'").unwrap();
    assert_eq!(c.sites[0].max_token_ttl_secs, 3600);
    assert!(identified(&c.sites[0], &token_exp("secret", 3000)));
    assert!(!identified(&c.sites[0], &token_exp("secret", 7200)));
    assert!(Config::parse("[[site]]\nid='a'\nkey='pk_a'\nmax_token_ttl_secs=0").is_err());
}
