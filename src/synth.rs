//! Synthetic event generator for the spike and load tests (PRD §10 phase 0).
//!
//! Produces tracker-shaped wire batches (what `/collect` receives), so generated data goes
//! through the real validation pipeline. Deterministic for a given seed.

use rand::{Rng, SeedableRng, rngs::StdRng};
use serde_json::{Value, json};

const PAGES: &[(&str, &str)] = &[
    ("/", "home"),
    ("/pricing", "pricing"),
    ("/docs", "docs"),
    ("/docs/start", "docs"),
    ("/blog", "blog"),
    ("/signup", "signup"),
    ("/signup/done", "signup"),
    ("/login", "login"),
];
const BUTTONS: &[(&str, &str, &str)] = &[
    ("button#cta", "Start free", "main>div.hero>button#cta"),
    ("a.nav", "Pricing", "header>nav>a.nav"),
    ("button#buy", "Buy", "main>form>button#buy"),
    ("div.card", "Docs", "main>div.card"),
];
const COUNTRIES: &[&str] = &["SG", "US", "DE", "IN", "MY", "ID"];
const AGENTS: &[&str] = &[
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/17.0 Safari/605.1.15",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120.0 Safari/537.36",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 Version/17.0 Mobile/15E148 Safari/604.1",
    "Mozilla/5.0 (X11; Linux x86_64; rv:121.0) Gecko/20100101 Firefox/121.0",
];

pub struct Generated {
    pub batches: Vec<Value>,
    pub user_agent: &'static str,
    pub country: &'static str,
}

pub struct Generator {
    rng: StdRng,
    next_session: u64,
    pub visitors: u64,
    pub start_ms: i64,
    pub span_ms: i64,
    pub sample_rate: f64,
}

impl Generator {
    pub fn new(seed: u64, start_ms: i64, span_ms: i64) -> Self {
        Generator {
            rng: StdRng::seed_from_u64(seed),
            next_session: 0,
            visitors: 5000,
            start_ms,
            span_ms,
            sample_rate: 1.0,
        }
    }

    /// One session as ≤ 50-event wire batches.
    pub fn session(&mut self) -> Generated {
        let r = &mut self.rng;
        let visitor = format!("v_{}", r.gen_range(0..self.visitors));
        let sid = format!("s_{}_{}", self.next_session, r.gen_range(0..1_000_000));
        self.next_session += 1;
        let t0 = self.start_ms + r.gen_range(0..self.span_ms.max(1));
        let mut ts = t0;
        let mut seq = 0i64;
        let mut evs: Vec<Value> = vec![];
        let mut push =
            |ts: i64, name: &str, path: &str, group: &str, parent: &str, extra: Value| {
                let mut e =
                    json!({"ts": ts, "seq": seq, "name": name, "path": path, "page_group": group});
                if !parent.is_empty() {
                    e["parent_path"] = json!(parent);
                }
                if let (Some(o), Some(x)) = (e.as_object_mut(), extra.as_object()) {
                    for (k, v) in x {
                        o.insert(k.clone(), v.clone());
                    }
                }
                seq += 1;
                evs.push(e);
            };
        let (mut path, mut group) = PAGES[r.gen_range(0..PAGES.len())];
        let is_new = r.gen_bool(0.4);
        let utm = ["newsletter", "twitter", "search"][r.gen_range(0..3)];
        push(
            ts,
            "$session_start",
            path,
            group,
            "",
            json!({
                "is_new_visitor": is_new, "referrer": "https://news.example.org/post?x=1",
                "props": {"utm_source": utm}
            }),
        );
        push(ts, "$pageview", path, group, "", json!({}));
        let steps = r.gen_range(1..8);
        for _ in 0..steps {
            ts += r.gen_range(500..20_000);
            match r.gen_range(0..10) {
                0..=3 => {
                    let (p, g) = PAGES[r.gen_range(0..PAGES.len())];
                    push(ts, "$pageview", p, g, path, json!({}));
                    path = p;
                    group = g;
                }
                4..=6 => {
                    let (css_tail, text, css) = BUTTONS[r.gen_range(0..BUTTONS.len())];
                    push(
                        ts,
                        "$click",
                        path,
                        group,
                        "",
                        json!({"element": {"tag": "button", "id": css_tail, "text": text, "css": css, "attrs": {}}}),
                    );
                }
                7 => push(
                    ts,
                    "$scroll",
                    path,
                    group,
                    "",
                    json!({"props": {"depth": 50}}),
                ),
                8 => {
                    push(
                        ts,
                        "$rage_click",
                        path,
                        group,
                        "",
                        json!({"element": {"tag": "button", "id": "buy", "text": "Buy", "css": "main>form>button#buy", "attrs": {}}}),
                    );
                    push(
                        ts + 10,
                        "$dead_click",
                        path,
                        group,
                        "",
                        json!({"element": {"tag": "div", "id": "", "text": "Docs", "css": "main>div.card", "attrs": {}}}),
                    );
                }
                _ => push(
                    ts,
                    "$web_vital",
                    path,
                    group,
                    "",
                    json!({"props": {"name": "INP", "value": r.gen_range(20..600)}}),
                ),
            }
        }
        if r.gen_bool(0.1) {
            push(
                ts + 100,
                "$error",
                path,
                group,
                "",
                json!({"props": {"message": "TypeError: x is undefined"}}),
            );
        }
        if r.gen_bool(0.15) {
            push(
                ts + 200,
                "$form_submit",
                path,
                group,
                "",
                json!({"props": {"form_id": "signup", "fields": ["email", "name"]}}),
            );
            push(ts + 300, "signup", "/signup/done", "signup", "", json!({}));
        }
        if r.gen_bool(0.2) {
            push(ts + 400, "$identify", path, group, "", json!({}));
        }
        push(
            ts + 1000,
            "$pageleave",
            path,
            group,
            "",
            json!({"props": {"duration_ms": ts + 1000 - t0}}),
        );

        let sent_at = ts + 1500;
        let batches = evs
            .chunks(50)
            .map(|c| {
                json!({
                    "visitor_id": visitor, "session_id": sid, "sent_at": sent_at,
                    "sample_rate": self.sample_rate, "env": "prod", "release": "1.0.0",
                    "events": c,
                })
            })
            .collect();
        Generated {
            batches,
            user_agent: AGENTS[self.rng.gen_range(0..AGENTS.len())],
            country: COUNTRIES[self.rng.gen_range(0..COUNTRIES.len())],
        }
    }
}
