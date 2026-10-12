use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use greprum::{
    VERSION,
    collect::{self, RequestInfo},
    config::Config,
    jobs,
    metrics::Metrics,
    migrate,
    query::{self, Reader as _, SqlxReader},
    server::{self, AppState},
    sink::{Sink, SqlxSink},
    spike,
    spool::{Spool, drainer::Drainer},
    synth::Generator,
};
use std::{path::PathBuf, sync::Arc, sync::atomic::AtomicBool};

#[derive(Parser)]
#[command(name = "greprum", version = VERSION, about = "Product analytics and RUM in one binary, on GreptimeDB")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the gateway: tracker, ingest, query API.
    Serve {
        #[arg(long, default_value = "greprum.toml")]
        config: PathBuf,
    },
    /// Create tables and flows on GreptimeDB (idempotent).
    Migrate {
        #[arg(long, default_value = "greprum.toml")]
        config: PathBuf,
        /// Print the SQL and exit; no database connection.
        #[arg(long)]
        print: bool,
        /// Keep retrying for up to this many seconds while GreptimeDB starts (compose, CI).
        #[arg(long, default_value_t = 0)]
        wait: u64,
    },
    /// Generate synthetic events (spike / load tests).
    Gen {
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, default_value_t = 100_000)]
        events: u64,
        #[arg(long, default_value = "demo")]
        site: String,
        #[arg(long, default_value_t = 7)]
        days: i64,
        /// Write to this MySQL URL; omit to print NDJSON wire batches to stdout.
        #[arg(long)]
        url: Option<String>,
        #[arg(long, default_value_t = 1.0)]
        sample_rate: f64,
    },
    /// Time the architecture-deciding queries and check the PRD gates.
    Spike {
        #[arg(long, default_value = "mysql://root@127.0.0.1:4002/public")]
        url: String,
        #[arg(long, default_value = "demo")]
        site: String,
        #[arg(long, default_value_t = 5)]
        runs: usize,
        #[arg(long, default_value_t = 7)]
        days: i64,
    },
    /// Run a background job once (same code path as the scheduler).
    Jobs {
        #[command(subcommand)]
        action: JobsCmd,
    },
    /// Print the SHA-256 hex of an API key for `[[api_key]].sha256`.
    HashKey { key: String },
    /// Probe `/healthz` (used by the container HEALTHCHECK).
    Healthcheck {
        /// host:port; defaults to GREPRUM_LISTEN or 127.0.0.1:8080.
        #[arg(long)]
        addr: Option<String>,
    },
    /// Print the version.
    Version,
}

#[derive(Subcommand)]
enum JobsCmd {
    RunOnce {
        /// first-seen | reconcile
        #[arg(long)]
        job: String,
        #[arg(long)]
        site: Option<String>,
        #[arg(long, default_value = "greprum.toml")]
        config: PathBuf,
        /// Print the SQL and exit; no database connection.
        #[arg(long)]
        print: bool,
    },
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    match Cli::parse().cmd {
        Cmd::Version => println!("greprum {VERSION}"),
        Cmd::Healthcheck { addr } => {
            let listen = addr
                .or_else(|| std::env::var("GREPRUM_LISTEN").ok())
                .unwrap_or_else(|| "127.0.0.1:8080".into());
            greprum::health::check(&greprum::health::probe_addr(&listen)).await?;
        }
        Cmd::Jobs {
            action:
                JobsCmd::RunOnce {
                    job,
                    site,
                    config,
                    print,
                },
        } => {
            let job = jobs::JobName::parse(&job)?;
            if print {
                println!("{}", job.sql());
                return Ok(());
            }
            let cfg = Config::load(&config)?;
            let runner = jobs::SqlJobs::lazy(&cfg.greptime.write_url)?;
            let sites: Vec<String> = match site {
                Some(s) => vec![s],
                None => cfg.sites.iter().map(|s| s.id.clone()).collect(),
            };
            for s in sites {
                let v = jobs::run_once(&runner, job, &s, server::system_clock()()).await?;
                println!("{v}");
            }
        }
        Cmd::HashKey { key } => println!("{}", query::hash_key(&key)),
        Cmd::Migrate {
            config,
            print,
            wait,
        } => {
            let cfg = if config.exists() {
                Config::load(&config)?
            } else {
                Config::default()
            };
            if print {
                for s in migrate::plan_prefixed(&cfg.greptime.ttl, &cfg.greptime.flow_prefix)? {
                    println!("-- {}\n{};\n", s.name, s.sql);
                }
            } else {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait);
                let done = loop {
                    match migrate::apply(
                        &cfg.greptime.write_url,
                        &cfg.greptime.ttl,
                        &cfg.greptime.flow_prefix,
                    )
                    .await
                    {
                        Ok(done) => break done,
                        Err(e) if std::time::Instant::now() < deadline => {
                            eprintln!("database not ready ({e:#}); retrying");
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }
                        Err(e) => return Err(e),
                    }
                };
                for n in done {
                    println!("ok  {n}");
                }
            }
        }
        Cmd::Serve { config } => serve(Config::load(&config)?).await?,
        Cmd::Gen {
            seed,
            events,
            site,
            days,
            url,
            sample_rate,
        } => gen_events(seed, events, &site, days, url, sample_rate).await?,
        Cmd::Spike {
            url,
            site,
            runs,
            days,
        } => spike_run(&url, &site, runs, days).await?,
    }
    Ok(())
}

async fn serve(cfg: Config) -> Result<()> {
    let cfg = Arc::new(cfg);
    let metrics = Arc::new(Metrics::default());
    let dir = PathBuf::from(&cfg.spool.dir);
    let spool = Spool::open(&dir, cfg.spool.clone(), metrics.clone()).context("opening spool")?;
    server::load_usage(&dir, &metrics);

    // Lazy pools: the gateway starts and spools even when GreptimeDB is down.
    let sink: Arc<dyn Sink> =
        Arc::new(SqlxSink::lazy(&cfg.greptime.write_url).context("write pool")?);
    let reader: Option<Arc<dyn query::Reader>> = Some(Arc::new(
        SqlxReader::lazy(cfg.greptime.read_url()).context("read pool")?,
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let drainer = Drainer::new(spool.clone(), sink, cfg.ingest.clone(), metrics.clone());
    tokio::spawn(drainer.run(stop.clone()));

    if cfg.jobs.enabled && !cfg.sites.is_empty() {
        let runner: Arc<dyn jobs::JobRunner> =
            Arc::new(jobs::SqlJobs::lazy(&cfg.greptime.write_url)?);
        let sched = jobs::Scheduler::new(
            runner,
            cfg.sites.iter().map(|s| s.id.clone()).collect(),
            cfg.jobs.first_seen_secs,
            cfg.jobs.reconcile_secs,
            metrics.clone(),
        );
        tokio::spawn(async move {
            loop {
                sched.tick(server::system_clock()()).await;
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });
    }

    let m2 = metrics.clone();
    let d2 = dir.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let _ = server::save_usage(&d2, &m2);
        }
    });

    let state = AppState::new(
        cfg.clone(),
        spool,
        metrics.clone(),
        reader,
        server::system_clock(),
    );
    let app = server::router(state);
    let listener = tokio::net::TcpListener::bind(&cfg.server.listen).await?;
    tracing::info!("greprum {VERSION} listening on {}", cfg.server.listen);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = server::save_usage(&dir, &metrics);
    Ok(())
}

async fn gen_events(
    seed: u64,
    target: u64,
    site: &str,
    days: i64,
    url: Option<String>,
    sample_rate: f64,
) -> Result<()> {
    let now = server::system_clock()();
    let mut g = Generator::new(seed, now - days * 86_400_000, days * 86_400_000 - 60_000);
    g.sample_rate = sample_rate;
    let site_cfg = greprum::config::SiteConfig {
        id: site.into(),
        key: "pk_gen".into(),
        origins: vec![],
        hmac_secret: String::new(),
        timezone: "UTC".into(),
        drop_attrs: vec![],
        store_text: true,
        honour_dnt: false,
        ..Default::default()
    };
    let ingest = greprum::config::IngestConfig::default();
    let sink = match &url {
        Some(u) => Some(SqlxSink::connect(u).await?),
        None => None,
    };
    let mut buf = vec![];
    let mut n = 0u64;
    while n < target {
        let s = g.session();
        let info = RequestInfo {
            user_agent: s.user_agent.into(),
            country: s.country.into(),
            dnt: false,
        };
        for b in s.batches {
            // sent_at is in the generator's past: process with received_at ≈ sent_at so ts is kept.
            let received = b["sent_at"].as_i64().unwrap_or(now);
            let body = b.to_string();
            let p = collect::process(body.as_bytes(), &site_cfg, &info, &ingest, received)
                .map_err(|e| anyhow::anyhow!("generator produced an invalid batch: {e:?}"))?;
            n += p.events.len() as u64;
            match &sink {
                Some(_) => buf.extend(p.events),
                None => println!("{body}"),
            }
        }
        if let (Some(sink), true) = (&sink, buf.len() >= 5000) {
            sink.write(&buf).await.map_err(|e| anyhow::anyhow!("{e}"))?;
            buf.clear();
        }
    }
    if let (Some(sink), false) = (&sink, buf.is_empty()) {
        sink.write(&buf).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    eprintln!("generated {n} events for site {site}");
    Ok(())
}

async fn spike_run(url: &str, site: &str, runs: usize, days: i64) -> Result<()> {
    use query::{Kind, build};
    use std::collections::HashMap;
    let reader = SqlxReader::connect(url).await?;
    let now = server::system_clock()();
    let cfg_site = greprum::config::SiteConfig {
        id: site.into(),
        key: "pk_x".into(),
        origins: vec![],
        hmac_secret: String::new(),
        timezone: "UTC".into(),
        drop_attrs: vec![],
        store_text: true,
        honour_dnt: false,
        ..Default::default()
    };
    let mk = |kind: Kind, extra: &[(&str, &str)], span_days: i64| -> Result<build::Params> {
        let mut q: HashMap<String, String> = extra
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        q.insert("from".into(), (now - span_days * 86_400_000).to_string());
        q.insert("to".into(), now.to_string());
        build::Params::parse(kind, &cfg_site, &q, None, now).map_err(|e| anyhow::anyhow!("{}", e.1))
    };
    let span = days.min(30);
    type Case<'a> = (&'a str, Kind, Vec<(&'a str, &'a str)>);
    let cases: Vec<Case> = vec![
        ("rollup_overview", Kind::Overview, vec![]),
        ("rollup_top_pages", Kind::TopPages, vec![]),
        ("rollup_journeys", Kind::Journeys, vec![]),
        (
            "funnel",
            Kind::Funnel,
            vec![
                ("steps", "$session_start,$pageview,signup"),
                ("window_seconds", "1800"),
            ],
        ),
        ("retention", Kind::Retention, vec![("days", "7")]),
        ("sessions_24h", Kind::Sessions, vec![]),
    ];
    let mut report = vec![];
    for (name, kind, extra) in cases {
        let p = mk(kind, &extra, if kind == Kind::Sessions { 1 } else { span })?;
        let plan = build::plan(kind, &p).map_err(|e| anyhow::anyhow!("{}", e.1))?;
        let mut ms = vec![];
        for _ in 0..runs {
            let t = std::time::Instant::now();
            for q in &plan {
                reader
                    .fetch(q)
                    .await
                    .with_context(|| format!("{name}/{}", q.name))?;
            }
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        report.push(spike::evaluate(name, &ms).context("gate")?);
    }
    // count(DISTINCT) over raw rows (spike item 5)
    let q = query::Q {
        name: "cd",
        sql: "SELECT CAST(count(DISTINCT visitor_id) AS BIGINT) AS n FROM events WHERE site_id = ?"
            .into(),
        binds: vec![query::Bind::Str(site.into())],
    };
    let mut ms = vec![];
    for _ in 0..runs {
        let t = std::time::Instant::now();
        reader.fetch(&q).await?;
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    report.push(spike::evaluate("count_distinct_raw", &ms).context("gate")?);
    println!("{}", serde_json::to_string_pretty(&report)?);
    if report.iter().any(|v| !v.pass) {
        bail!("one or more spike gates missed (see adopt_summary_job)");
    }
    Ok(())
}
