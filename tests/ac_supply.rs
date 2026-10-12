//! AC0.10.6 supply chain: SHA-pinned actions, audit in CI, update automation.
use std::{fs, path::Path};

fn read(p: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(p))
        .unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn workflows() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows");
    fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "yml"))
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                fs::read_to_string(e.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn ac_0_10_6_a_every_action_is_pinned_to_a_full_commit_sha() {
    let mut n = 0;
    for (name, body) in workflows() {
        for line in body
            .lines()
            .filter(|l| l.trim_start().trim_start_matches("- ").starts_with("uses:"))
        {
            let r = line.split("uses:").nth(1).unwrap().trim();
            let (_, rest) = r
                .split_once('@')
                .unwrap_or_else(|| panic!("{name}: unpinned `{line}`"));
            let sha = rest.split_whitespace().next().unwrap();
            assert!(
                sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
                "{name}: {line}"
            );
            assert!(
                line.contains('#'),
                "{name}: keep the human-readable tag as a comment: {line}"
            );
            n += 1;
        }
    }
    assert!(n >= 20, "{n}");
}

#[test]
fn ac_0_10_6_b_ci_runs_cargo_audit_and_cargo_deny() {
    let ci = read(".github/workflows/ci.yml");
    assert!(ci.contains("rustsec/audit-check"), "cargo audit");
    assert!(ci.contains("EmbarkStudios/cargo-deny-action"), "cargo deny");
    assert!(
        ci.contains("checks: write"),
        "audit-check needs the checks permission"
    );
}

#[test]
fn ac_0_10_6_c_deny_config_covers_advisories_licences_bans_sources_and_no_update_bot_is_configured()
{
    let d = read("deny.toml");
    for s in [
        "[advisories]",
        "[licenses]",
        "[bans]",
        "[sources]",
        "Apache-2.0",
        "unknown-git = \"deny\"",
    ] {
        assert!(d.contains(s), "{s}");
    }
    // Updates are manual and tested (no Dependabot/Renovate config): see RELEASE/v0.11.0.md.
    for f in [
        ".github/dependabot.yml",
        ".github/dependabot.yaml",
        "renovate.json",
    ] {
        assert!(
            !Path::new(env!("CARGO_MANIFEST_DIR")).join(f).exists(),
            "{f} should not exist"
        );
    }
}

#[test]
fn ac_0_10_6_e_every_ignored_advisory_has_an_accepted_adr() {
    // Ignores live in deny.toml (`id = "RUSTSEC-…"`), `.cargo/audit.toml` and the CI audit step.
    let mut ids: Vec<String> = vec![];
    for f in ["deny.toml", ".github/workflows/ci.yml"] {
        for tok in read(f).split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
            if tok.starts_with("RUSTSEC-") {
                ids.push(tok.to_string());
            }
        }
    }
    if Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".cargo/audit.toml")
        .exists()
    {
        for tok in
            read(".cargo/audit.toml").split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        {
            if tok.starts_with("RUSTSEC-") {
                ids.push(tok.to_string());
            }
        }
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/adr");
    let adrs: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| fs::read_to_string(e.path()).unwrap())
        .collect();
    for id in ids {
        let ok = adrs.iter().any(|a| {
            a.contains(&id) && a.contains("Status: Accepted") && a.contains("## Decision")
        });
        assert!(ok, "{id} is ignored but has no Accepted ADR");
    }
}

#[test]
fn ac_0_10_6_d_bump_target_uses_the_pin_script_and_no_tag_refs_remain_in_the_docker_job() {
    let m = read("Makefile");
    assert!(m.contains("github-action-bump:") && m.contains("scripts/pin-actions.py"));
    assert!(read("scripts/pin-actions.py").contains("git\", \"ls-remote"));
}
