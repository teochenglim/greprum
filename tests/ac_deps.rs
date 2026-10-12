//! AC0.11.x: dependency upgrades taken deliberately, and the ones intentionally not taken.
use std::{fs, path::Path};

fn read(p: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(p))
        .unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn dep_line(toml: &str, name: &str) -> String {
    toml.lines()
        .find(|l| l.starts_with(&format!("{name} ")))
        .unwrap_or_else(|| panic!("{name} missing"))
        .to_string()
}

fn locked_version(name: &str) -> Option<String> {
    let lock = read("Cargo.lock");
    let mut it = lock.lines();
    while let Some(l) = it.next() {
        if l == format!("name = \"{name}\"") {
            return it.next().map(|v| {
                v.trim_start_matches("version = ")
                    .trim_matches('"')
                    .to_string()
            });
        }
    }
    None
}

// ---- AC0.11.1 axum 0.8, tower-http gone ------------------------------------------

#[test]
fn ac_0_11_1_a_axum_is_0_8_and_the_unused_tower_http_dependency_is_removed() {
    let c = read("Cargo.toml");
    assert!(
        dep_line(&c, "axum").contains("\"0.8"),
        "{}",
        dep_line(&c, "axum")
    );
    assert!(
        !c.contains("tower-http"),
        "tower-http was never used; removed instead of upgraded"
    );
    assert!(locked_version("axum").is_some_and(|v| v.starts_with("0.8.")));
    assert!(locked_version("tower-http").is_none());
}

#[test]
fn ac_0_11_1_b_routes_use_the_native_brace_path_syntax() {
    let q = read("src/query/mod.rs");
    assert!(
        !q.contains(".replace(\"{id}\""),
        "axum 0.8 takes `{{id}}` directly"
    );
    assert!(q.contains("(\"/api/v1/sessions/{id}\""));
}

// ---- AC0.11.2 sqlx 0.9 (rsa gone) ------------------------------------------------

#[test]
fn ac_0_11_2_a_sqlx_is_0_9_and_dynamic_sql_is_explicitly_audited() {
    let c = read("Cargo.toml");
    assert!(
        dep_line(&c, "sqlx").contains("\"0.9"),
        "{}",
        dep_line(&c, "sqlx")
    );
    assert!(locked_version("sqlx").is_some_and(|v| v.starts_with("0.9.")));
    for f in [
        "src/query/reader.rs",
        "src/jobs/mod.rs",
        "src/migrate/mod.rs",
    ] {
        assert!(
            read(f).contains("AssertSqlSafe"),
            "{f}: sqlx 0.9 needs the audited-SQL marker"
        );
    }
}

#[test]
fn ac_0_11_2_b_the_rsa_crate_is_not_in_the_dependency_tree() {
    assert!(
        locked_version("rsa").is_none(),
        "rsa (RUSTSEC-2023-0071) must stay out of Cargo.lock"
    );
    assert!(locked_version("sqlx-mysql").is_some());
}

// ---- AC0.11.3 jsonwebtoken stays on 9.x ------------------------------------------

#[test]
fn ac_0_11_3_a_jsonwebtoken_is_pinned_to_9_with_an_accepted_adr() {
    let c = read("Cargo.toml");
    assert!(dep_line(&c, "jsonwebtoken").contains("\"9\""));
    assert!(locked_version("jsonwebtoken").is_some_and(|v| v.starts_with("9.")));
    let adr = read("docs/adr/0052-jsonwebtoken-stays-on-9x.md");
    assert!(adr.contains("Status: Accepted") && adr.contains("rsa"));
}

#[test]
fn ac_0_11_3_b_hs256_pinning_tests_still_exist_for_the_token_path() {
    let t = read("src/collect/identity.rs");
    assert!(t.contains("Algorithm::HS256") && t.contains("alg_none_is_refused"));
    assert!(t.contains("max_ttl_secs"));
}

// ---- AC0.11.4 tracker toolchain --------------------------------------------------

#[test]
fn ac_0_11_4_a_tracker_dev_dependencies_are_current_majors() {
    let p: serde_json::Value = serde_json::from_str(&read("tracker/package.json")).unwrap();
    let d = &p["devDependencies"];
    assert!(d["esbuild"].as_str().unwrap().starts_with("^0.28"));
    assert!(d["jsdom"].as_str().unwrap().starts_with("^30"));
    assert!(d["playwright-core"].as_str().unwrap().starts_with("^1.6"));
}

#[test]
fn ac_0_11_4_b_size_budgets_are_unchanged() {
    let s = read("tracker/size.mjs");
    assert!(s.contains("core > 3072") && s.contains("total > 5120"));
}

// ---- AC0.11.5 remaining crates, audit clean --------------------------------------

#[test]
fn ac_0_11_5_a_no_advisory_is_ignored_anywhere() {
    let d = read("deny.toml");
    assert!(!d.contains("RUSTSEC-"), "deny.toml carries no ignores");
    assert!(
        !Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".cargo/audit.toml")
            .exists()
    );
    assert!(!read(".github/workflows/ci.yml").contains("ignore:"));
}

#[test]
fn ac_0_11_5_b_audit_and_deny_still_run_in_ci() {
    let ci = read(".github/workflows/ci.yml");
    assert!(ci.contains("rustsec/audit-check") && ci.contains("cargo-deny-action"));
}

// ---- AC0.11.6 action majors ------------------------------------------------------

#[test]
fn ac_0_11_6_a_action_majors_are_taken_and_still_sha_pinned() {
    let mut majors = std::collections::HashMap::new();
    for f in ["ci.yml", "release.yml", "docker.yml"] {
        for l in read(&format!(".github/workflows/{f}"))
            .lines()
            .filter(|l| l.contains("uses:"))
        {
            let (spec, tag) = l.split_once('#').expect("tag comment");
            let name = spec
                .split("uses:")
                .nth(1)
                .unwrap()
                .trim()
                .split('@')
                .next()
                .unwrap()
                .to_string();
            let sha = spec.split('@').nth(1).unwrap().trim();
            assert!(
                sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
                "{l}"
            );
            majors.insert(
                name,
                tag.trim()
                    .trim_start_matches('v')
                    .split('.')
                    .next()
                    .unwrap()
                    .parse::<u32>()
                    .unwrap_or(0),
            );
        }
    }
    assert!(majors["actions/checkout"] >= 6, "{majors:?}");
    assert!(majors["docker/build-push-action"] >= 7, "{majors:?}");
    assert!(
        majors["actions/download-artifact"] >= 7 && majors["actions/upload-artifact"] >= 6,
        "{majors:?}"
    );
}

#[test]
fn ac_0_11_6_b_pin_script_can_move_to_the_newest_major_on_request() {
    let s = read("scripts/pin-actions.py");
    assert!(s.contains("MAJOR") && s.contains("newest_any_major"));
}
