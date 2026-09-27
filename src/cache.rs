//! Stale-while-revalidate JSON cache for Library surfaces and Collections (ADR 0011).

use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Result of a cache read.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup<T> {
    /// Within TTL: show it, no fetch needed.
    Fresh(T),
    /// Past TTL: show it now, revalidate in the background.
    Stale(T),
    /// Absent or unreadable: fetch.
    Missing,
}

impl<T> Lookup<T> {
    pub fn value(self) -> Option<T> {
        match self {
            Lookup::Fresh(v) | Lookup::Stale(v) => Some(v),
            Lookup::Missing => None,
        }
    }
    pub fn needs_fetch(&self) -> bool {
        !matches!(self, Lookup::Fresh(_))
    }
}

#[derive(serde::Deserialize)]
struct EnvelopeOwned<T> {
    stored_at: u64,
    value: T,
}

#[derive(Serialize)]
struct EnvelopeRef<'a, T> {
    stored_at: u64,
    key: &'a str,
    value: &'a T,
}

/// File-name-safe namespace; `path()` and `purge_namespace()` must agree on it.
fn ns_slug(namespace: &str) -> String {
    namespace
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

pub struct JsonCache {
    dir: PathBuf,
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl JsonCache {
    pub fn new(dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `namespace` groups entries for purging (e.g. the source slug).
    fn path(&self, namespace: &str, key: &str) -> PathBuf {
        let digest = hex::encode(Sha256::digest(key.as_bytes()));
        let ns = ns_slug(namespace);
        self.dir.join(format!("{ns}-{}.json", &digest[..32]))
    }

    pub fn get<T: DeserializeOwned>(&self, namespace: &str, key: &str, ttl: Duration) -> Lookup<T> {
        self.get_at(namespace, key, ttl, SystemTime::now())
    }

    pub fn get_at<T: DeserializeOwned>(
        &self,
        namespace: &str,
        key: &str,
        ttl: Duration,
        now: SystemTime,
    ) -> Lookup<T> {
        let path = self.path(namespace, key);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    log::warn!("cache read {}: {e}", path.display());
                }
                return Lookup::Missing;
            }
        };
        match serde_json::from_slice::<EnvelopeOwned<T>>(&bytes) {
            Ok(env) => {
                let age = secs(now).saturating_sub(env.stored_at);
                if age <= ttl.as_secs() {
                    Lookup::Fresh(env.value)
                } else {
                    Lookup::Stale(env.value)
                }
            }
            Err(e) => {
                log::warn!("discarding corrupt cache entry {}: {e}", path.display());
                let _ = fs::remove_file(&path);
                Lookup::Missing
            }
        }
    }

    pub fn put<T: Serialize>(&self, namespace: &str, key: &str, value: &T) -> io::Result<()> {
        self.put_at(namespace, key, value, SystemTime::now())
    }

    /// Atomic write (temp file + rename) so a crash never leaves a torn entry.
    pub fn put_at<T: Serialize>(
        &self,
        namespace: &str,
        key: &str,
        value: &T,
        now: SystemTime,
    ) -> io::Result<()> {
        let path = self.path(namespace, key);
        let env = EnvelopeRef {
            stored_at: secs(now),
            key,
            value,
        };
        let data = serde_json::to_vec(&env).map_err(io::Error::other)?;
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&data)?;
            f.sync_data().ok();
        }
        fs::rename(&tmp, &path)
    }

    pub fn remove(&self, namespace: &str, key: &str) {
        let _ = fs::remove_file(self.path(namespace, key));
    }

    /// Delete every entry of a namespace (sign-out).
    pub fn purge_namespace(&self, namespace: &str) -> io::Result<usize> {
        let prefix = format!("{}-", ns_slug(namespace));
        let mut n = 0;
        for e in fs::read_dir(&self.dir)? {
            let e = e?;
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                fs::remove_file(e.path())?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// Delete entries not written for `max_age` (periodic GC). Returns number removed.
    pub fn prune_older_than(&self, max_age: Duration, now: SystemTime) -> usize {
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return 0;
        };
        let mut n = 0;
        for e in rd.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            let old = meta
                .modified()
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|age| age > max_age);
            if old && fs::remove_file(e.path()).is_ok() {
                n += 1;
            }
        }
        n
    }
}
