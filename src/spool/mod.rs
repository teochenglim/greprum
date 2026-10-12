//! Durable spool (PRD §7, ADR 0007): length-prefixed, checksummed frames, one per accepted
//! batch, in fixed-size segment files, with group commit.
//!
//! ```text
//! frame   = len:u32le  crc32:u32le  payload[len]      (payload = JSON SpoolBatch)
//! segment = seg-<20 digit id>.wal
//! ```
//!
//! A frame is *acknowledged* once it has been written to the OS (or fsynced in strict mode).

pub mod drainer;

use crate::{config::SpoolConfig, metrics::Metrics, model::SpoolBatch};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering::Relaxed},
        mpsc,
    },
    time::{Duration, Instant},
};

pub const MAX_FRAME: u32 = 16 * 1024 * 1024;
const HEADER: u64 = 8;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    Default,
)]
pub struct Pos {
    pub seg: u64,
    pub off: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum SpoolError {
    #[error("spool is full")]
    Full,
    #[error("spool io: {0}")]
    Io(String),
}

pub fn seg_path(dir: &Path, id: u64) -> PathBuf {
    dir.join(format!("seg-{id:020}.wal"))
}

pub fn list_segments(dir: &Path) -> Vec<u64> {
    let mut v: Vec<u64> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.strip_prefix("seg-")?.strip_suffix(".wal")?.parse().ok()
        })
        .collect();
    v.sort_unstable();
    v
}

pub fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + HEADER as usize);
    f.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    f.extend_from_slice(&crc32fast::hash(payload).to_le_bytes());
    f.extend_from_slice(payload);
    f
}

#[derive(Debug)]
pub enum Read_ {
    /// A decoded batch and the position after it.
    Batch(Box<SpoolBatch>, Pos),
    /// A frame whose checksum or JSON is bad; skip it (length was plausible).
    Corrupt(Pos),
    /// Nothing more to read right now.
    End,
}

/// Read the frame at `pos`, hopping to the next segment at a clean end of file.
pub fn read_at(dir: &Path, mut pos: Pos) -> io::Result<Read_> {
    loop {
        let segs = list_segments(dir);
        if !segs.contains(&pos.seg) {
            return match segs.iter().find(|s| **s > pos.seg) {
                Some(n) => {
                    pos = Pos { seg: *n, off: 0 };
                    continue;
                }
                None => Ok(Read_::End),
            };
        }
        let has_newer = segs.iter().any(|s| *s > pos.seg);
        let mut f = File::open(seg_path(dir, pos.seg))?;
        let size = f.metadata()?.len();
        let hop = |pos: &mut Pos| -> bool {
            if has_newer && let Some(n) = segs.iter().find(|s| **s > pos.seg) {
                *pos = Pos { seg: *n, off: 0 };
                return true;
            }
            false
        };
        if size < pos.off + HEADER {
            if hop(&mut pos) {
                continue;
            }
            return Ok(Read_::End);
        }
        f.seek(SeekFrom::Start(pos.off))?;
        let mut h = [0u8; 8];
        f.read_exact(&mut h)?;
        let len = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
        let crc = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
        if len > MAX_FRAME || size < pos.off + HEADER + len as u64 {
            // Torn tail (or garbage length): nothing valid beyond here in this segment.
            if hop(&mut pos) {
                continue;
            }
            return Ok(Read_::End);
        }
        let mut payload = vec![0u8; len as usize];
        f.read_exact(&mut payload)?;
        let next = Pos {
            seg: pos.seg,
            off: pos.off + HEADER + len as u64,
        };
        if crc32fast::hash(&payload) != crc {
            return Ok(Read_::Corrupt(next));
        }
        return Ok(match serde_json::from_slice::<SpoolBatch>(&payload) {
            Ok(b) => Read_::Batch(Box::new(b), next),
            Err(_) => Read_::Corrupt(next),
        });
    }
}

struct AppendReq {
    frame: Vec<u8>,
    ack: tokio::sync::oneshot::Sender<io::Result<()>>,
}

pub struct Spool {
    dir: PathBuf,
    cfg: SpoolConfig,
    tx: mpsc::Sender<AppendReq>,
    bytes: Arc<AtomicU64>,
    tail: Arc<Mutex<Pos>>,
    pub metrics: Arc<Metrics>,
}

impl Spool {
    /// Open (or create) the spool. A torn tail left by a crash is truncated.
    pub fn open(dir: &Path, cfg: SpoolConfig, metrics: Arc<Metrics>) -> io::Result<Arc<Spool>> {
        fs::create_dir_all(dir)?;
        fs::create_dir_all(dir.join("dead"))?;
        let segs = list_segments(dir);
        let active = segs.last().copied().unwrap_or(1);
        let valid_end = if segs.is_empty() {
            0
        } else {
            scan_valid_end(dir, active)?
        };
        let path = seg_path(dir, active);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        if file.metadata()?.len() != valid_end {
            file.set_len(valid_end)?;
        }
        let total: u64 = list_segments(dir)
            .iter()
            .map(|s| {
                fs::metadata(seg_path(dir, *s))
                    .map(|m| m.len())
                    .unwrap_or(0)
            })
            .sum();
        let bytes = Arc::new(AtomicU64::new(total));
        let tail = Arc::new(Mutex::new(Pos {
            seg: active,
            off: valid_end,
        }));
        metrics.spool_cap_bytes.store(cfg.cap_bytes, Relaxed);
        metrics.spool_bytes.store(total, Relaxed);

        let (tx, rx) = mpsc::channel::<AppendReq>();
        let w = Writer {
            dir: dir.to_path_buf(),
            cfg: cfg.clone(),
            file,
            seg: active,
            size: valid_end,
            unsynced: 0,
            last_sync: Instant::now(),
            bytes: bytes.clone(),
            tail: tail.clone(),
            metrics: metrics.clone(),
        };
        std::thread::Builder::new()
            .name("greprum-spool-writer".into())
            .spawn(move || w.run(rx))?;
        Ok(Arc::new(Spool {
            dir: dir.to_path_buf(),
            cfg,
            tx,
            bytes,
            tail,
            metrics,
        }))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn bytes(&self) -> u64 {
        self.bytes.load(Relaxed)
    }

    pub fn fill_pct(&self) -> f64 {
        self.bytes() as f64 * 100.0 / self.cfg.cap_bytes.max(1) as f64
    }

    pub fn tail(&self) -> Pos {
        self.tail.lock().map(|t| *t).unwrap_or_default()
    }

    pub fn add_bytes_removed(&self, n: u64) {
        let _ = self
            .bytes
            .try_update(Relaxed, Relaxed, |b| Some(b.saturating_sub(n)));
        self.metrics.spool_bytes.store(self.bytes(), Relaxed);
    }

    /// Append one frame. Returns after the group commit wrote it (and fsynced, if strict).
    pub async fn append(&self, batch: &SpoolBatch) -> Result<(), SpoolError> {
        let payload = serde_json::to_vec(batch).map_err(|e| SpoolError::Io(e.to_string()))?;
        let frame = encode_frame(&payload);
        if self.bytes() + frame.len() as u64 > self.cfg.cap_bytes {
            return Err(SpoolError::Full);
        }
        let (ack, rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(AppendReq { frame, ack })
            .map_err(|_| SpoolError::Io("writer stopped".into()))?;
        match rx.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(SpoolError::Io(e.to_string())),
            Err(_) => Err(SpoolError::Io("writer dropped ack".into())),
        }
    }
}

fn scan_valid_end(dir: &Path, seg: u64) -> io::Result<u64> {
    let mut f = File::open(seg_path(dir, seg))?;
    let size = f.metadata()?.len();
    let mut off = 0u64;
    loop {
        if size < off + HEADER {
            return Ok(off);
        }
        f.seek(SeekFrom::Start(off))?;
        let mut h = [0u8; 8];
        f.read_exact(&mut h)?;
        let len = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
        if len > MAX_FRAME || size < off + HEADER + len as u64 {
            return Ok(off);
        }
        off += HEADER + len as u64;
    }
}

struct Writer {
    dir: PathBuf,
    cfg: SpoolConfig,
    file: File,
    seg: u64,
    size: u64,
    unsynced: u64,
    last_sync: Instant,
    bytes: Arc<AtomicU64>,
    tail: Arc<Mutex<Pos>>,
    metrics: Arc<Metrics>,
}

impl Writer {
    fn run(mut self, rx: mpsc::Receiver<AppendReq>) {
        let interval = Duration::from_millis(self.cfg.fsync_ms.max(1));
        loop {
            let first = if self.unsynced > 0 {
                match rx.recv_timeout(interval) {
                    Ok(r) => Some(r),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let _ = self.sync();
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        let _ = self.sync();
                        return;
                    }
                }
            } else {
                match rx.recv() {
                    Ok(r) => Some(r),
                    Err(_) => return,
                }
            };
            // Group commit: drain everything queued behind the first request.
            let mut group: Vec<AppendReq> = first.into_iter().collect();
            while let Ok(r) = rx.try_recv() {
                group.push(r);
            }
            let res = self.write_group(&group);
            for r in group {
                let _ = r.ack.send(match &res {
                    Ok(()) => Ok(()),
                    Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
                });
            }
        }
    }

    fn write_group(&mut self, group: &[AppendReq]) -> io::Result<()> {
        for r in group {
            if self.size > 0 && self.size + r.frame.len() as u64 > self.cfg.segment_bytes {
                self.roll()?;
            }
            self.file.write_all(&r.frame)?;
            self.size += r.frame.len() as u64;
            self.unsynced += r.frame.len() as u64;
            self.bytes.fetch_add(r.frame.len() as u64, Relaxed);
        }
        self.file.flush()?;
        if let Ok(mut t) = self.tail.lock() {
            *t = Pos {
                seg: self.seg,
                off: self.size,
            };
        }
        self.metrics
            .spool_bytes
            .store(self.bytes.load(Relaxed), Relaxed);
        if self.cfg.strict_fsync
            || self.unsynced >= self.cfg.fsync_bytes
            || self.last_sync.elapsed() >= Duration::from_millis(self.cfg.fsync_ms)
        {
            self.sync()?;
        }
        Ok(())
    }

    fn sync(&mut self) -> io::Result<()> {
        if self.unsynced > 0 {
            self.file.sync_data()?;
            self.unsynced = 0;
        }
        self.last_sync = Instant::now();
        Ok(())
    }

    fn roll(&mut self) -> io::Result<()> {
        self.sync()?;
        self.seg += 1;
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(seg_path(&self.dir, self.seg))?;
        self.size = 0;
        Ok(())
    }
}
