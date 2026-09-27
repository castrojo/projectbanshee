//! Durable app memory (ADR 0012): the Queue with the resume point, search history and
//! results, every Track seen, and preferences. Nothing the user asked for is lost on quit
//! or crash: writes are atomic (temp + rename, mode 0600) and happen shortly after every
//! change, not only at shutdown.

use crate::model::{SearchFilter, SearchItem, SourceKind, Track};
use crate::queue::Queue;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::{Path, PathBuf};

pub const RECENT_SEARCHES_CAP: usize = 50;
pub const SAVED_RESULT_SETS_CAP: usize = 300;
pub const SEEN_TRACKS_CAP: usize = 20_000;

/// Queue plus where playback was, so a restart resumes at the same entry and second.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub queue: Queue,
    /// Playback position in the current entry, whole seconds.
    #[serde(default)]
    pub position_secs: u64,
}

/// A search result list as the service returned it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedResults {
    pub source: SourceKind,
    pub filter: SearchFilter,
    /// Normalised (trimmed, lowercase) query.
    pub query: String,
    pub items: Vec<SearchItem>,
}

pub fn normalise_query(q: &str) -> String {
    q.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Search memory: recent queries (newest first) and their result lists (most recent last).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchHistory {
    pub recent: Vec<String>,
    pub results: Vec<SavedResults>,
    #[serde(default)]
    pub last_query: String,
    #[serde(default)]
    pub last_filter: SearchFilter,
}

impl SearchHistory {
    /// Record a query the user acted on (queued from / searched). Case-insensitive dedup,
    /// newest first, capped.
    pub fn remember_query(&mut self, query: &str) {
        let q = query.split_whitespace().collect::<Vec<_>>().join(" ");
        if q.is_empty() {
            return;
        }
        let norm = q.to_lowercase();
        self.recent.retain(|r| r.to_lowercase() != norm);
        self.recent.insert(0, q);
        self.recent.truncate(RECENT_SEARCHES_CAP);
    }

    pub fn forget_query(&mut self, query: &str) {
        let norm = normalise_query(query);
        self.recent.retain(|r| r.to_lowercase() != norm);
    }

    /// Store a result list, replacing an older one for the same key; oldest sets are evicted.
    pub fn store_results(
        &mut self,
        source: SourceKind,
        filter: SearchFilter,
        query: &str,
        items: &[SearchItem],
    ) {
        let query = normalise_query(query);
        self.results
            .retain(|r| !(r.source == source && r.filter == filter && r.query == query));
        self.results.push(SavedResults {
            source,
            filter,
            query,
            items: items.to_vec(),
        });
        if self.results.len() > SAVED_RESULT_SETS_CAP {
            let excess = self.results.len() - SAVED_RESULT_SETS_CAP;
            self.results.drain(..excess);
        }
    }

    pub fn results_for(
        &self,
        source: SourceKind,
        filter: SearchFilter,
        query: &str,
    ) -> Option<Vec<SearchItem>> {
        let query = normalise_query(query);
        self.results
            .iter()
            .rev()
            .find(|r| r.source == source && r.filter == filter && r.query == query)
            .map(|r| r.items.clone())
    }

    pub fn purge_source(&mut self, source: SourceKind) {
        self.results.retain(|r| r.source != source);
    }
}

/// Small user preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefs {
    pub volume: f64,
    pub window_width: i32,
    pub window_height: i32,
    pub maximized: bool,
    pub show_queue: bool,
    pub mini_mode: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            volume: 1.0,
            window_width: 1120,
            window_height: 760,
            maximized: false,
            show_queue: true,
            mini_mode: false,
        }
    }
}

/// JSON files in the app's state directory.
pub struct StateStore {
    dir: PathBuf,
}

impl StateStore {
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.json"))
    }

    /// Load `name`, or `T::default()` if absent. A corrupt file is kept aside as
    /// `<name>.json.corrupt` (never silently deleted) and the default is returned.
    pub fn load<T: DeserializeOwned + Default>(&self, name: &str) -> T {
        let path = self.path(name);
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return T::default(),
            Err(e) => {
                log::error!("reading {}: {e}", path.display());
                return T::default();
            }
        };
        match serde_json::from_slice(&data) {
            Ok(v) => v,
            Err(e) => {
                let aside = path.with_extension("json.corrupt");
                log::error!(
                    "{} is unreadable ({e}); kept as {}",
                    path.display(),
                    aside.display()
                );
                let _ = std::fs::rename(&path, &aside);
                T::default()
            }
        }
    }

    pub fn save<T: Serialize>(&self, name: &str, value: &T) -> std::io::Result<()> {
        let data = serde_json::to_vec(value).map_err(std::io::Error::other)?;
        crate::paths::write_private(&self.path(name), &data)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Seen tracks in insertion order (the Local Index persisted).
pub type SeenTracks = Vec<Track>;
