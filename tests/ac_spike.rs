//! AC0.1.2 generator, AC0.1.3 harness maths, AC0.1.4/0.1.5 ADR files.
mod common;
use common::*;
use greprum::{collect, model::BUILTIN_EVENTS, spike, synth::Generator};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

fn gen_events(seed: u64, sessions: usize) -> Vec<greprum::model::StoredEvent> {
    let mut g = Generator::new(seed, NOW - 7 * 86_400_000, 6 * 86_400_000);
    let s = site();
    let mut out = vec![];
    for _ in 0..sessions {
        let sess = g.session();
        let info = collect::RequestInfo {
            user_agent: sess.user_agent.into(),
            country: sess.country.into(),
            dnt: false,
        };
        for b in sess.batches {
            let received = b["sent_at"].as_i64().unwrap();
            let p = collect::process(b.to_string().as_bytes(), &s, &info, &cfg(), received)
                .expect("valid batch");
            assert_eq!(
                p.dropped_count(),
                0,
                "generator must not produce droppable events: {:?}",
                p.dropped
            );
            out.extend(p.events);
        }
    }
    out
}

// ---- AC0.1.2 generator ------------------------------------------------------

#[test]
fn ac_0_1_2_a_same_seed_same_output_different_seed_differs() {
    assert_eq!(gen_events(7, 30), gen_events(7, 30));
    assert_ne!(gen_events(7, 30), gen_events(8, 30));
}

#[test]
fn ac_0_1_2_b_every_event_passes_gateway_validation_with_no_drops() {
    let evs = gen_events(1, 300);
    assert!(evs.len() > 1000);
    assert!(
        evs.iter()
            .all(|e| !e.visitor_id.is_empty() && !e.session_id.is_empty() && e.sample_rate == 1.0)
    );
}

#[test]
fn ac_0_1_2_c_all_builtin_events_and_a_custom_event_present() {
    let names: HashSet<_> = gen_events(1, 400)
        .into_iter()
        .map(|e| e.event_name)
        .collect();
    for b in BUILTIN_EVENTS {
        assert!(names.contains(*b), "generator never emits {b}");
    }
    assert!(names.contains("signup"));
}

#[test]
fn ac_0_1_2_d_seq_contiguous_from_zero_per_session_and_session_start_first() {
    let mut by: HashMap<String, Vec<(i32, String)>> = HashMap::new();
    for e in gen_events(3, 100) {
        by.entry(e.session_id)
            .or_default()
            .push((e.seq, e.event_name));
    }
    for (sid, mut v) in by {
        v.sort();
        for (i, (seq, _)) in v.iter().enumerate() {
            assert_eq!(*seq as usize, i, "gap in {sid}");
        }
        assert_eq!(v[0].1, "$session_start");
    }
}

// ---- AC0.1.3 spike harness --------------------------------------------------

#[test]
fn ac_0_1_3_a_percentile_nearest_rank() {
    let s: Vec<f64> = (1..=100).map(|x| x as f64).collect();
    assert_eq!(spike::percentile(&s, 95.0), 95.0);
    assert_eq!(spike::percentile(&s, 50.0), 50.0);
    assert_eq!(spike::percentile(&s, 100.0), 100.0);
    assert_eq!(spike::percentile(&[], 95.0), 0.0);
    assert_eq!(spike::percentile(&[5.0], 95.0), 5.0);
}

#[test]
fn ac_0_1_3_b_gates_pass_fail_and_summary_job_adoption() {
    let ok = spike::evaluate("rollup_overview", &[100.0, 200.0, 1500.0]).unwrap();
    assert!(ok.pass && !ok.adopt_summary_job);
    let slow_rollup = spike::evaluate("rollup_overview", &[100.0, 3000.0]).unwrap();
    assert!(
        !slow_rollup.pass && !slow_rollup.adopt_summary_job,
        "rollups are not fixed by a job"
    );
    let slow_funnel = spike::evaluate("funnel", &[12_000.0]).unwrap();
    assert!(!slow_funnel.pass && slow_funnel.adopt_summary_job);
    assert!(spike::evaluate("nonsense", &[1.0]).is_none());
}

#[test]
fn ac_0_1_3_c_gate_table_matches_prd_limits() {
    let lim = |n: &str| {
        spike::GATES
            .iter()
            .find(|g| g.name == n)
            .unwrap()
            .p95_limit_ms
    };
    assert_eq!(lim("rollup_overview"), 2_000.0);
    for n in ["funnel", "retention", "sessions_24h", "count_distinct_raw"] {
        assert_eq!(lim(n), 10_000.0);
    }
}

// ---- AC0.1.4 / 0.1.5 ADR files ----------------------------------------------

fn adrs() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/adr");
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".md"))
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read_to_string(e.path()).unwrap(),
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn ac_0_1_4_a_eleven_spike_items_each_have_an_adr() {
    let all = adrs();
    for item in 1..=11 {
        let tag = format!("spike-item-{item:02}");
        assert!(
            all.iter().any(|(_, body)| body.contains(&tag)),
            "no ADR answers {tag}"
        );
    }
}

#[test]
fn ac_0_1_4_b_adrs_have_status_context_decision_consequences() {
    for (name, body) in adrs() {
        assert!(
            body.lines().any(|l| l.starts_with("Status:")),
            "{name} lacks a `Status:` line"
        );
        for h in ["Context", "Decision", "Consequences"] {
            assert!(body.contains(&format!("## {h}")), "{name} lacks ## {h}");
        }
    }
}

#[test]
fn ac_0_1_5_a_prd_adrs_0001_to_0027_exist() {
    let all = adrs();
    for n in 1..=27 {
        let p = format!("{n:04}-");
        assert!(
            all.iter().any(|(f, _)| f.starts_with(&p)),
            "missing ADR {n:04}"
        );
    }
}

#[test]
fn ac_0_1_5_b_status_is_one_of_accepted_superseded_open() {
    for (name, body) in adrs() {
        let line = body
            .lines()
            .find(|l| l.to_lowercase().starts_with("status:"))
            .unwrap_or_else(|| panic!("{name}: no `Status:` line"));
        let st = line.split_once(':').unwrap().1.trim().to_lowercase();
        assert!(
            ["accepted", "superseded", "open"]
                .iter()
                .any(|s| st.starts_with(s)),
            "{name}: {st}"
        );
    }
}

#[test]
fn ac_0_1_5_c_numbering_has_no_gaps_and_names_are_kebab_case() {
    let nums: Vec<u32> = adrs()
        .iter()
        .map(|(f, _)| f[..4].parse().unwrap())
        .collect();
    for (i, n) in nums.iter().enumerate() {
        assert_eq!(*n as usize, i + 1, "gap or duplicate near {n:04}");
    }
    for (f, _) in adrs() {
        assert!(
            f.trim_end_matches(".md")[5..]
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{f}"
        );
    }
}
