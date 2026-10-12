//! AC0.12.x: a new user reaches working dashboards with one command, and the tutorials stay true.
//! Static checks over the repository files plus the real binary (no database needed).
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn read(p: &str) -> String {
    fs::read_to_string(root().join(p)).unwrap_or_else(|e| panic!("{p}: {e}"))
}
fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_greprum"))
}
fn tutorials() -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(root().join("tutorials"))
        .expect("tutorials/")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md"))
        .collect();
    v.sort();
    v
}

/// `(link target)` of every markdown link / image in `md`, fragments and queries removed.
fn links(md: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = md;
    while let Some(i) = rest.find("](") {
        rest = &rest[i + 2..];
        if let Some(j) = rest.find(')') {
            out.push(rest[..j].to_string());
            rest = &rest[j..];
        }
    }
    out.into_iter()
        .map(|l| l.split('#').next().unwrap_or("").to_string())
        .filter(|l| !l.is_empty() && !l.starts_with("http") && !l.starts_with("mailto:"))
        .collect()
}

// ---- AC0.12.1 zero-edit compose -------------------------------------------------------

#[test]
fn ac_0_12_1_a_compose_runs_migrate_before_the_gateway_with_a_committed_demo_config() {
    let c = read("docker-compose.yml");
    assert!(c.contains("${GREPRUM_CONFIG:-./docker/quickstart.toml}"));
    assert!(c.contains("\n  migrate:"), "one-shot migrate service");
    assert!(c.contains("\"--wait\"") && c.contains("restart: \"no\""));
    assert!(
        c.contains("condition: service_completed_successfully"),
        "greprum starts only after migrate"
    );
    assert!(
        !c.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .any(|l| l.contains("./greprum.toml")),
        "no file to create before `docker compose up`"
    );
    assert!(Path::new(&root().join("docker/quickstart.toml")).exists());
}

#[test]
fn ac_0_12_1_b_demo_config_parses_and_its_api_key_hash_matches_the_documented_key() {
    let cfg = greprum::config::Config::parse(&read("docker/quickstart.toml")).expect("valid");
    let site = cfg.site("demo").expect("site demo");
    assert_eq!(site.key, "pk_demo");
    assert!(site.origins.iter().any(|o| o == "http://localhost:8000"));
    assert_eq!(cfg.api_keys.len(), 1);
    assert_eq!(
        cfg.api_keys[0].sha256,
        greprum::query::hash_key("demo-api-key"),
        "tutorials say `Bearer demo-api-key`"
    );
    assert_eq!(cfg.api_keys[0].sites, vec!["demo".to_string()]);
    assert!(read("docker/quickstart.toml").contains("DEMO configuration"));
}

#[test]
fn ac_0_12_1_c_migrate_wait_retries_while_the_database_is_down_then_gives_up() {
    // Nothing listens on 127.0.0.1:1.
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("c.toml");
    fs::write(
        &cfg,
        "[greptime]\nwrite_url = \"mysql://root@127.0.0.1:1/public\"\n",
    )
    .unwrap();
    let fast = bin()
        .args(["migrate", "--config"])
        .arg(&cfg)
        .output()
        .unwrap();
    assert!(!fast.status.success());
    assert!(
        !String::from_utf8_lossy(&fast.stderr).contains("retrying"),
        "no retry without --wait"
    );
    let t = Instant::now();
    let waited = bin()
        .args(["migrate", "--wait", "3", "--config"])
        .arg(&cfg)
        .output()
        .unwrap();
    assert!(!waited.status.success(), "still fails after the wait");
    assert!(
        t.elapsed() >= Duration::from_secs(3),
        "retried for the whole wait, took {:?}",
        t.elapsed()
    );
    assert!(String::from_utf8_lossy(&waited.stderr).contains("retrying"));
}

// ---- AC0.12.2 demo data -------------------------------------------------------------

#[test]
fn ac_0_12_2_a_seed_service_is_an_opt_in_profile_with_a_fixed_seed() {
    let c = read("docker-compose.yml");
    let seed = &c[c.find("\n  seed:").expect("seed service")..];
    let seed = &seed[..seed.find("\nvolumes:").unwrap_or(seed.len())];
    assert!(
        seed.contains("profiles: [\"demo\"]"),
        "not started by plain `up`"
    );
    assert!(seed.contains("\"gen\"") && seed.contains("\"--site\", \"demo\""));
    assert!(seed.contains("\"--seed\", \"7\""));
    assert!(seed.contains("service_completed_successfully"));
}

#[test]
fn ac_0_12_2_b_gen_is_deterministic_for_the_documented_seed() {
    let run = || {
        let o = bin()
            .args([
                "gen", "--seed", "7", "--site", "demo", "--events", "300", "--days", "1",
            ])
            .output()
            .unwrap();
        assert!(o.status.success());
        // `sent_at` anchors to "now"; compare the shape: events per batch, names, sequence.
        String::from_utf8(o.stdout)
            .unwrap()
            .lines()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).unwrap();
                let names: Vec<String> = v["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| format!("{}:{}", e["name"], e["seq"]))
                    .collect();
                names.join(",")
            })
            .collect::<Vec<_>>()
    };
    let (a, b) = (run(), run());
    assert!(a.len() > 10);
    assert_eq!(a, b);
}

#[test]
fn ac_0_12_2_c_make_demo_targets_exist_and_use_the_profile() {
    let m = read("Makefile");
    assert!(m.contains("\ndemo: "));
    assert!(m.contains("--profile demo run --rm seed"));
    assert!(m.contains("\ndemo-local: docker-build"));
    assert!(m.contains("GREPRUM_IMAGE=$(IMAGE):$(VERSION_CURRENT)"));
}

// ---- AC0.12.3 tutorials with screenshots ------------------------------------------------

#[test]
fn ac_0_12_3_a_five_numbered_tutorials_are_indexed_in_order() {
    let names = tutorials();
    let numbered: Vec<&String> = names
        .iter()
        .filter(|n| n[..2].chars().all(|c| c.is_ascii_digit()))
        .collect();
    assert_eq!(numbered.len(), 5, "{names:?}");
    for (i, n) in numbered.iter().enumerate() {
        assert!(n.starts_with(&format!("0{}-", i + 1)), "{n}");
    }
    let idx = read("tutorials/README.md");
    let mut at = 0;
    for n in numbered {
        let p = idx
            .find(n.as_str())
            .unwrap_or_else(|| panic!("index misses {n}"));
        assert!(p >= at, "{n} out of order");
        at = p;
    }
    assert!(
        read("README.md").contains("tutorials/"),
        "README links the tutorials"
    );
}

#[test]
fn ac_0_12_3_b_every_relative_link_and_image_resolves() {
    let mut checked = 0;
    for f in tutorials() {
        for l in links(&read(&format!("tutorials/{f}"))) {
            let target = root().join("tutorials").join(&l);
            assert!(target.exists(), "tutorials/{f}: broken link {l}");
            checked += 1;
        }
    }
    assert!(checked > 25, "only {checked} links checked");
}

#[test]
fn ac_0_12_3_c_every_dashboard_has_a_screenshot_shown_in_the_tutorials() {
    let all: String = tutorials()
        .iter()
        .map(|f| read(&format!("tutorials/{f}")))
        .collect();
    let shown: HashSet<String> = links(&all)
        .into_iter()
        .filter(|l| l.contains("images/grafana_"))
        .map(|l| l.rsplit('/').next().unwrap().to_string())
        .collect();
    let shots: HashSet<String> = fs::read_dir(root().join("images"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("grafana_") && n.ends_with(".png"))
        .collect();
    assert_eq!(shots.len(), 11);
    assert_eq!(
        shown, shots,
        "tutorials show exactly the dashboard screenshots"
    );
    let dashboards = fs::read_dir(root().join("ui/grafana/dashboards"))
        .unwrap()
        .count();
    assert_eq!(dashboards, shots.len(), "one screenshot per dashboard");
}

#[test]
fn ac_0_12_3_d_commands_in_tutorials_name_real_targets_services_and_keys() {
    let all: String = tutorials()
        .iter()
        .map(|f| read(&format!("tutorials/{f}")))
        .collect();
    let mk = read("Makefile");
    for line in all.lines().filter(|l| l.contains("make ")) {
        for w in line.split("make ").skip(1) {
            let t: String = w
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            if !t.is_empty() && t.len() > 2 && !line.contains("`make` +") {
                assert!(
                    mk.contains(&format!("\n{t}:")),
                    "tutorial mentions `make {t}` which the Makefile lacks: {line}"
                );
            }
        }
    }
    let compose = read("docker-compose.yml");
    for svc in [
        "greptime",
        "migrate",
        "greprum",
        "prometheus",
        "grafana",
        "seed",
    ] {
        assert!(compose.contains(&format!("\n  {svc}:")), "{svc}");
    }
    assert!(all.contains("demo-api-key") && all.contains("pk_demo"));
    for var in [
        "GREPRUM_PORT",
        "GRAFANA_PORT",
        "GREPTIME_HTTP_PORT",
        "GREPTIME_MYSQL_PORT",
        "PROMETHEUS_PORT",
        "GREPRUM_CONFIG",
    ] {
        assert!(compose.contains(var), "{var} documented but not in compose");
    }
    let site = read("docker/demo-site/index.html");
    assert!(site.contains("data-site=\"pk_demo\"") && site.contains(":8080/t.js"));
}

// ---- AC0.12.4 schema reference stays true ----------------------------------------------

#[test]
fn ac_0_12_4_a_tutorial_lists_every_events_column_and_every_rollup_table() {
    let t = read("tutorials/04-your-own-tools.md");
    for c in greprum::migrate::EVENT_COLUMNS {
        assert!(
            t.contains(&format!("`{c}`")),
            "events column `{c}` undocumented"
        );
    }
    for tbl in greprum::migrate::ROLLUP_TABLES {
        assert!(
            t.contains(&format!("`{tbl}`")),
            "table `{tbl}` undocumented"
        );
    }
    for tbl in ["first_seen", "sessions", "events"] {
        assert!(t.contains(&format!("`{tbl}`")), "{tbl}");
    }
}

#[test]
fn ac_0_12_4_b_example_sql_filters_on_site_and_quotes_reserved_words() {
    let t = read("tutorials/04-your-own-tools.md");
    let sql: Vec<&str> = t
        .split("```sql")
        .skip(1)
        .map(|b| b.split("```").next().unwrap())
        .collect();
    let mut seen = 0;
    for b in sql {
        for stmt in b.split(';').filter(|s| s.contains("FROM")) {
            seen += 1;
            assert!(stmt.contains("site_id ="), "no site filter: {stmt}");
            assert!(
                !stmt.contains(" path ") && !stmt.contains("(path)"),
                "unquoted reserved word: {stmt}"
            );
        }
    }
    assert!(seen >= 2, "{seen} example statements");
}

// ---- AC0.12.5 split container build --------------------------------------------------

#[test]
fn ac_0_12_5_a_each_architecture_builds_natively_and_pushes_by_digest() {
    let w = read(".github/workflows/docker.yml");
    assert!(
        !w.contains("setup-qemu-action"),
        "emulated arm64 never finished"
    );
    assert!(w.contains("runner: ubuntu-latest") && w.contains("runner: ubuntu-24.04-arm"));
    assert!(w.contains("platform: linux/amd64") && w.contains("platform: linux/arm64"));
    assert!(w.contains("push-by-digest=true"));
    assert!(!w.contains("platforms: linux/amd64,linux/arm64"));
}

#[test]
fn ac_0_12_5_b_a_merge_job_combines_digests_and_every_job_has_a_timeout() {
    let w = read(".github/workflows/docker.yml");
    assert!(w.contains("\n  merge:") && w.contains("needs: build"));
    assert!(w.contains("imagetools create"));
    assert!(w.contains("-eq 2"), "merge insists on both architectures");
    assert!(
        w.matches("timeout-minutes:").count() >= 2,
        "a hung build must not run for hours"
    );
    assert!(
        w.contains("test \"$TAG\" = \"v$(cat VERSION)\""),
        "tag matches VERSION"
    );
}
