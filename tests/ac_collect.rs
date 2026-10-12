//! L1/L2 tests for the ingest pipeline: AC0.4.4 – AC0.4.10.
mod common;
use common::*;
use greprum::collect::{self, Admission, BatchError, dedup::Dedup};
use greprum::model::{BUILTIN_EVENTS, Priority};
use serde_json::json;
use std::time::{Duration, Instant};

// ---- AC0.4.4 caps -----------------------------------------------------------

#[test]
fn ac_0_4_4_a_id_200_ok_201_rejects_without_truncation() {
    let mut b = batch(vec![ev("$pageview", 0)]);
    b["visitor_id"] = json!("x".repeat(200));
    assert_eq!(run(&b).unwrap().events[0].visitor_id.len(), 200);
    b["visitor_id"] = json!("x".repeat(201));
    assert_eq!(run(&b).unwrap_err(), BatchError::BadIds);
    b["visitor_id"] = json!("v");
    b["session_id"] = json!("y".repeat(201));
    assert_eq!(run(&b).unwrap_err(), BatchError::BadIds);
}

#[test]
fn ac_0_4_4_b_props_4096_ok_4097_rejects_event() {
    let ok = "a".repeat(4096 - r#"{"k":""}"#.len());
    let mut e = ev("signup", 0);
    e["props"] = json!({"k": ok});
    let p = run(&batch(vec![e.clone()])).unwrap();
    assert_eq!(p.events.len(), 1);
    assert!(p.events[0].props.len() <= 4096);
    e["props"] = json!({"k": "a".repeat(4097)});
    let p = run(&batch(vec![e])).unwrap();
    assert!(p.events.is_empty());
    assert_eq!(p.dropped, vec![("props_too_large", 1)]);
}

#[test]
fn ac_0_4_4_c_ctx_caps_keys_key_len_value_len() {
    let mut ctx = serde_json::Map::new();
    for i in 0..15 {
        ctx.insert(format!("k{i:02}"), json!("v"));
    }
    let mut b = batch(vec![ev("$pageview", 0)]);
    b["ctx"] = json!(ctx);
    let stored: serde_json::Value = serde_json::from_str(&run(&b).unwrap().events[0].ctx).unwrap();
    assert!(stored.as_object().unwrap().len() <= 10);

    b["ctx"] = json!({"k".repeat(33): "v", "ok": "z".repeat(300)});
    let stored: serde_json::Value = serde_json::from_str(&run(&b).unwrap().events[0].ctx).unwrap();
    let o = stored.as_object().unwrap();
    assert!(!o.contains_key(&"k".repeat(33)));
    assert_eq!(o["ok"].as_str().unwrap().len(), 200);
}

#[test]
fn ac_0_4_4_d_seq_must_be_non_negative_integer() {
    for bad in [json!(-1), json!(1.5), json!("a"), json!(null)] {
        let mut e = ev("$pageview", 0);
        e["seq"] = bad;
        let p = run(&batch(vec![e])).unwrap();
        assert!(p.events.is_empty());
        assert_eq!(p.dropped, vec![("bad_seq", 1)]);
    }
    assert_eq!(
        run(&batch(vec![ev("$pageview", 0)])).unwrap().events.len(),
        1
    );
}

#[test]
fn ac_0_4_4_e_sample_rate_outside_0_1_stored_as_1() {
    for (given, want) in [
        (0.0, 1.0),
        (-0.5, 1.0),
        (2.0, 1.0),
        (0.25, 0.25),
        (1.0, 1.0),
    ] {
        assert_eq!(collect::clean_sample_rate(Some(given)), want);
    }
    assert_eq!(collect::clean_sample_rate(Some(f64::NAN)), 1.0);
    assert_eq!(collect::clean_sample_rate(None), 1.0);
    let mut b = batch(vec![ev("$pageview", 0)]);
    b["sample_rate"] = json!(0.25);
    assert_eq!(run(&b).unwrap().events[0].sample_rate, 0.25);
}

#[test]
fn ac_0_4_4_f_group_env_release_cut_to_64_and_text_to_50() {
    let mut b = batch(vec![ev("$click", 0)]);
    b["env"] = json!("e".repeat(100));
    b["release"] = json!("r".repeat(100));
    b["events"][0]["page_group"] = json!("g".repeat(100));
    b["events"][0]["element"] = json!({"tag": "button", "text": "t".repeat(100)});
    let e = &run(&b).unwrap().events[0];
    assert_eq!(
        (
            e.env.len(),
            e.release.len(),
            e.page_group.len(),
            e.el_text.len()
        ),
        (64, 64, 64, 50)
    );
}

#[test]
fn ac_0_4_4_g_batch_over_50_events_rejected() {
    let ok = batch((0..50).map(|i| ev("$pageview", i)).collect());
    assert_eq!(run(&ok).unwrap().events.len(), 50);
    let big = batch((0..51).map(|i| ev("$pageview", i)).collect());
    assert_eq!(run(&big).unwrap_err(), BatchError::TooManyEvents);
}

// ---- AC0.4.5 names ----------------------------------------------------------

#[test]
fn ac_0_4_5_a_all_builtins_pass() {
    let evs: Vec<_> = BUILTIN_EVENTS
        .iter()
        .enumerate()
        .map(|(i, n)| ev(n, i as i64))
        .collect();
    let p = run(&batch(evs)).unwrap();
    assert_eq!(p.events.len(), BUILTIN_EVENTS.len());
    assert_eq!(p.dropped_count(), 0);
}

#[test]
fn ac_0_4_5_b_unknown_dollar_name_dropped_and_counted() {
    let p = run(&batch(vec![ev("$bogus", 0), ev("$pageview", 1)])).unwrap();
    assert_eq!(p.events.len(), 1);
    assert_eq!(p.dropped, vec![("unknown_builtin", 1)]);
}

#[test]
fn ac_0_4_5_c_custom_name_regex_and_length() {
    for bad in ["Bad Name", "UPPER", "a".repeat(65).as_str(), "emoji😀", ""] {
        assert!(!collect::valid_custom_name(bad), "{bad}");
    }
    for good in ["signup", "a.b:c-d_e", "0", &"a".repeat(64)] {
        assert!(collect::valid_custom_name(good), "{good}");
    }
    let p = run(&batch(vec![ev("Bad Name", 0)])).unwrap();
    assert_eq!(p.dropped, vec![("bad_name", 1)]);
}

// ---- AC0.4.6 enrichment & privacy ------------------------------------------

#[test]
fn ac_0_4_6_a_ua_fields_and_country_set() {
    let e = &run(&batch(vec![ev("$pageview", 0)])).unwrap().events[0];
    assert_eq!(
        (e.browser.as_str(), e.os.as_str(), e.device.as_str()),
        ("Safari", "macOS", "desktop")
    );
    assert_eq!(e.country, "SG");
}

#[test]
fn ac_0_4_6_b_bot_dropped() {
    let mut r = req();
    r.user_agent = "Googlebot/2.1".into();
    let b = batch(vec![ev("$pageview", 0)]).to_string();
    let got = collect::process(b.as_bytes(), &site(), &r, &cfg(), NOW);
    assert_eq!(got.unwrap_err(), BatchError::Bot);
}

#[test]
fn ac_0_4_6_c_path_loses_query_and_hash() {
    for (raw, want) in [
        ("/p?token=abc#x", "/p"),
        ("/a/b#frag", "/a/b"),
        ("https://x.com/q?z=1", "/q"),
        ("nolead", "/nolead"),
        ("/redirect?u=http://evil/x", "/redirect"),
    ] {
        assert_eq!(collect::normalise_path(raw), want);
    }
}

#[test]
fn ac_0_4_6_d_stored_event_never_holds_query_or_ip_fuzz() {
    let secrets = ["?email=a@b.c", "#access_token=zzz", "?ip=203.0.113.9"];
    for s in secrets {
        let mut e = ev("$pageview", 0);
        e["path"] = json!(format!("/page{s}"));
        e["referrer"] = json!(format!("https://ref.example.org/x{s}"));
        let ser = serde_json::to_string(&run(&batch(vec![e])).unwrap().events[0]).unwrap();
        assert!(
            !ser.contains("a@b.c") && !ser.contains("zzz") && !ser.contains("203.0.113.9"),
            "{ser}"
        );
    }
}

#[test]
fn ac_0_4_6_e_referrer_stored_as_host_only() {
    let mut e = ev("$session_start", 0);
    e["referrer"] = json!("https://News.Example.com/a/b?x=1");
    assert_eq!(
        run(&batch(vec![e])).unwrap().events[0].referrer_host,
        "news.example.com"
    );
}

#[test]
fn ac_0_4_6_f_dnt_honoured_when_site_setting_on() {
    let mut s = site();
    s.honour_dnt = true;
    let mut r = req();
    r.dnt = true;
    let b = batch(vec![ev("$pageview", 0)]).to_string();
    assert_eq!(
        collect::process(b.as_bytes(), &s, &r, &cfg(), NOW).unwrap_err(),
        BatchError::DoNotTrack
    );
    s.honour_dnt = false;
    assert!(collect::process(b.as_bytes(), &s, &r, &cfg(), NOW).is_ok());
}

#[test]
fn ac_0_4_6_g_session_start_keeps_only_utm_props() {
    let mut e = ev("$session_start", 0);
    e["props"] = json!({"utm_source": "news", "email": "a@b.c"});
    let p = &run(&batch(vec![e])).unwrap().events[0].props;
    assert!(p.contains("utm_source") && !p.contains("email"));
}

#[test]
fn ac_0_4_6_h_site_drops_listed_attrs_and_can_disable_text() {
    let mut s = site();
    s.drop_attrs = vec!["data-email".into()];
    s.store_text = false;
    let mut e = ev("$click", 0);
    e["element"] =
        json!({"tag": "a", "text": "Hi", "attrs": {"data-email": "a@b.c", "data-x": "1"}});
    let b = batch(vec![e]).to_string();
    let got = collect::process(b.as_bytes(), &s, &req(), &cfg(), NOW).unwrap();
    assert!(!got.events[0].el_attrs.contains("a@b.c") && got.events[0].el_attrs.contains("data-x"));
    assert_eq!(got.events[0].el_text, "");
}

// ---- AC0.4.7 identity -------------------------------------------------------

fn token(secret: &str, alg: jsonwebtoken::Algorithm, exp_delta: i64) -> String {
    // NOW is a fixed date; jsonwebtoken validates exp against the real clock, so anchor to it.
    let real = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let exp = if exp_delta >= 0 {
        real + exp_delta as u64
    } else {
        real - (-exp_delta) as u64
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(alg),
        &json!({"sub": "u_42", "exp": exp}),
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap()
}

#[test]
fn ac_0_4_7_a_valid_token_sets_user_and_identified() {
    let mut b = batch(vec![ev("$identify", 0)]);
    b["token"] = json!(token("secret", jsonwebtoken::Algorithm::HS256, 300));
    let e = &run(&b).unwrap().events[0];
    assert_eq!((e.user_id.as_str(), e.is_identified), ("u_42", true));
}

#[test]
fn ac_0_4_7_b_expired_wrong_secret_other_alg_are_anonymous() {
    for t in [
        token("secret", jsonwebtoken::Algorithm::HS256, -60),
        token("other", jsonwebtoken::Algorithm::HS256, 300),
        token("secret", jsonwebtoken::Algorithm::HS384, 300),
        "garbage".to_string(),
    ] {
        let mut b = batch(vec![ev("$identify", 0)]);
        b["token"] = json!(t);
        let p = run(&b).unwrap();
        assert_eq!(p.events.len(), 1, "event stored anonymous, not dropped");
        assert!(!p.events[0].is_identified && p.events[0].user_id.is_empty());
    }
}

#[test]
fn ac_0_4_7_c_token_signed_for_other_site_secret_rejected() {
    let mut s = site();
    s.hmac_secret = "site-b-secret".into();
    let mut b = batch(vec![ev("$identify", 0)]);
    b["token"] = json!(token("secret", jsonwebtoken::Algorithm::HS256, 300));
    let got = collect::process(b.to_string().as_bytes(), &s, &req(), &cfg(), NOW).unwrap();
    assert!(!got.events[0].is_identified);
}

// ---- AC0.4.8 clock fix ------------------------------------------------------

fn at(ts: i64, sent_at: Option<i64>) -> Option<(i64, bool)> {
    collect::fix_clock(ts, sent_at, NOW, &cfg())
}

#[test]
fn ac_0_4_8_a_skewed_browser_clock_corrected() {
    // Browser clock is 1 h fast: sent_at and ts both shifted; event happened 5 s before send.
    let skew = 3_600_000;
    assert_eq!(
        at(NOW + skew - 5_000, Some(NOW + skew)),
        Some((NOW - 5_000, false))
    );
}

#[test]
fn ac_0_4_8_b_future_clamped_to_received_at() {
    assert_eq!(at(NOW + 6 * 60_000, None), Some((NOW, false)));
    assert_eq!(at(NOW + 4 * 60_000, None), Some((NOW + 4 * 60_000, false)));
}

#[test]
fn ac_0_4_8_c_late_flag_at_one_day() {
    let day = 86_400_000;
    assert!(!at(NOW - day, None).unwrap().1);
    assert!(at(NOW - day - 1, None).unwrap().1);
}

#[test]
fn ac_0_4_8_d_older_than_seven_days_dropped_and_counted() {
    let week = 7 * 86_400_000;
    assert!(at(NOW - week - 1, None).is_none());
    assert!(at(NOW - week, None).is_some());
    let mut e = ev("$pageview", 0);
    e["ts"] = json!(NOW - week - 10_000);
    let b = json!({"visitor_id":"v","session_id":"s","events":[e]});
    let p = run(&b).unwrap();
    assert_eq!(p.dropped, vec![("too_old", 1)]);
}

#[test]
fn ac_0_4_8_e_no_sent_at_means_no_correction() {
    assert_eq!(at(NOW - 1234, None), Some((NOW - 1234, false)));
}

// ---- AC0.4.9 dedup ----------------------------------------------------------

#[test]
fn ac_0_4_9_a_retry_dropped_other_site_and_seq_pass() {
    let t = Instant::now();
    let mut d = Dedup::new(Duration::from_secs(600), 1000);
    d.insert("s1", "a", 7, t);
    assert!(d.contains("s1", "a", 7, t));
    assert!(!d.contains("s2", "a", 7, t));
    assert!(!d.contains("s1", "a", 8, t));
}

#[test]
fn ac_0_4_9_b_entry_expires_after_window() {
    let t = Instant::now();
    let mut d = Dedup::new(Duration::from_secs(600), 1000);
    d.insert("s", "a", 1, t);
    assert!(d.contains("s", "a", 1, t + Duration::from_secs(599)));
    assert!(!d.contains("s", "a", 1, t + Duration::from_secs(601)));
}

#[test]
fn ac_0_4_9_c_cache_bounded_under_flood() {
    let t = Instant::now();
    let mut d = Dedup::new(Duration::from_secs(600), 100);
    for i in 0..5000 {
        d.insert("s", "a", i, t);
        d.contains("s", "a", -1, t);
    }
    assert!(d.len() <= 101, "len {}", d.len());
}

// ---- AC0.4.10 priority & shedding -------------------------------------------

#[test]
fn ac_0_4_10_a_classification() {
    for n in ["$rage_click", "$dead_click", "$error"] {
        assert_eq!(Priority::of(n), Priority::P1);
    }
    for n in ["$scroll", "$web_vital", "$pageleave"] {
        assert_eq!(Priority::of(n), Priority::P2);
    }
    for n in [
        "$session_start",
        "$pageview",
        "$click",
        "$identify",
        "$form_submit",
        "custom_thing",
    ] {
        assert_eq!(Priority::of(n), Priority::P0);
    }
}

#[test]
fn ac_0_4_10_b_thresholds_50_and_80() {
    use Priority::*;
    assert_eq!(collect::admission(P2, 49.9, 50, 80), Admission::Admit);
    assert_eq!(collect::admission(P2, 50.0, 50, 80), Admission::Shed);
    assert_eq!(collect::admission(P1, 79.9, 50, 80), Admission::Admit);
    assert_eq!(collect::admission(P1, 80.0, 50, 80), Admission::Shed);
}

#[test]
fn ac_0_4_10_c_p0_survives_until_full_then_503() {
    assert_eq!(
        collect::admission(Priority::P0, 99.9, 50, 80),
        Admission::Admit
    );
    assert_eq!(
        collect::admission(Priority::P0, 100.0, 50, 80),
        Admission::Full
    );
}
