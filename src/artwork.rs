//! Artwork Store: byte-bounded decoded-texture LRU + byte-bounded disk cache (ADR 0010).

use crate::lru::WeightedLru;
use gtk::prelude::*;
use gtk::{gdk, glib};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

pub const MEMORY_BUDGET: usize = 64 * 1024 * 1024;
pub const DISK_BUDGET: u64 = 256 * 1024 * 1024;
/// Largest artwork download accepted (guards against huge/hostile responses).
const MAX_DOWNLOAD: usize = 4 * 1024 * 1024;

/// Rewrite a thumbnail URL to request roughly `px` pixels (Google image-server suffixes).
pub fn sized_thumbnail(url: &str, px: u32) -> String {
    if url.contains("googleusercontent.com") || url.contains("ggpht.com") {
        if let Some(pos) = url.rfind('=') {
            let suffix = &url[pos + 1..];
            if suffix.starts_with('w') || suffix.starts_with('s') {
                return format!("{}=w{px}-h{px}-l90-rj", &url[..pos]);
            }
        }
    }
    // i.ytimg.com video thumbnails: pick the smallest variant that covers `px`.
    if url.contains("i.ytimg.com/vi") {
        if let Some(slash) = url.rfind('/') {
            let file = url[slash + 1..].split('?').next().unwrap_or("");
            let is_jpg = file.ends_with(".jpg") || file.ends_with(".webp");
            if is_jpg && (file.contains("default") || file.starts_with("hq720")) {
                let ext = if file.ends_with(".webp") {
                    "webp"
                } else {
                    "jpg"
                };
                let variant = if px <= 180 { "mqdefault" } else { "hqdefault" };
                return format!("{}/{variant}.{ext}", &url[..slash]);
            }
        }
    }
    url.to_string()
}

/// Content-addressed on-disk artwork bytes, bounded by total size, evicted by last access.
pub struct DiskCache {
    dir: PathBuf,
    budget: u64,
}

impl DiskCache {
    pub fn new(dir: impl Into<PathBuf>, budget: u64) -> io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir, budget })
    }

    fn path(&self, url: &str) -> PathBuf {
        self.dir
            .join(hex::encode(&Sha256::digest(url.as_bytes())[..16]))
    }

    pub fn get(&self, url: &str) -> Option<Vec<u8>> {
        let p = self.path(url);
        let data = fs::read(&p).ok()?;
        // Touch mtime as "last access" (atime is often disabled/relatime).
        if let Ok(f) = fs::File::options().append(true).open(&p) {
            let _ = f.set_modified(SystemTime::now());
        }
        Some(data)
    }

    pub fn put(&self, url: &str, data: &[u8]) -> io::Result<()> {
        let p = self.path(url);
        let tmp = p.with_extension("part");
        fs::write(&tmp, data)?;
        fs::rename(tmp, p)
    }

    /// Remove least-recently-used files until the directory fits the budget.
    /// Returns (files removed, bytes remaining).
    pub fn prune(&self) -> io::Result<(usize, u64)> {
        prune_dir(&self.dir, self.budget)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

pub fn prune_dir(dir: &Path, budget: u64) -> io::Result<(usize, u64)> {
    let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let mut total = 0u64;
    for e in fs::read_dir(dir)? {
        let e = e?;
        let meta = e.metadata()?;
        if !meta.is_file() {
            continue;
        }
        total += meta.len();
        files.push((
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
            e.path(),
        ));
    }
    if total <= budget {
        return Ok((0, total));
    }
    files.sort_by_key(|(t, _, _)| *t);
    let mut removed = 0;
    for (_, len, path) in files {
        if total <= budget {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total -= len;
            removed += 1;
        }
    }
    Ok((removed, total))
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ArtworkError {
    #[error("artwork download failed: {0}")]
    Network(String),
    #[error("artwork could not be decoded: {0}")]
    Decode(String),
}

type Waiters = Vec<futures::channel::oneshot::Sender<Result<gdk::Texture, ArtworkError>>>;

struct Inner {
    memory: WeightedLru<String, gdk::Texture>,
    pending: HashMap<String, Waiters>,
}

/// Main-thread handle; cheap to clone.
#[derive(Clone)]
pub struct ArtworkStore {
    inner: Rc<RefCell<Inner>>,
    disk: std::sync::Arc<DiskCache>,
    http: reqwest::Client,
}

fn texture_weight(t: &gdk::Texture) -> usize {
    (t.width().max(1) as usize) * (t.height().max(1) as usize) * 4
}

impl ArtworkStore {
    pub fn new(disk: DiskCache, memory_budget: usize, http: reqwest::Client) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                memory: WeightedLru::new(memory_budget),
                pending: HashMap::new(),
            })),
            disk: std::sync::Arc::new(disk),
            http,
        }
    }

    pub fn cached(&self, url: &str) -> Option<gdk::Texture> {
        self.inner
            .borrow_mut()
            .memory
            .get(&url.to_string())
            .cloned()
    }

    /// Insert a decoded texture (also used by tests to exercise the budget).
    pub fn insert(&self, url: &str, tex: gdk::Texture) {
        let w = texture_weight(&tex);
        // Evicted textures drop here; widgets still showing them keep their own reference.
        drop(
            self.inner
                .borrow_mut()
                .memory
                .insert(url.to_string(), tex, w),
        );
    }

    pub fn memory_bytes(&self) -> usize {
        self.inner.borrow().memory.total_weight()
    }
    pub fn memory_len(&self) -> usize {
        self.inner.borrow().memory.len()
    }

    /// Fetch artwork (memory → disk → network). Concurrent calls for one URL are coalesced.
    pub async fn load(&self, url: &str) -> Result<gdk::Texture, ArtworkError> {
        if let Some(t) = self.cached(url) {
            return Ok(t);
        }
        let (tx, rx) = futures::channel::oneshot::channel();
        let first = {
            let mut inner = self.inner.borrow_mut();
            let waiters = inner.pending.entry(url.to_string()).or_default();
            waiters.push(tx);
            waiters.len() == 1
        };
        if first {
            let disk = self.disk.clone();
            let http = self.http.clone();
            let key = url.to_string();
            let result = crate::runtime::run(fetch_and_decode(disk, http, key.clone()))
                .await
                .unwrap_or_else(|e| Err(ArtworkError::Network(e)));
            if let Ok(tex) = &result {
                self.insert(&key, tex.clone());
            }
            let waiters = self
                .inner
                .borrow_mut()
                .pending
                .remove(&key)
                .unwrap_or_default();
            for w in waiters {
                let _ = w.send(result.clone());
            }
        }
        rx.await
            .unwrap_or_else(|_| Err(ArtworkError::Network("request cancelled".into())))
    }

    /// Periodic GC: enforce the memory budget and prune the disk cache.
    pub fn collect(&self) {
        let mut inner = self.inner.borrow_mut();
        let budget = inner.memory.budget();
        drop(inner.memory.shrink_to(budget));
        drop(inner);
        let disk = self.disk.clone();
        crate::runtime::runtime().spawn_blocking(move || match disk.prune() {
            Ok((n, left)) if n > 0 => {
                log::debug!("artwork disk GC removed {n} files, {left} bytes left")
            }
            Ok(_) => {}
            Err(e) => log::warn!("artwork disk GC failed: {e}"),
        });
    }

    /// Drop every decoded texture (e.g. when the window is hidden).
    pub fn clear_memory(&self) {
        self.inner.borrow_mut().memory.clear();
    }
}

async fn fetch_and_decode(
    disk: std::sync::Arc<DiskCache>,
    http: reqwest::Client,
    url: String,
) -> Result<gdk::Texture, ArtworkError> {
    let cached = {
        let (d, u) = (disk.clone(), url.clone());
        tokio::task::spawn_blocking(move || d.get(&u))
            .await
            .ok()
            .flatten()
    };
    let bytes = match cached {
        Some(b) => b,
        None => {
            let resp = http
                .get(&url)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| ArtworkError::Network(e.to_string()))?;
            if resp
                .content_length()
                .is_some_and(|l| l as usize > MAX_DOWNLOAD)
            {
                return Err(ArtworkError::Network("image too large".into()));
            }
            let b = resp
                .bytes()
                .await
                .map_err(|e| ArtworkError::Network(e.to_string()))?;
            if b.len() > MAX_DOWNLOAD {
                return Err(ArtworkError::Network("image too large".into()));
            }
            let (d, u, data) = (disk.clone(), url.clone(), b.to_vec());
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = d.put(&u, &data) {
                    log::warn!("artwork disk write failed: {e}");
                }
            })
            .await;
            b.to_vec()
        }
    };
    tokio::task::spawn_blocking(move || {
        gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes))
            .map_err(|e| ArtworkError::Decode(e.to_string()))
    })
    .await
    .map_err(|e| ArtworkError::Decode(e.to_string()))?
}
