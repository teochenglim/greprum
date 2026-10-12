//! AC0.7.x: container + registry release are checked statically (no docker daemon needed)
//! plus one live probe of `greprum::health`.
mod common;
use std::{fs, path::Path};

fn read(p: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(p))
        .unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn final_stage(dockerfile: &str) -> String {
    let i = dockerfile.rfind("\nFROM ").expect("FROM") + 1;
    dockerfile[i..].to_string()
}

// ---- AC0.7.1 Dockerfile -----------------------------------------------------

#[test]
fn ac_0_7_1_a_multi_stage_and_final_image_is_distroless_nonroot() {
    let d = read("Dockerfile");
    assert!(
        d.matches("\nFROM ").count() + d.starts_with("FROM") as usize >= 3,
        "tracker, build, runtime stages"
    );
    let last = final_stage(&d);
    assert!(last.starts_with("FROM gcr.io/distroless/"), "{last}");
    assert!(
        last.lines()
            .any(|l| l.trim() == "USER nonroot" || l.trim() == "USER 65532")
    );
}

#[test]
fn ac_0_7_1_b_spool_lives_on_a_declared_volume_owned_by_the_runtime_user() {
    let last = final_stage(&read("Dockerfile"));
    assert!(last.contains("VOLUME [\"/data\"]"));
    assert!(last.contains("GREPRUM_SPOOL_DIR=/data/spool"));
    assert!(last.contains("--chown=65532:65532") && last.contains("/data"));
}

#[test]
fn ac_0_7_1_c_healthcheck_uses_the_binary_not_curl() {
    let last = final_stage(&read("Dockerfile"));
    let h = last
        .lines()
        .skip_while(|l| !l.starts_with("HEALTHCHECK"))
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(h.contains("greprum") && h.contains("healthcheck"), "{h}");
    assert!(!last.contains("curl") && !last.contains("wget"));
}

#[test]
fn ac_0_7_1_d_exposes_8080_entrypoint_serve_and_builds_locked_with_version_arg() {
    let d = read("Dockerfile");
    assert!(d.contains("EXPOSE 8080"));
    assert!(d.contains("ENTRYPOINT [\"/usr/local/bin/greprum\"]"));
    assert!(d.contains("\"serve\", \"--config\""));
    assert!(d.contains("cargo build --release --locked"));
    assert!(d.contains("ARG VERSION") && d.contains("GREPRUM_VERSION=${VERSION}"));
    assert!(
        d.contains("node tracker/size.mjs"),
        "size budget enforced inside the image build"
    );
}

#[test]
fn ac_0_7_1_e_dockerignore_keeps_build_context_small_and_clean() {
    let i = read(".dockerignore");
    for must in ["target", ".git", "node_modules", "spool", "*.wal"] {
        assert!(i.lines().any(|l| l.trim() == must), "{must}");
    }
}

// ---- AC0.7.2 GHCR publish workflow ------------------------------------------

#[test]
fn ac_0_7_2_a_triggers_on_version_tags_only_plus_manual() {
    let w = read(".github/workflows/docker.yml");
    assert!(w.contains("tags:") && w.contains("'v[0-9]*'"));
    assert!(w.contains("workflow_dispatch"));
    assert!(!w.contains("branches:"), "never publish on branch pushes");
}

#[test]
fn ac_0_7_2_b_logs_in_to_ghcr_with_github_token_and_least_privilege() {
    let w = read(".github/workflows/docker.yml");
    assert!(w.contains("registry: ghcr.io"));
    assert!(w.contains("secrets.GITHUB_TOKEN"));
    assert!(w.contains("packages: write") && w.contains("contents: read"));
    assert!(
        !w.contains("secrets.GHCR") && !w.contains("PAT"),
        "no personal access token"
    );
}

#[test]
fn ac_0_7_2_c_multi_arch_with_semver_and_latest_tags() {
    let w = read(".github/workflows/docker.yml");
    assert!(w.contains("linux/amd64") && w.contains("linux/arm64"));
    assert!(w.contains("type=semver,pattern={{version}}"));
    assert!(w.contains("type=semver,pattern={{major}}.{{minor}}"));
    assert!(w.contains("value=latest"));
    assert!(w.contains("push: true"));
    assert!(w.contains("ghcr.io/${{ github.repository }}"));
}

#[test]
fn ac_0_7_2_d_tag_must_equal_version_file_and_passes_version_build_arg() {
    let w = read(".github/workflows/docker.yml");
    assert!(w.contains("test \"$TAG\" = \"v$(cat VERSION)\""));
    assert!(w.contains("VERSION=${{ github.ref_name }}"));
    // tag is passed through env, never interpolated into the script (shell injection)
    assert!(!w.contains("run: test \"${{ github.ref_name }}\""));
}

// ---- AC0.7.3 runtime wiring (env overrides: see config tests) ----------------

#[test]
fn ac_0_7_3_c_probe_addr_rewrites_wildcard_to_loopback() {
    assert_eq!(
        greprum::health::probe_addr("0.0.0.0:8080"),
        "127.0.0.1:8080"
    );
    assert_eq!(
        greprum::health::probe_addr("127.0.0.1:9000"),
        "127.0.0.1:9000"
    );
    assert_eq!(greprum::health::probe_addr("[::]:8080"), "127.0.0.1:8080");
}

#[tokio::test]
async fn ac_0_7_3_d_healthcheck_ok_against_live_gateway_and_fails_when_down() {
    use common::app::harness;
    let h = harness();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let app = h.router.clone();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    greprum::health::check(&addr).await.expect("healthy");
    // A closed port fails fast.
    let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_addr = dead.local_addr().unwrap().to_string();
    drop(dead);
    assert!(greprum::health::check(&dead_addr).await.is_err());
}

// ---- AC0.7.4 compose, Makefile, default container config ----------------------

#[test]
fn ac_0_7_4_a_compose_pairs_pinned_greptimedb_with_the_published_image_and_volumes() {
    let c = read("docker-compose.yml");
    assert!(c.contains("greptime/greptimedb:v1.2.1"));
    assert!(c.contains("ghcr.io/teochenglim/greprum"));
    assert!(c.contains("greprum-data:/data"));
    assert!(
        c.contains("127.0.0.1:${GREPRUM_PORT:-8080}:8080"),
        "published on loopback by default"
    );
}

#[test]
fn ac_0_7_4_b_makefile_has_docker_targets_using_the_version_file() {
    let m = read("Makefile");
    for t in ["docker-build:", "docker-run:", "docker-push:"] {
        assert!(m.contains(t), "{t}");
    }
    assert!(m.contains("--build-arg VERSION=v$(VERSION_CURRENT)"));
}

#[test]
fn ac_0_7_4_c_default_container_config_parses_and_binds_inside_the_container() {
    let c = greprum::config::Config::parse(&read("docker/greprum.toml")).expect("valid config");
    assert_eq!(c.server.listen, "0.0.0.0:8080");
    assert_eq!(c.spool.dir, "/data/spool");
    assert!(c.sites.is_empty(), "no secrets baked into the image");
}

// ---- AC0.7.5 naming consistency & repo wiring --------------------------------

#[test]
fn ac_0_7_5_a_image_name_is_the_same_everywhere() {
    let want = "ghcr.io/teochenglim/greprum";
    assert!(read("Makefile").contains(&format!("IMAGE ?= {want}")));
    assert!(read("docker-compose.yml").contains(want));
    assert!(read("Dockerfile").contains("https://github.com/teochenglim/greprum"));
    assert!(read("RELEASE/v0.7.0.md").contains(want));
}

#[test]
fn ac_0_7_5_b_agents_md_keeps_the_user_owns_git_rule() {
    let a = read("AGENTS.md");
    assert!(a.contains("never runs git commit/push/tag"));
    assert!(a.contains("make release VERSION=x.y.z"));
}

// ---- regression: the first tag build failed on a clean checkout (folded into v0.8.0) -----

#[test]
fn ci_regression_tracker_target_installs_locked_node_dependencies() {
    let m = read("Makefile");
    assert!(m.contains("tracker/node_modules: tracker/package.json tracker/package-lock.json"));
    assert!(m.contains("npm ci"));
    assert!(
        m.contains("tracker: tracker/node_modules"),
        "build depends on installed deps"
    );
    assert!(!m.contains("npm install"), "reproducible installs only");
    let r = read(".github/workflows/release.yml");
    assert!(
        r.contains("npm ci"),
        "release build installs the tracker deps"
    );
    assert!(!r.contains("macos-13"), "retired runner");
    // Node 22 (CI) does not accept a directory for `--test`; globs work on every version.
    assert!(
        m.contains("node --test test/*.test.mjs"),
        "tracker tests by glob"
    );
    assert!(
        m.contains("node --test browser/*.test.mjs"),
        "browser tests by glob"
    );
    assert!(!m.contains("node --test test/\n") && !m.contains("node --test browser/\n"));
}
