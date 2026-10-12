//! Container health probe: `greprum healthcheck` (distroless has no curl).

use anyhow::{Context, Result, bail};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Duration, timeout},
};

/// `0.0.0.0:8080` is not connectable on every platform: probe loopback instead.
pub fn probe_addr(listen: &str) -> String {
    listen
        .replacen("0.0.0.0", "127.0.0.1", 1)
        .replacen("[::]", "127.0.0.1", 1)
}

pub async fn check(addr: &str) -> Result<()> {
    let mut s = timeout(Duration::from_secs(2), TcpStream::connect(addr))
        .await
        .context("connect timeout")?
        .with_context(|| format!("connect {addr}"))?;
    s.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await?;
    let mut buf = vec![0u8; 256];
    let n = timeout(Duration::from_secs(2), s.read(&mut buf))
        .await
        .context("read timeout")??;
    let head = String::from_utf8_lossy(&buf[..n]);
    if head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200") {
        Ok(())
    } else {
        bail!("unhealthy: {}", head.lines().next().unwrap_or(""))
    }
}
