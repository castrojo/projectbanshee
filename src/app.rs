//! App controller: owns the Queue, Player Core, Audio Sources, caches and the Search Engine
//! state, and broadcasts [`AppEvent`]s to the UI. Everything here runs on the GTK main thread;
//! network work is delegated to the Tokio runtime through `banshee::runtime::run`.

use adw::prelude::*;
use banshee::artwork::{ArtworkStore, DiskCache};
use banshee::cache::{JsonCache, Lookup};
use banshee::fuzzy::{self, LocalIndex, Ranked, Scorer};
use banshee::lru::WeightedLru;
use banshee::model::{
    Collection, LibrarySection, Playable, SearchFilter, SearchItem, SourceKind, Track,
};
use banshee::mpris::{self, Command, Mpris, MprisTarget, Snapshot};
use banshee::persist::{Prefs, SearchHistory, SeenTracks, Session, StateStore};
use banshee::player::{PlaybackState, PlayerCore, PlayerEvent};
use banshee::queue::{Advance, EntryId, Queue, QueueEntry, RepeatMode};
use banshee::runtime::run;
use banshee::sources::spotify::SpotifySource;
use banshee::sources::youtube::YouTubeMusicSource;
use banshee::sources::{AudioSource, Resolved, SourceError};
use banshee::{memory, paths};
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const LIBRARY_TTL: Duration = Duration::from_secs(6 * 3600);
pub const COLLECTION_TTL: Duration = Duration::from_secs(3600);
pub const HOME_TTL: Duration = Duration::from_secs(30 * 60);
const SEARCH_MEMO_TTL: Duration = Duration::from_secs(600);
const RESOLVED_TTL: Duration = Duration::from_secs(45 * 60);
const GC_INTERVAL: Duration = Duration::from_secs(60);
const LOCAL_INDEX_CAP: usize = 20_000;
const MAX_CONSECUTIVE_FAILURES: u32 = 5;

/// Things the UI reacts to.
#[derive(Clone)]
pub enum AppEvent {
    QueueChanged,
    NowPlaying(Option<QueueEntry>),
    State(PlaybackState),
    Position {
        position: Duration,
        duration: Option<Duration>,
    },
    Busy(bool),
    Toast(ToastSpec),
    Video(Option<gdk::Paintable>),
    AccountsChanged,
    ModesChanged,
    HistoryChanged,
    Raise,
}

#[derive(Clone)]
pub struct ToastSpec {
    pub title: String,
    pub undo: Option<Rc<dyn Fn()>>,
    pub priority_high: bool,
}

impl ToastSpec {
    pub fn info(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            undo: None,
            priority_high: false,
        }
    }
    pub fn error(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            undo: None,
            priority_high: true,
        }
    }
}

/// Home feed as the UI sees it.
#[derive(Clone)]
pub enum HomeState {
    Loading,
    Ready(Vec<banshee::model::HomeShelf>),
    Failed(String),
}

/// Library data for one source as the UI sees it.
#[derive(Clone)]
pub enum LibraryState {
    SignedOut,
    Loading,
    Ready {
        sections: Vec<LibrarySection>,
        refreshing: bool,
    },
    Failed(String),
}

type Listener = Box<dyn Fn(&AppEvent)>;

pub struct Controller {
    pub app: adw::Application,
    queue: RefCell<Queue>,
    pub queue_store: gio::ListStore,
    pub player: PlayerCore,
    pub youtube: Arc<YouTubeMusicSource>,
    pub spotify: Arc<SpotifySource>,
    pub cache: Option<JsonCache>,
    pub artwork: ArtworkStore,
    /// Discord Rich Presence ("Listening to …").
    pub presence: banshee::discord::Presence,
    local: RefCell<LocalIndex>,
    scorer: RefCell<Scorer>,
    search_memo: RefCell<WeightedLru<(SourceKind, SearchFilter, String), Vec<SearchItem>>>,
    resolved: RefCell<HashMap<EntryId, (Instant, Resolved)>>,
    listeners: RefCell<Vec<Listener>>,
    busy: Cell<u32>,
    play_generation: Cell<u64>,
    failures: Cell<u32>,
    last_state: Cell<Option<PlaybackState>>,
    last_position: Cell<(Duration, Option<Duration>)>,
    mpris: RefCell<Option<Mpris>>,
    save_pending: Cell<bool>,
    store: Option<StateStore>,
    pub history: RefCell<SearchHistory>,
    pub prefs: RefCell<Prefs>,
    history_dirty: Cell<bool>,
    seen_dirty: Cell<bool>,
    /// Where to resume the restored current entry once it starts playing.
    resume_at: Cell<Option<(EntryId, Duration)>>,
    prefs_save_pending: Cell<bool>,
    link_seq: Cell<u64>,
    /// Entry whose stream was re-resolved after a playback error (retry once).
    retried_entry: Cell<Option<EntryId>>,
    link_next: Cell<u64>,
    link_ready: RefCell<std::collections::BTreeMap<u64, Result<Track, String>>>,
    /// Bumped on sign-in/out so library fetches started for the old account are dropped.
    accounts_epoch: Cell<u64>,
    last_saved_position: Cell<u64>,
    library_inflight: RefCell<std::collections::HashSet<SourceKind>>,
    self_weak: RefCell<Weak<Controller>>,
}

fn source_label(s: SourceKind) -> &'static str {
    match s {
        SourceKind::YouTubeMusic => "YouTube Music",
        SourceKind::Spotify => "Spotify",
    }
}

/// Human message for a failed source call.
pub fn describe(source: SourceKind, e: &SourceError) -> String {
    format!("{}: {e}", source_label(source))
}

impl Controller {
    pub fn new(app: &adw::Application) -> Result<Rc<Self>, String> {
        let player =
            PlayerCore::new().map_err(|e| format!("Audio playback is unavailable: {e}"))?;
        let cache = match JsonCache::new(paths::cache_dir().join("library")) {
            Ok(c) => Some(c),
            Err(e) => {
                log::error!("library cache disabled: {e}");
                None
            }
        };
        let http = reqwest::Client::builder()
            .user_agent(concat!("Banshee/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| format!("HTTP client: {e}"))?;
        let disk = DiskCache::new(
            paths::cache_dir().join("artwork"),
            banshee::artwork::DISK_BUDGET,
        )
        .map_err(|e| format!("artwork cache: {e}"))?;
        let store = match StateStore::new(paths::state_dir()) {
            Ok(s) => Some(s),
            Err(e) => {
                log::error!("state directory unavailable, nothing will be remembered: {e}");
                None
            }
        };
        let this = Rc::new(Self {
            app: app.clone(),
            queue: RefCell::new(Queue::new()),
            queue_store: gio::ListStore::new::<glib::BoxedAnyObject>(),
            player,
            youtube: Arc::new(YouTubeMusicSource::new()),
            spotify: Arc::new(SpotifySource::new()),
            cache,
            artwork: ArtworkStore::new(disk, banshee::artwork::MEMORY_BUDGET, http),
            presence: banshee::discord::Presence::new(),
            local: RefCell::new(LocalIndex::new(LOCAL_INDEX_CAP)),
            scorer: RefCell::new(Scorer::default()),
            search_memo: RefCell::new(WeightedLru::new(256).with_ttl(SEARCH_MEMO_TTL)),
            resolved: RefCell::new(HashMap::new()),
            listeners: RefCell::new(Vec::new()),
            busy: Cell::new(0),
            play_generation: Cell::new(0),
            failures: Cell::new(0),
            last_state: Cell::new(None),
            last_position: Cell::new((Duration::ZERO, None)),
            mpris: RefCell::new(None),
            save_pending: Cell::new(false),
            history: RefCell::new(store.as_ref().map(|s| s.load("search")).unwrap_or_default()),
            prefs: RefCell::new(store.as_ref().map(|s| s.load("prefs")).unwrap_or_default()),
            store,
            history_dirty: Cell::new(false),
            seen_dirty: Cell::new(false),
            resume_at: Cell::new(None),
            prefs_save_pending: Cell::new(false),
            link_seq: Cell::new(0),
            retried_entry: Cell::new(None),
            link_next: Cell::new(0),
            link_ready: RefCell::new(Default::default()),
            accounts_epoch: Cell::new(0),
            last_saved_position: Cell::new(0),
            library_inflight: RefCell::new(Default::default()),
            self_weak: RefCell::new(Weak::new()),
        });
        *this.self_weak.borrow_mut() = Rc::downgrade(&this);
        this.wire_player();
        this.restore_state();
        let target: Rc<dyn MprisTarget> = Rc::new(MprisBridge(Rc::downgrade(&this)));
        *this.mpris.borrow_mut() = Some(Mpris::new(target));
        {
            let p = this.prefs.borrow();
            this.presence
                .configure(p.discord_presence, p.discord_client_id.clone());
        }
        this.start_gc();
        Ok(this)
    }

    fn weak(&self) -> Weak<Controller> {
        self.self_weak.borrow().clone()
    }

    pub fn subscribe(&self, f: impl Fn(&AppEvent) + 'static) {
        self.listeners.borrow_mut().push(Box::new(f));
    }

    fn emit(&self, ev: AppEvent) {
        // Listeners may call back into the controller, but never subscribe while emitting.
        for l in self.listeners.borrow().iter() {
            l(&ev);
        }
        if matches!(ev, AppEvent::NowPlaying(_) | AppEvent::State(_)) {
            self.push_presence();
        }
        if matches!(
            ev,
            AppEvent::QueueChanged
                | AppEvent::NowPlaying(_)
                | AppEvent::State(_)
                | AppEvent::ModesChanged
        ) {
            if let Some(m) = self.mpris.borrow().as_ref() {
                m.notify();
            }
        }
    }

    pub fn toast(&self, t: ToastSpec) {
        self.emit(AppEvent::Toast(t));
    }

    pub fn toast_error(&self, msg: impl Into<String>) {
        let msg = msg.into();
        log::warn!("{msg}");
        self.toast(ToastSpec::error(msg));
    }

    /// Track background work for the global loading indicator. Drop the guard when done.
    pub fn busy_guard(&self) -> BusyGuard {
        let n = self.busy.get() + 1;
        self.busy.set(n);
        if n == 1 {
            self.emit(AppEvent::Busy(true));
        }
        BusyGuard(self.weak())
    }

    fn unbusy(&self) {
        let n = self.busy.get().saturating_sub(1);
        self.busy.set(n);
        if n == 0 {
            self.emit(AppEvent::Busy(false));
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy.get() > 0
    }

    pub fn source(&self, s: SourceKind) -> Arc<dyn AudioSource> {
        match s {
            SourceKind::YouTubeMusic => self.youtube.clone(),
            SourceKind::Spotify => self.spotify.clone(),
        }
    }

    // ---------------------------------------------------------------- Queue

    pub fn queue_len(&self) -> usize {
        self.queue.borrow().len()
    }
    pub fn current_entry(&self) -> Option<QueueEntry> {
        self.queue.borrow().current().cloned()
    }
    pub fn current_index(&self) -> Option<usize> {
        self.queue.borrow().current_index()
    }
    /// What plays after the current entry finishes (for "Up next").
    pub fn up_next(&self) -> Option<QueueEntry> {
        let q = self.queue.borrow();
        let next = q.peek_next(Advance::Finished)?;
        (q.current().map(|c| c.id) != Some(next.id)).then(|| next.clone())
    }

    pub fn repeat(&self) -> RepeatMode {
        self.queue.borrow().repeat()
    }
    pub fn is_shuffled(&self) -> bool {
        self.queue.borrow().is_shuffled()
    }
    pub fn queue_entries(&self) -> Vec<QueueEntry> {
        self.queue.borrow().entries().to_vec()
    }
    pub fn queue_tracks(&self) -> Vec<Track> {
        self.queue
            .borrow()
            .entries()
            .iter()
            .map(|e| e.track.clone())
            .collect()
    }

    fn queue_changed(&self) {
        let objs: Vec<glib::BoxedAnyObject> = self
            .queue
            .borrow()
            .entries()
            .iter()
            .map(|e| glib::BoxedAnyObject::new(e.clone()))
            .collect();
        self.queue_store
            .splice(0, self.queue_store.n_items(), &objs);
        self.emit(AppEvent::QueueChanged);
        self.schedule_save();
    }

    /// Emit the initial state to freshly built UI.
    pub fn announce(&self) {
        self.emit(AppEvent::QueueChanged);
        self.emit(AppEvent::ModesChanged);
        if let Some((e, at)) = self.resume_point() {
            self.emit(AppEvent::NowPlaying(Some(e.clone())));
            let duration = e.track.duration_secs.map(|s| Duration::from_secs(s.into()));
            self.emit(AppEvent::Position {
                position: at,
                duration,
            });
        }
    }

    /// The default action: append to the queue. Starts playback only if nothing is playing
    /// and the queue was empty before, so the first add "just works".
    pub fn enqueue(&self, track: Track) {
        let was_idle = self.queue.borrow().is_empty() && self.current_entry().is_none();
        self.remember_tracks([&track]);
        let title = track.title.clone();
        let id = self.queue.borrow_mut().append(track);
        self.queue_changed();
        let weak = self.weak();
        self.toast(ToastSpec {
            title: format!("Queued “{title}”"),
            undo: Some(Rc::new(move || {
                if let Some(c) = weak.upgrade() {
                    c.remove_entry(id, false);
                }
            })),
            priority_high: false,
        });
        if was_idle {
            self.play_index(0);
        }
    }

    pub fn enqueue_many(&self, tracks: Vec<Track>, label: &str) {
        if tracks.is_empty() {
            self.toast(ToastSpec::info(format!("“{label}” has nothing playable")));
            return;
        }
        let was_idle = self.queue.borrow().is_empty();
        self.remember_tracks(tracks.iter());
        let n = tracks.len();
        let ids = self.queue.borrow_mut().append_many(tracks);
        self.queue_changed();
        let weak = self.weak();
        self.toast(ToastSpec {
            title: format!("Queued {n} items from “{label}”"),
            undo: Some(Rc::new(move || {
                if let Some(c) = weak.upgrade() {
                    for id in &ids {
                        c.queue.borrow_mut().remove(*id);
                    }
                    c.queue_changed();
                }
            })),
            priority_high: false,
        });
        if was_idle {
            self.play_index(0);
        }
    }

    pub fn play_next(&self, track: Track) {
        self.remember_tracks([&track]);
        let title = track.title.clone();
        self.queue.borrow_mut().play_next(track);
        self.queue_changed();
        self.toast(ToastSpec::info(format!("“{title}” plays next")));
        if self.current_entry().is_none()
            && self
                .last_state
                .get()
                .is_none_or(|s| s == PlaybackState::Stopped)
        {
            self.play_index(0);
        }
    }

    /// Insert a whole Collection right after the current entry, keeping its order.
    pub fn play_next_many(&self, tracks: Vec<Track>, label: &str) {
        if tracks.is_empty() {
            return;
        }
        self.remember_tracks(tracks.iter());
        let n = tracks.len();
        {
            let mut q = self.queue.borrow_mut();
            let start = q.current_index().map_or(0, |c| c + 1);
            for (i, t) in tracks.into_iter().enumerate() {
                q.insert(start + i, t);
            }
        }
        self.queue_changed();
        self.toast(ToastSpec::info(format!(
            "{n} items from “{label}” play next"
        )));
        if self.current_entry().is_none() {
            self.play_index(0);
        }
    }

    pub fn play_now(&self, track: Track) {
        self.remember_tracks([&track]);
        let id = self.queue.borrow_mut().play_next(track);
        self.queue_changed();
        let idx = self.queue.borrow().index_of(id);
        if let Some(i) = idx {
            self.play_index(i);
        }
    }

    /// Stop playback and invalidate any in-flight resolve/load.
    pub fn cancel_playback(&self) {
        self.play_generation.set(self.play_generation.get() + 1);
        self.player.stop();
    }

    pub fn remove_entry(&self, id: EntryId, offer_undo: bool) {
        let was_current = self.current_entry().is_some_and(|e| e.id == id);
        let Some((idx, entry)) = self.queue.borrow_mut().remove(id) else {
            return;
        };
        self.resolved.borrow_mut().remove(&id);
        self.queue_changed();
        if was_current {
            self.cancel_playback();
            self.emit(AppEvent::NowPlaying(None));
        }
        if offer_undo {
            let weak = self.weak();
            let title = entry.track.title.clone();
            self.toast(ToastSpec {
                title: format!("Removed “{title}”"),
                undo: Some(Rc::new(move || {
                    if let Some(c) = weak.upgrade() {
                        c.queue.borrow_mut().restore(idx, entry.clone());
                        c.queue_changed();
                    }
                })),
                priority_high: false,
            });
        }
    }

    pub fn clear_queue(&self) {
        let snapshot = self.queue.borrow().clone();
        if snapshot.is_empty() {
            return;
        }
        self.cancel_playback();
        self.queue.borrow_mut().clear();
        self.resolved.borrow_mut().clear();
        self.queue_changed();
        self.emit(AppEvent::NowPlaying(None));
        let weak = self.weak();
        self.toast(ToastSpec {
            title: "Queue cleared".into(),
            undo: Some(Rc::new(move || {
                if let Some(c) = weak.upgrade() {
                    let mut q = snapshot.clone();
                    // Restore order but don't resume playback automatically.
                    q.jump(usize::MAX);
                    *c.queue.borrow_mut() = q;
                    c.queue_changed();
                }
            })),
            priority_high: false,
        });
    }

    pub fn move_entry(&self, from: usize, to: usize) {
        if self.queue.borrow_mut().move_entry(from, to) {
            self.queue_changed();
            self.prefetch_next();
        }
    }

    pub fn set_shuffle(&self, on: bool) {
        self.queue.borrow_mut().set_shuffle(on, &mut rand::rng());
        self.queue_changed();
        self.emit(AppEvent::ModesChanged);
        self.prefetch_next();
    }

    pub fn set_repeat(&self, mode: RepeatMode) {
        self.queue.borrow_mut().set_repeat(mode);
        self.emit(AppEvent::ModesChanged);
        self.schedule_save();
    }

    fn schedule_save(&self) {
        if self.save_pending.replace(true) {
            return;
        }
        let weak = self.weak();
        glib::timeout_add_local_once(Duration::from_millis(400), move || {
            if let Some(c) = weak.upgrade() {
                c.save_pending.set(false);
                c.save_session();
            }
        });
    }

    fn save<T: serde::Serialize>(&self, name: &str, v: &T) {
        if let Some(store) = &self.store {
            if let Err(e) = store.save(name, v) {
                log::error!("saving {name} failed: {e}");
                self.toast_error(format!("Couldn’t save your {name}: {e}"));
            }
        }
    }

    /// Queue + resume point. Called shortly after every change and on quit.
    pub fn save_session(&self) {
        let position_secs = match self.player.state() {
            PlaybackState::Stopped => self
                .resume_at
                .get()
                .filter(|(id, _)| self.current_entry().is_some_and(|c| c.id == *id))
                .map_or(0, |(_, d)| d.as_secs()),
            _ => self.player.position().map_or(0, |p| p.as_secs()),
        };
        self.last_saved_position.set(position_secs);
        let session = Session {
            queue: self.queue.borrow().clone(),
            position_secs,
        };
        self.save("session", &session);
    }

    pub fn save_prefs(&self) {
        self.prefs.borrow_mut().volume = self.player.volume();
        let p = self.prefs.borrow().clone();
        self.save("prefs", &p);
    }

    fn mark_history_dirty(&self) {
        if self.history_dirty.replace(true) {
            return;
        }
        let weak = self.weak();
        glib::timeout_add_local_once(Duration::from_secs(2), move || {
            if let Some(c) = weak.upgrade() {
                c.flush_history();
            }
        });
    }

    fn flush_history(&self) {
        if self.history_dirty.replace(false) {
            let h = self.history.borrow().clone();
            self.save("search", &h);
        }
    }

    fn mark_seen_dirty(&self) {
        if self.seen_dirty.replace(true) {
            return;
        }
        let weak = self.weak();
        glib::timeout_add_local_once(Duration::from_secs(5), move || {
            if let Some(c) = weak.upgrade() {
                c.flush_seen();
            }
        });
    }

    fn flush_seen(&self) {
        if self.seen_dirty.replace(false) {
            let seen: SeenTracks = self.local.borrow().tracks().cloned().collect();
            self.save("seen", &seen);
        }
    }

    fn remember_tracks<'a>(&self, tracks: impl IntoIterator<Item = &'a Track>) {
        self.local.borrow_mut().extend(tracks);
        self.mark_seen_dirty();
    }

    fn restore_state(&self) {
        let Some(store) = &self.store else { return };
        let seen: SeenTracks = store.load("seen");
        self.local.borrow_mut().extend(seen.iter());
        let session: Session = store.load("session");
        self.local
            .borrow_mut()
            .extend(session.queue.entries().iter().map(|e| &e.track));
        if let Some(cur) = session
            .queue
            .current()
            .filter(|_| session.position_secs > 0)
        {
            self.resume_at
                .set(Some((cur.id, Duration::from_secs(session.position_secs))));
        }
        self.last_saved_position.set(session.position_secs);
        *self.queue.borrow_mut() = session.queue;
        let volume = self.prefs.borrow().volume;
        self.player.set_volume(volume.clamp(0.0, 1.0));
        let objs: Vec<glib::BoxedAnyObject> = self
            .queue
            .borrow()
            .entries()
            .iter()
            .map(|e| glib::BoxedAnyObject::new(e.clone()))
            .collect();
        self.queue_store
            .splice(0, self.queue_store.n_items(), &objs);
    }

    /// The restored current entry and resume point, for the Now Playing Bar at startup.
    pub fn resume_point(&self) -> Option<(QueueEntry, Duration)> {
        let e = self.current_entry()?;
        let at = self
            .resume_at
            .get()
            .filter(|(id, _)| *id == e.id)
            .map(|(_, d)| d)
            .unwrap_or_default();
        Some((e, at))
    }

    // ---------------------------------------------------------------- Playback

    fn wire_player(&self) {
        let weak = self.weak();
        self.player.connect_event(move |ev| {
            let Some(c) = weak.upgrade() else { return };
            match ev {
                PlayerEvent::State(s) => {
                    c.last_state.set(Some(*s));
                    if *s == PlaybackState::Playing {
                        c.failures.set(0);
                        c.retried_entry.set(None);
                        if let Some((id, at)) = c.resume_at.take() {
                            if c.current_entry().is_some_and(|e| e.id == id) {
                                c.seek(at);
                            }
                        }
                    }
                    if *s == PlaybackState::Paused {
                        c.save_session();
                    }
                    c.emit(AppEvent::State(*s));
                }
                PlayerEvent::Position { position, duration } => {
                    c.last_position.set((*position, *duration));
                    // Persist the resume point every ~5 s of playback.
                    if position.as_secs().abs_diff(c.last_saved_position.get()) >= 5 {
                        c.save_session();
                    }
                    c.emit(AppEvent::Position {
                        position: *position,
                        duration: *duration,
                    });
                }
                PlayerEvent::Finished => c.advance(Advance::Finished),
                PlayerEvent::Error(e) => {
                    let current = c.current_entry();
                    // Stream URLs can be rejected (HTTP 403) or expire; re-resolve once
                    // before giving up on the entry.
                    if let (Some(entry), banshee::player::PlayerError::Stream(_)) = (&current, e) {
                        if c.retried_entry.get() != Some(entry.id) {
                            log::info!(
                                "stream failed for “{}” ({e}); re-resolving once",
                                entry.track.title
                            );
                            c.retried_entry.set(Some(entry.id));
                            c.resolved.borrow_mut().remove(&entry.id);
                            // Continue from where the stream broke off.
                            let (pos, _) = c.last_position.get();
                            if pos > Duration::from_secs(5) {
                                c.resume_at.set(Some((entry.id, pos)));
                            }
                            c.start_entry(entry.clone());
                            return;
                        }
                    }
                    let title = current.map(|e| e.track.title).unwrap_or_default();
                    c.playback_failed(format!("Couldn’t play “{title}”: {e}"));
                }
                PlayerEvent::VideoPaintable(p) => c.emit(AppEvent::Video(p.clone())),
            }
        });
    }

    fn playback_failed(&self, msg: String) {
        self.toast_error(msg);
        let n = self.failures.get() + 1;
        self.failures.set(n);
        if n >= MAX_CONSECUTIVE_FAILURES {
            self.failures.set(0);
            self.player.stop();
            self.toast_error("Playback stopped after repeated failures");
            return;
        }
        // Skip to the next entry; with Repeat One don't loop on a broken item.
        let next = self.queue.borrow().peek_next(Advance::User).map(|e| e.id);
        let cur = self.current_entry().map(|e| e.id);
        if next.is_some() && next != cur {
            self.advance(Advance::User);
        } else {
            self.player.stop();
        }
    }

    pub fn play_index(&self, index: usize) {
        let entry = self.queue.borrow_mut().jump(index).cloned();
        let Some(entry) = entry else { return };
        self.start_entry(entry);
    }

    pub fn play_entry_id(&self, id: EntryId) {
        let idx = self.queue.borrow().index_of(id);
        if let Some(i) = idx {
            self.play_index(i);
        }
    }

    pub fn advance(&self, why: Advance) {
        let entry = self.queue.borrow_mut().advance(why).cloned();
        match entry {
            Some(e) => self.start_entry(e),
            None => {
                self.player.stop();
                self.emit(AppEvent::QueueChanged);
            }
        }
    }

    pub fn previous(&self) {
        // Classic behaviour: restart the current item if more than 3 s in.
        if self
            .player
            .position()
            .is_some_and(|p| p > Duration::from_secs(3))
        {
            self.seek(Duration::ZERO);
            return;
        }
        let entry = self.queue.borrow_mut().previous().cloned();
        if let Some(e) = entry {
            self.start_entry(e);
        }
    }

    pub fn toggle_play(&self) {
        match self.player.state() {
            PlaybackState::Stopped => {
                let idx = self.queue.borrow().resume_index();
                self.play_index(idx);
            }
            _ => self.player.toggle(),
        }
    }

    fn start_entry(&self, entry: QueueEntry) {
        if self.resume_at.get().is_some_and(|(id, _)| id != entry.id) {
            self.resume_at.set(None);
        }
        // Stop the old item first so its EOS/errors/position can't act on the new entry
        // while this one resolves.
        self.player.stop();
        let generation = self.play_generation.get() + 1;
        self.play_generation.set(generation);
        self.emit(AppEvent::NowPlaying(Some(entry.clone())));
        self.emit(AppEvent::QueueChanged);
        self.emit(AppEvent::State(PlaybackState::Loading));
        let cached = self
            .resolved
            .borrow()
            .get(&entry.id)
            .filter(|(t, _)| t.elapsed() < RESOLVED_TTL)
            .map(|(_, r)| r.clone());
        let weak = self.weak();
        glib::spawn_future_local(async move {
            let Some(c) = weak.upgrade() else { return };
            let resolved = match cached {
                Some(r) => Ok(r),
                None => {
                    let busy = c.busy_guard();
                    let fut = c.source(entry.track.source).resolve(entry.track.clone());
                    drop(c);
                    let r = run(fut).await;
                    drop(busy);
                    r.map_err(SourceError::Unavailable).and_then(|r| r)
                }
            };
            let Some(c) = weak.upgrade() else { return };
            if c.play_generation.get() != generation {
                return; // user moved on while resolving
            }
            match resolved {
                Ok(r) => {
                    c.apply_resolved_metadata(entry.id, &r);
                    let session = match &r.playable {
                        Playable::Spotify { .. } => match run(c.spotify.session()).await {
                            _ if c.play_generation.get() != generation => return,
                            Ok(Ok(s)) => Some(s),
                            Ok(Err(e)) => {
                                c.playback_failed(describe(SourceKind::Spotify, &e));
                                return;
                            }
                            Err(e) => {
                                c.playback_failed(format!("Spotify: {e}"));
                                return;
                            }
                        },
                        Playable::Uri { .. } => None,
                    };
                    if c.play_generation.get() != generation {
                        return;
                    }
                    c.resolved.borrow_mut().remove(&entry.id);
                    c.player.load(r.playable, session);
                    c.prefetch_next();
                }
                Err(e) => {
                    c.playback_failed(format!("Couldn’t play “{}”: {e}", entry.track.title));
                }
            }
        });
    }

    fn apply_resolved_metadata(&self, id: EntryId, r: &Resolved) {
        let needs = self
            .queue
            .borrow()
            .entries()
            .iter()
            .find(|e| e.id == id)
            .is_some_and(|e| {
                (e.track.artist_id.is_none() && r.artist_id.is_some())
                    || (e.track.duration_secs.is_none() && r.duration_secs.is_some())
            });
        if !needs {
            return;
        }
        let updated = self
            .queue
            .borrow_mut()
            .update_track(id, |t| {
                if t.artist_id.is_none() {
                    t.artist_id = r.artist_id.clone();
                }
                if t.duration_secs.is_none() {
                    t.duration_secs = r.duration_secs;
                }
            })
            .cloned();
        if let Some(entry) = updated {
            self.queue_changed();
            if self.current_entry().is_some_and(|c| c.id == id) {
                self.emit(AppEvent::NowPlaying(Some(entry)));
            }
        }
    }

    /// Resolve the next entry ahead of time so the transition is gapless-ish.
    fn prefetch_next(&self) {
        let next = self.queue.borrow().peek_next(Advance::Finished).cloned();
        let Some(next) = next else { return };
        if self.current_entry().is_some_and(|c| c.id == next.id)
            || self.resolved.borrow().contains_key(&next.id)
        {
            return;
        }
        let weak = self.weak();
        let src = self.source(next.track.source);
        glib::spawn_future_local(async move {
            let r = run(src.resolve(next.track.clone())).await;
            let Some(c) = weak.upgrade() else { return };
            match r {
                Ok(Ok(res)) => {
                    let mut map = c.resolved.borrow_mut();
                    map.retain(|_, (t, _)| t.elapsed() < RESOLVED_TTL);
                    map.insert(next.id, (Instant::now(), res));
                }
                // Failure is reported when the entry actually plays.
                Ok(Err(e)) => log::info!("pre-resolve of “{}” failed: {e}", next.track.title),
                Err(e) => log::warn!("pre-resolve task failed: {e}"),
            }
        });
    }

    pub fn set_volume(&self, v: f64) {
        self.player.set_volume(v.clamp(0.0, 1.0));
        self.emit(AppEvent::ModesChanged);
        self.update_prefs(move |p| p.volume = v.clamp(0.0, 1.0));
    }

    /// Change a preference and save it shortly after (not only on window close, which
    /// logout/SIGTERM never emits).
    pub fn update_prefs(&self, f: impl FnOnce(&mut Prefs) + 'static) {
        // Called from GTK notify handlers: a caller up the stack may still hold a borrow
        // (a panic here would abort inside a signal), so defer instead of panicking.
        let Ok(mut prefs) = self.prefs.try_borrow_mut() else {
            log::debug!("prefs busy; deferring update");
            let weak = self.weak();
            glib::idle_add_local_once(move || {
                if let Some(c) = weak.upgrade() {
                    c.update_prefs(f);
                }
            });
            return;
        };
        f(&mut prefs);
        drop(prefs);
        if self.prefs_save_pending.replace(true) {
            return;
        }
        let weak = self.weak();
        glib::timeout_add_local_once(Duration::from_millis(600), move || {
            if let Some(c) = weak.upgrade() {
                c.prefs_save_pending.set(false);
                c.save_prefs();
            }
        });
    }

    /// Tell Discord what is playing now (or nothing).
    fn push_presence(&self) {
        let now = self.current_entry().map(|e| {
            let playing = matches!(
                self.player.state(),
                PlaybackState::Playing | PlaybackState::Buffering(_) | PlaybackState::Loading
            );
            banshee::discord::NowPlaying {
                url: e.track.web_url(),
                title: e.track.title,
                artist: e.track.artist,
                album: e.track.album,
                artwork_url: e
                    .track
                    .thumbnail_url
                    .map(|u| banshee::artwork::sized_thumbnail(&u, 512)),
                source: e.track.source,
                duration_secs: e.track.duration_secs,
                position_secs: self.player.position().unwrap_or_default().as_secs(),
                playing,
            }
        });
        self.presence.update(now);
    }

    /// Turn Discord Rich Presence on/off or change the application ID.
    pub fn set_discord(&self, enabled: bool, client_id: Option<String>) {
        let id = client_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let saved_id = id.clone();
        self.update_prefs(move |p| {
            p.discord_presence = enabled;
            p.discord_client_id = saved_id;
        });
        self.presence.configure(enabled, id);
        self.push_presence();
    }

    pub fn seek(&self, to: Duration) {
        self.player.seek(to);
        self.push_presence();
        if let Some(m) = self.mpris.borrow().as_ref() {
            m.seeked(to.as_micros() as i64);
        }
    }

    pub fn position(&self) -> (Duration, Option<Duration>) {
        self.last_position.get()
    }

    pub fn state(&self) -> PlaybackState {
        self.player.state()
    }

    // ---------------------------------------------------------------- Search

    /// Instant local results for the current keystroke.
    pub fn search_local(&self, query: &str, filter: SearchFilter) -> Vec<Track> {
        self.local
            .borrow()
            .search(&mut self.scorer.borrow_mut(), query, filter, 30)
    }

    /// Fresh in-memory results, if any (no network needed).
    pub fn memoised(
        &self,
        source: SourceKind,
        filter: SearchFilter,
        query: &str,
    ) -> Option<Vec<SearchItem>> {
        self.search_memo
            .borrow_mut()
            .get(&(source, filter, banshee::persist::normalise_query(query)))
            .cloned()
    }

    /// Results remembered from any earlier session (shown instantly, then refreshed).
    pub fn remembered(
        &self,
        source: SourceKind,
        filter: SearchFilter,
        query: &str,
    ) -> Option<Vec<SearchItem>> {
        self.history.borrow().results_for(source, filter, query)
    }

    /// The user acted on a result for this query: keep it in Recent Searches.
    pub fn remember_query(&self, query: &str) {
        self.history.borrow_mut().remember_query(query);
        self.mark_history_dirty();
        self.emit(AppEvent::HistoryChanged);
    }

    pub fn forget_query(&self, query: &str) {
        self.history.borrow_mut().forget_query(query);
        self.mark_history_dirty();
        self.emit(AppEvent::HistoryChanged);
    }

    pub fn set_last_search(&self, query: &str, filter: SearchFilter) {
        let mut h = self.history.borrow_mut();
        if h.last_query != query || h.last_filter != filter {
            h.last_query = query.to_string();
            h.last_filter = filter;
            drop(h);
            self.mark_history_dirty();
        }
    }

    pub fn remember_search(
        &self,
        source: SourceKind,
        filter: SearchFilter,
        query: &str,
        items: &[SearchItem],
    ) {
        self.remember_tracks(items.iter().filter_map(|i| match i {
            SearchItem::Track(t) => Some(t),
            SearchItem::Collection(_) => None,
        }));
        self.search_memo.borrow_mut().insert(
            (source, filter, banshee::persist::normalise_query(query)),
            items.to_vec(),
            1,
        );
        self.history
            .borrow_mut()
            .store_results(source, filter, query, items);
        self.mark_history_dirty();
    }

    pub fn rank(
        &self,
        query: &str,
        filter: SearchFilter,
        local: &[Track],
        remote: &[Vec<SearchItem>],
    ) -> Vec<Ranked> {
        fuzzy::merge(
            &mut self.scorer.borrow_mut(),
            query,
            filter,
            local,
            remote,
            80,
        )
    }

    /// Keep only items whose text fuzzy-matches `query` (used for results of an older query).
    pub fn still_matching(&self, query: &str, items: Vec<SearchItem>) -> Vec<SearchItem> {
        let mut scorer = self.scorer.borrow_mut();
        items
            .into_iter()
            .filter(|i| {
                let hay = match i {
                    SearchItem::Track(t) => t.haystack(),
                    SearchItem::Collection(c) => format!("{} {}", c.title, c.subtitle),
                };
                scorer.score(query, &hay).is_some()
            })
            .collect()
    }

    /// Sources to query for search right now.
    pub fn search_sources(&self) -> Vec<SourceKind> {
        let mut v = vec![SourceKind::YouTubeMusic];
        if self.spotify.is_signed_in() {
            v.push(SourceKind::Spotify);
        }
        v
    }

    // ---------------------------------------------------------------- Library & collections

    fn cache_get<T: serde::de::DeserializeOwned>(
        &self,
        ns: &str,
        key: &str,
        ttl: Duration,
    ) -> Lookup<T> {
        match &self.cache {
            Some(c) => c.get(ns, key, ttl),
            None => Lookup::Missing,
        }
    }

    fn cache_put<T: serde::Serialize>(&self, ns: &str, key: &str, v: &T) {
        if let Some(c) = &self.cache {
            if let Err(e) = c.put(ns, key, v) {
                log::warn!("cache write failed: {e}");
            }
        }
    }

    /// Stale-while-revalidate library load. `on_update` is called with each state change.
    pub fn load_library(
        &self,
        source: SourceKind,
        force: bool,
        on_update: impl Fn(LibraryState) + 'static,
    ) {
        self.load_library_rc(source, force, Rc::new(on_update));
    }

    fn load_library_rc(
        &self,
        source: SourceKind,
        force: bool,
        on_update: Rc<dyn Fn(LibraryState)>,
    ) {
        let src = self.source(source);
        if !src.is_signed_in() {
            on_update(LibraryState::SignedOut);
            return;
        }
        let ns = source.slug();
        let cached: Lookup<Vec<LibrarySection>> = self.cache_get(ns, "library", LIBRARY_TTL);
        let needs = force || cached.needs_fetch();
        let have = cached.value();
        match &have {
            Some(s) => on_update(LibraryState::Ready {
                sections: s.clone(),
                refreshing: needs,
            }),
            None => on_update(LibraryState::Loading),
        }
        // Coalesce: one library fetch per source at a time.
        if !needs || !self.library_inflight.borrow_mut().insert(source) {
            return;
        }
        let weak = self.weak();
        let epoch = self.accounts_epoch.get();
        glib::spawn_future_local(async move {
            let Some(c) = weak.upgrade() else { return };
            let busy = c.busy_guard();
            drop(c);
            let r = run(src.library()).await;
            drop(busy);
            let Some(c) = weak.upgrade() else { return };
            if c.accounts_epoch.get() != epoch {
                // Signed out or switched account meanwhile; accounts_changed already
                // cleared the in-flight set, so don't touch the new fetch's marker.
                return;
            }
            c.library_inflight.borrow_mut().remove(&source);
            match r.map_err(SourceError::Unavailable).and_then(|r| r) {
                Ok(sections) => {
                    c.cache_put(ns, "library", &sections);
                    on_update(LibraryState::Ready {
                        sections,
                        refreshing: false,
                    });
                }
                Err(SourceError::AuthRequired(why)) if source == SourceKind::YouTubeMusic => {
                    // The browser rotated the session cookies: the source re-imports once
                    // from the browser profile, then we retry.
                    let fail = {
                        let (on_update, weak) = (on_update.clone(), weak.clone());
                        let msg = describe(source, &SourceError::AuthRequired(why.clone()));
                        move || match (&have, weak.upgrade()) {
                            (Some(sections), Some(c)) => {
                                c.toast_error(format!("Couldn’t refresh the library — {msg}"));
                                on_update(LibraryState::Ready {
                                    sections: sections.clone(),
                                    refreshing: false,
                                });
                            }
                            _ => on_update(LibraryState::Failed(msg.clone())),
                        }
                    };
                    let busy = c.busy_guard();
                    let refresh = c.youtube.refresh_session();
                    drop(c);
                    let refreshed = run(refresh)
                        .await
                        .map_err(SourceError::Unavailable)
                        .and_then(|r| r);
                    drop(busy);
                    let Some(c) = weak.upgrade() else { return };
                    match refreshed {
                        Ok(true) => {
                            c.toast(ToastSpec::info(
                                "YouTube Music session refreshed from your browser",
                            ));
                            c.search_memo.borrow_mut().clear();
                            c.load_library_rc(source, true, on_update);
                        }
                        Ok(false) | Err(_) => fail(),
                    }
                }
                Err(e) => {
                    let msg = describe(source, &e);
                    match have {
                        Some(s) => {
                            c.toast_error(format!("Couldn’t refresh the library — {msg}"));
                            on_update(LibraryState::Ready {
                                sections: s,
                                refreshing: false,
                            });
                        }
                        None => on_update(LibraryState::Failed(msg)),
                    }
                }
            }
        });
    }

    /// Home shelves: cached (stale-while-revalidate, 30 min), refreshed in the background.
    pub fn load_home(&self, force: bool, on_update: impl Fn(HomeState) + 'static) {
        let cached: Lookup<Vec<banshee::model::HomeShelf>> =
            self.cache_get("youtube", "home", HOME_TTL);
        let needs = force || cached.needs_fetch();
        match cached.value() {
            Some(s) if !s.is_empty() => {
                self.remember_tracks(s.iter().flat_map(|sh| sh.items.iter()).filter_map(
                    |i| match i {
                        SearchItem::Track(t) => Some(t),
                        SearchItem::Collection(_) => None,
                    },
                ));
                on_update(HomeState::Ready(s))
            }
            _ => on_update(HomeState::Loading),
        }
        if !needs {
            return;
        }
        let weak = self.weak();
        let yt = self.youtube.clone();
        glib::spawn_future_local(async move {
            let Some(c) = weak.upgrade() else { return };
            let busy = c.busy_guard();
            drop(c);
            let mut r = run(yt.home())
                .await
                .map_err(SourceError::Unavailable)
                .and_then(|r| r);
            if matches!(r, Err(SourceError::AuthRequired(_))) {
                if let Ok(Ok(true)) = run(yt.refresh_session()).await {
                    r = run(yt.home())
                        .await
                        .map_err(SourceError::Unavailable)
                        .and_then(|r| r);
                }
            }
            drop(busy);
            let Some(c) = weak.upgrade() else { return };
            match r {
                Ok(shelves) => {
                    c.cache_put("youtube", "home", &shelves);
                    c.remember_tracks(shelves.iter().flat_map(|sh| sh.items.iter()).filter_map(
                        |i| match i {
                            SearchItem::Track(t) => Some(t),
                            SearchItem::Collection(_) => None,
                        },
                    ));
                    on_update(HomeState::Ready(shelves));
                }
                Err(e) => {
                    let msg = describe(SourceKind::YouTubeMusic, &e);
                    let cached: Lookup<Vec<banshee::model::HomeShelf>> =
                        c.cache_get("youtube", "home", HOME_TTL);
                    match cached.value() {
                        Some(s) if !s.is_empty() => {
                            c.toast_error(format!("Couldn’t refresh Home — {msg}"))
                        }
                        _ => on_update(HomeState::Failed(msg)),
                    }
                }
            }
        });
    }

    /// Cached collection contents; `on_done` gets `Ok(tracks)` (possibly twice: stale then fresh).
    pub fn load_collection(
        &self,
        collection: Collection,
        force: bool,
        on_done: impl Fn(Result<Vec<Track>, String>, bool) + 'static,
    ) {
        let ns = collection.source.slug();
        let key = collection.key();
        let cached: Lookup<Vec<Track>> = self.cache_get(ns, &key, COLLECTION_TTL);
        let needs = force || cached.needs_fetch();
        if let Some(v) = cached.value() {
            self.remember_tracks(v.iter());
            on_done(Ok(v), needs);
        }
        if !needs {
            return;
        }
        let src = self.source(collection.source);
        let weak = self.weak();
        glib::spawn_future_local(async move {
            let Some(c) = weak.upgrade() else { return };
            let busy = c.busy_guard();
            drop(c);
            let mut r = run(src.collection(collection.clone()))
                .await
                .map_err(SourceError::Unavailable)
                .and_then(|r| r);
            // An expired YouTube session: refresh it from the browser once and retry.
            if matches!(r, Err(SourceError::AuthRequired(_)))
                && collection.source == SourceKind::YouTubeMusic
            {
                let Some(c) = weak.upgrade() else { return };
                let refresh = c.youtube.refresh_session();
                drop(c);
                if let Ok(Ok(true)) = run(refresh).await {
                    r = run(src.collection(collection.clone()))
                        .await
                        .map_err(SourceError::Unavailable)
                        .and_then(|r| r);
                }
            }
            drop(busy);
            let Some(c) = weak.upgrade() else { return };
            match r {
                Ok(tracks) => {
                    c.cache_put(ns, &key, &tracks);
                    c.remember_tracks(tracks.iter());
                    on_done(Ok(tracks), false);
                }
                Err(e) => on_done(Err(describe(collection.source, &e)), false),
            }
        });
    }

    /// Expand a Collection and append all of it (the `+` on playlists/albums/podcasts).
    pub fn enqueue_collection(&self, collection: Collection) {
        let weak = self.weak();
        let label = collection.title.clone();
        let done = Rc::new(Cell::new(false));
        self.load_collection(collection, false, move |r, more_coming| {
            let Some(c) = weak.upgrade() else { return };
            if done.get() {
                return;
            }
            match r {
                Ok(tracks) => {
                    done.set(true);
                    let _ = more_coming;
                    c.enqueue_many(tracks, &label);
                }
                Err(e) => {
                    done.set(true);
                    c.toast_error(format!("Couldn’t queue “{label}”: {e}"));
                }
            }
        });
    }

    pub fn purge_source_cache(&self, source: SourceKind) {
        if let Some(c) = &self.cache {
            if let Err(e) = c.purge_namespace(source.slug()) {
                log::warn!("purging {source} cache: {e}");
            }
        }
        self.search_memo.borrow_mut().clear();
        self.history.borrow_mut().purge_source(source);
        self.mark_history_dirty();
    }

    pub fn accounts_changed(&self) {
        self.accounts_epoch.set(self.accounts_epoch.get() + 1);
        self.library_inflight.borrow_mut().clear();
        self.search_memo.borrow_mut().clear();
        self.emit(AppEvent::AccountsChanged);
    }

    pub fn raise(&self) {
        self.emit(AppEvent::Raise);
    }

    // ---------------------------------------------------------------- Links (MPRIS OpenUri)

    /// Queue a YouTube/Spotify link (MPRIS `OpenUri`, `banshee <link>`). Links are queued
    /// in the order they were opened, even though lookups finish in any order.
    pub fn open_uri(&self, uri: &str) {
        let lookup: futures::future::BoxFuture<'static, Result<Track, String>> =
            if let Some(link) = banshee::sources::youtube::parse_video_url(uri) {
                let yt = self.youtube.clone();
                Box::pin(async move {
                    run(yt.lookup_video(link.id, link.kind))
                        .await
                        .map_err(SourceError::Unavailable)
                        .and_then(|r| r)
                        .map_err(|e| describe(SourceKind::YouTubeMusic, &e))
                })
            } else if let Some((kind, id)) = banshee::sources::spotify::track_from_url(uri) {
                let sp = self.spotify.clone();
                Box::pin(async move {
                    run(sp.lookup(kind, id))
                        .await
                        .map_err(SourceError::Unavailable)
                        .and_then(|r| r)
                        .map_err(|e| describe(SourceKind::Spotify, &e))
                })
            } else {
                self.toast_error(format!("Unsupported link: {uri}"));
                return;
            };
        let seq = self.link_seq.get();
        self.link_seq.set(seq + 1);
        let weak = self.weak();
        glib::spawn_future_local(async move {
            let r = lookup.await;
            let Some(c) = weak.upgrade() else { return };
            c.link_ready.borrow_mut().insert(seq, r);
            // Flush every consecutive finished lookup, oldest first.
            loop {
                let next = c.link_next.get();
                let Some(r) = c.link_ready.borrow_mut().remove(&next) else {
                    break;
                };
                c.link_next.set(next + 1);
                match r {
                    Ok(track) => c.enqueue(track),
                    Err(e) => c.toast_error(format!("Couldn’t open link — {e}")),
                }
            }
        });
    }

    // ---------------------------------------------------------------- GC

    fn start_gc(&self) {
        let weak = self.weak();
        glib::timeout_add_local(GC_INTERVAL, move || {
            let Some(c) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            c.collect_garbage();
            glib::ControlFlow::Continue
        });
    }

    pub fn collect_garbage(&self) {
        self.artwork.collect();
        self.search_memo.borrow_mut().evict_expired();
        self.resolved
            .borrow_mut()
            .retain(|_, (t, _)| t.elapsed() < RESOLVED_TTL);
        if let Some(c) = &self.cache {
            c.prune_older_than(
                Duration::from_secs(30 * 24 * 3600),
                std::time::SystemTime::now(),
            );
        }
        memory::trim_heap();
        if let Some(rss) = memory::rss_bytes() {
            log::info!(
                "gc: rss {} MiB, artwork {} textures / {} MiB, local index {}",
                rss / (1024 * 1024),
                self.artwork.memory_len(),
                self.artwork.memory_bytes() / (1024 * 1024),
                self.local.borrow().len()
            );
        }
    }

    pub fn shutdown(&self) {
        self.save_session();
        self.save_prefs();
        self.history_dirty.set(true);
        self.flush_history();
        self.seen_dirty.set(true);
        self.flush_seen();
        self.player.stop();
        self.mpris.borrow_mut().take();
    }
}

pub struct BusyGuard(Weak<Controller>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Some(c) = self.0.upgrade() {
            c.unbusy();
        }
    }
}

struct MprisBridge(Weak<Controller>);

impl MprisTarget for MprisBridge {
    fn snapshot(&self) -> Snapshot {
        let Some(c) = self.0.upgrade() else {
            return Snapshot {
                status: mpris::Status::Stopped,
                current: None,
                length_us: None,
                position_us: 0,
                volume: 1.0,
                can_next: false,
                can_previous: false,
                shuffle: false,
                repeat: RepeatMode::Off,
                art_url: None,
            };
        };
        let q = c.queue.borrow();
        let cur = q.current().cloned();
        let (pos, dur) = (c.player.position().unwrap_or_default(), c.player.duration());
        let status = match c.player.state() {
            PlaybackState::Playing | PlaybackState::Buffering(_) | PlaybackState::Loading => {
                mpris::Status::Playing
            }
            PlaybackState::Paused => mpris::Status::Paused,
            PlaybackState::Stopped => mpris::Status::Stopped,
        };
        let length_us = dur
            .or_else(|| {
                cur.as_ref()
                    .and_then(|e| e.track.duration_secs)
                    .map(|s| Duration::from_secs(s.into()))
            })
            .map(|d| d.as_micros() as i64);
        Snapshot {
            status,
            art_url: cur.as_ref().and_then(|e| e.track.thumbnail_url.clone()),
            current: cur.map(|e| (e.id, e.track)),
            length_us,
            position_us: pos.as_micros() as i64,
            volume: c.player.volume(),
            can_next: q.peek_next(Advance::User).is_some(),
            can_previous: !q.is_empty(),
            shuffle: q.is_shuffled(),
            repeat: q.repeat(),
        }
    }

    fn command(&self, cmd: Command) {
        let Some(c) = self.0.upgrade() else { return };
        match cmd {
            Command::Raise => c.raise(),
            Command::Quit => c.app.quit(),
            // Play never pauses (MPRIS spec), even while loading or buffering.
            Command::Play => match c.player.state() {
                PlaybackState::Stopped => c.toggle_play(),
                PlaybackState::Paused => c.player.play(),
                _ => {}
            },
            Command::Pause => c.player.pause(),
            Command::PlayPause => c.toggle_play(),
            Command::Stop => c.cancel_playback(),
            Command::Next => c.advance(Advance::User),
            Command::Previous => c.previous(),
            Command::Seek(off) => {
                let pos = c.player.position().unwrap_or_default().as_micros() as i64 + off;
                let pos = pos.max(0) as u64;
                if let Some(d) = c.player.duration() {
                    if pos > d.as_micros() as u64 {
                        c.advance(Advance::User);
                        return;
                    }
                }
                c.seek(Duration::from_micros(pos));
            }
            Command::SetPosition {
                entry_id,
                position_us,
            } => {
                if c.current_entry().is_some_and(|e| e.id == entry_id) && position_us >= 0 {
                    c.seek(Duration::from_micros(position_us as u64));
                }
            }
            Command::OpenUri(u) => c.open_uri(&u),
            Command::SetVolume(v) => c.set_volume(v),
            Command::SetShuffle(s) => c.set_shuffle(s),
            Command::SetRepeat(r) => c.set_repeat(r),
        }
    }
}
