#![allow(dead_code)]
//! Real-process helpers: spawn the `greprum` binary, talk HTTP to it, kill it.
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub const SITE: &str = "ops";
pub const KEY: &str = "pk_ops";

pub fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

pub struct Gateway {
    pub child: Child,
    pub port: u16,
}

pub fn write_config(dir: &Path, port: u16, db_url: &str, extra: &str) -> PathBuf {
    let cfg = format!(
        "[server]\nlisten = \"127.0.0.1:{port}\"\nrate_limit_per_min = 1000000\n[spool]\ndir = \"{}\"\n[greptime]\nwrite_url = \"{db_url}\"\n\
         [jobs]\nenabled = false\n[[site]]\nid = \"{SITE}\"\nkey = \"{KEY}\"\norigins = [\"https://example.com\"]\n{extra}",
        dir.join("spool").display()
    );
    let p = dir.join("greprum.toml");
    std::fs::File::create(&p)
        .unwrap()
        .write_all(cfg.as_bytes())
        .unwrap();
    p
}

pub async fn start(dir: &Path, port: u16, db_url: &str) -> Gateway {
    let cfg = write_config(dir, port, db_url, "");
    let child = Command::new(env!("CARGO_BIN_EXE_greprum"))
        .args(["serve", "--config"])
        .arg(&cfg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn greprum");
    let g = Gateway { child, port };
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(15) {
        if get(port, "/healthz").await.is_some_and(|(s, _)| s == 200) {
            return g;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("gateway did not become healthy");
}

impl Gateway {
    /// SIGKILL: no shutdown hooks, no flush.
    pub fn kill9(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn request(port: u16, raw: String) -> Option<(u16, String)> {
    let mut s = tokio::time::timeout(
        Duration::from_secs(5),
        TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .ok()?
    .ok()?;
    s.write_all(raw.as_bytes()).await.ok()?;
    let mut buf = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), s.read_to_end(&mut buf))
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status = text.split_whitespace().nth(1)?.parse().ok()?;
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        dechunk(rest)
    } else {
        rest.to_string()
    };
    Some((status, body))
}

pub async fn get(port: u16, path: &str) -> Option<(u16, String)> {
    request(
        port,
        format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"),
    )
    .await
}

/// POST one batch; `Some(status)` when a response arrived, `None` when the connection broke.
pub async fn post_batch(port: u16, session: &str, events: usize, ts: i64) -> Option<u16> {
    let evs: Vec<String> = (0..events)
        .map(|i| {
            format!(
                r#"{{"ts":{},"seq":{i},"name":"$pageview","path":"/p{i}"}}"#,
                ts + i as i64
            )
        })
        .collect();
    let body = format!(
        r#"{{"visitor_id":"v_{session}","session_id":"{session}","events":[{}]}}"#,
        evs.join(",")
    );
    let raw = format!(
        "POST /collect?k={KEY} HTTP/1.1\r\nHost: x\r\nOrigin: https://example.com\r\nUser-Agent: Mozilla/5.0 Safari/605\r\n\
         Content-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    request(port, raw).await.map(|(s, _)| s)
}

pub async fn metric(port: u16, name: &str) -> Option<f64> {
    let (_, body) = get(port, "/metrics").await?;
    body.lines()
        .find(|l| l.starts_with(name) && !l.starts_with('#'))
        .and_then(|l| l.split_whitespace().last())
        .and_then(|v| v.parse().ok())
}

fn dechunk(mut s: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = s.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 || rest.len() < n {
            break;
        }
        out.push_str(&rest[..n]);
        s = rest[n..].trim_start_matches("\r\n");
    }
    out
}
