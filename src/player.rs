//! Player Core (ADR 0008, ADR 0010): a main-thread object that plays one `Playable` at a
//! time and reports progress as `PlayerEvent`s.
//!
//! - `Playable::Uri` → `playbin3` (audio-only unless the item is a video).
//! - `Playable::Spotify` → librespot → `appsrc` pipeline (see `spotify`).
//!
//! Every item gets a fresh pipeline; the previous one is set to NULL and dropped, which frees
//! decoder and queue buffers. The bus watch and position ticker live exactly as long as the
//! pipeline. Nothing here waits on GStreamer state changes: they complete asynchronously and
//! are observed on the bus.

mod spotify;
mod uri;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gst::glib;
use gst::prelude::*;
use gtk::gdk;

use crate::model::Playable;
use spotify::{SpotifyEvent, SpotifyPlayback};

const TICK_INTERVAL: Duration = Duration::from_millis(250);
/// Teardowns slower than this are logged; they run on the main thread.
const SLOW_TEARDOWN: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Loading,
    Playing,
    Paused,
    /// Network buffering, in percent.
    Buffering(u8),
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    State(PlaybackState),
    Position {
        position: Duration,
        duration: Option<Duration>,
    },
    /// The current item played to its end. The pipeline is already gone.
    Finished,
    /// The current item failed. The pipeline is already gone.
    Error(PlayerError),
    /// The video output to show, or `None` when no video is playing.
    VideoPaintable(Option<gdk::Paintable>),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlayerError {
    #[error("A GStreamer plugin is missing: {0}")]
    MissingPlugin(String),
    #[error("Could not play the stream: {0}")]
    Stream(String),
    #[error("Spotify playback failed: {0}")]
    Spotify(String),
    #[error("Playback pipeline error: {0}")]
    Pipeline(String),
}

/// Aborts a main-context task when dropped (glib `JoinHandle`s detach on drop).
struct TaskGuard(glib::JoinHandle<()>);

impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

enum Backend {
    Uri { video: bool },
    Spotify(SpotifyPlayback),
}

/// The pipeline of the current item plus everything whose lifetime is tied to it.
struct Active {
    pipeline: gst::Element,
    backend: Backend,
    bus_watch: Option<gst::bus::BusWatchGuard>,
    spotify_events: Option<TaskGuard>,
    is_live: bool,
    buffering: bool,
    pending_seek: Option<Duration>,
    missing_plugins: Vec<String>,
}

impl Active {
    fn new(pipeline: gst::Element, backend: Backend) -> Self {
        Self {
            pipeline,
            backend,
            bus_watch: None,
            spotify_events: None,
            is_live: false,
            buffering: false,
            pending_seek: None,
            missing_plugins: Vec::new(),
        }
    }

    fn position(&self) -> Option<Duration> {
        match &self.backend {
            Backend::Uri { .. } => self
                .pipeline
                .query_position::<gst::ClockTime>()
                .map(|t| Duration::from_nanos(t.nseconds())),
            Backend::Spotify(s) => s.position(&self.pipeline),
        }
    }

    fn duration(&self) -> Option<Duration> {
        match &self.backend {
            Backend::Uri { .. } => self
                .pipeline
                .query_duration::<gst::ClockTime>()
                .map(|t| Duration::from_nanos(t.nseconds())),
            Backend::Spotify(s) => s.duration(),
        }
    }

    fn apply_volume(&self, volume: f64, muted: bool) {
        match &self.backend {
            Backend::Uri { .. } => {
                self.pipeline.set_property("volume", volume);
                self.pipeline.set_property("mute", muted);
            }
            Backend::Spotify(s) => s.set_volume(volume, muted),
        }
    }

    fn seek_now(&mut self, to: Duration) -> bool {
        let flags = match self.backend {
            Backend::Uri { video: true } => {
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT | gst::SeekFlags::SNAP_NEAREST
            }
            _ => gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
        };
        self.pipeline
            .seek_simple(
                flags,
                gst::ClockTime::from_nseconds(u64::try_from(to.as_nanos()).unwrap_or(u64::MAX)),
            )
            .is_ok()
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        // Detach from the main loop first so nothing from the dying pipeline reaches us.
        self.spotify_events.take();
        self.bus_watch.take();
        if let Backend::Spotify(s) = &self.backend {
            s.close();
        }
        let started = Instant::now();
        if let Err(e) = self.pipeline.set_state(gst::State::Null) {
            log::warn!("pipeline did not shut down cleanly: {e}");
        }
        let took = started.elapsed();
        if took > SLOW_TEARDOWN {
            log::debug!("pipeline teardown took {took:?}");
        }
    }
}

type Handler = Rc<dyn Fn(&PlayerEvent)>;

struct Inner {
    playbin: &'static str,
    audio_sink: String,
    video_sink: Option<String>,
    state: Cell<PlaybackState>,
    /// What the user asked for; buffering and preroll pause the pipeline underneath it.
    want_playing: Cell<bool>,
    volume: Cell<f64>,
    muted: Cell<bool>,
    /// Bumped on every teardown; callbacks carrying an older value are stale.
    generation: Cell<u64>,
    active: RefCell<Option<Active>>,
    /// Snapshot-on-emit: handlers may connect more handlers while being called.
    handlers: RefCell<Rc<Vec<Handler>>>,
    ticker: RefCell<Option<TaskGuard>>,
    /// Every pipeline built so far, to prove that dropped ones were actually disposed.
    pipelines: RefCell<Vec<glib::WeakRef<gst::Element>>>,
    paintable_shown: Cell<bool>,
    last_position: Cell<Option<(Duration, Option<Duration>)>>,
    warned_no_video: Cell<bool>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.ticker.get_mut().take();
        self.active.get_mut().take();
    }
}

/// Player Core. Cheap to clone; all clones share one player. Main thread only.
#[derive(Clone)]
pub struct PlayerCore {
    inner: Rc<Inner>,
}

fn init_gstreamer() -> Result<(), PlayerError> {
    gst::init()
        .map_err(|e| PlayerError::Pipeline(format!("GStreamer could not be initialised: {e}")))
}

fn factory_exists(name: &str) -> bool {
    gst::ElementFactory::find(name).is_some()
}

impl PlayerCore {
    /// Player with the desktop outputs: `autoaudiosink` and, when installed,
    /// `gtk4paintablesink` for videos (otherwise videos play audio-only).
    /// `BANSHEE_AUDIO_SINK` overrides the audio output (gst-launch syntax, e.g.
    /// `pulsesink device=…` or `fakesink sync=true` for silent test runs).
    pub fn new() -> Result<Self, PlayerError> {
        init_gstreamer()?;
        let video = if factory_exists(uri::PAINTABLE_SINK) {
            Some(uri::PAINTABLE_SINK)
        } else {
            log::warn!(
                "{} is not installed; videos will play audio-only",
                uri::PAINTABLE_SINK
            );
            None
        };
        let audio = std::env::var("BANSHEE_AUDIO_SINK")
            .ok()
            .filter(|s| !s.trim().is_empty());
        Self::with_sinks(audio.as_deref().unwrap_or("autoaudiosink"), video)
    }

    /// Player with explicit output descriptions in `gst-launch` syntax
    /// (e.g. `"fakesink sync=true"` in tests).
    pub fn with_sinks(audio_sink: &str, video_sink: Option<&str>) -> Result<Self, PlayerError> {
        init_gstreamer()?;
        let playbin = if factory_exists("playbin3") {
            "playbin3"
        } else if factory_exists("playbin") {
            log::warn!("playbin3 is unavailable; falling back to playbin");
            "playbin"
        } else {
            return Err(PlayerError::MissingPlugin(
                "playbin3 (gst-plugins-base)".into(),
            ));
        };
        gst::parse::bin_from_description(audio_sink, true).map_err(|e| {
            PlayerError::Pipeline(format!("invalid audio output “{audio_sink}”: {e}"))
        })?;
        if let Some(desc) = video_sink {
            if desc.trim() == uri::PAINTABLE_SINK {
                if !factory_exists(uri::PAINTABLE_SINK) {
                    return Err(PlayerError::MissingPlugin(format!(
                        "GStreamer element “{}”",
                        uri::PAINTABLE_SINK
                    )));
                }
            } else {
                gst::parse::bin_from_description(desc, true).map_err(|e| {
                    PlayerError::Pipeline(format!("invalid video output “{desc}”: {e}"))
                })?;
            }
        }
        Ok(Self {
            inner: Rc::new(Inner {
                playbin,
                audio_sink: audio_sink.to_owned(),
                video_sink: video_sink.map(str::to_owned),
                state: Cell::new(PlaybackState::Stopped),
                want_playing: Cell::new(false),
                volume: Cell::new(1.0),
                muted: Cell::new(false),
                generation: Cell::new(0),
                active: RefCell::new(None),
                handlers: RefCell::new(Rc::new(Vec::new())),
                ticker: RefCell::new(None),
                pipelines: RefCell::new(Vec::new()),
                paintable_shown: Cell::new(false),
                last_position: Cell::new(None),
                warned_no_video: Cell::new(false),
            }),
        })
    }

    pub fn connect_event(&self, f: impl Fn(&PlayerEvent) + 'static) {
        let mut handlers = self.inner.handlers.borrow_mut();
        Rc::make_mut(&mut handlers).push(Rc::new(f));
    }

    /// Replace whatever is playing with `playable` and start it. Emits `Loading`, then
    /// `Buffering`/`Playing` from the bus, or `Error` if it cannot start.
    pub fn load(&self, playable: Playable, spotify_session: Option<librespot_core::Session>) {
        let had_video = self.inner.paintable_shown.get();
        self.teardown();
        let generation = self.inner.generation.get();
        self.inner.want_playing.set(true);
        self.set_state(PlaybackState::Loading);
        if self.stale(generation) {
            return;
        }

        let built = match playable {
            Playable::Uri {
                uri,
                video,
                headers,
            } => self.build_uri(&uri, video, headers),
            Playable::Spotify { uri } => match spotify_session {
                Some(session) => self.build_spotify(&uri, session, generation),
                None => Err(PlayerError::Spotify("Not signed in to Spotify".into())),
            },
        };
        let (mut active, paintable) = match built {
            Ok(built) => built,
            Err(e) => {
                self.fail_later(generation, e);
                return;
            }
        };
        if let Err(e) = self.watch_bus(&mut active, generation) {
            self.fail_later(generation, e);
            return;
        }
        active.apply_volume(self.inner.volume.get(), self.inner.muted.get());
        let pipeline = active.pipeline.clone();
        self.inner.pipelines.borrow_mut().push(pipeline.downgrade());
        *self.inner.active.borrow_mut() = Some(active);
        self.start_ticker();

        match paintable {
            Some(p) => {
                self.inner.paintable_shown.set(true);
                self.emit(&PlayerEvent::VideoPaintable(Some(p)));
            }
            None if had_video => {
                self.inner.paintable_shown.set(false);
                self.emit(&PlayerEvent::VideoPaintable(None));
            }
            None => {}
        }
        if self.stale(generation) {
            return;
        }

        match pipeline.set_state(gst::State::Playing) {
            Ok(gst::StateChangeSuccess::NoPreroll) => {
                self.with_active(|a| a.is_live = true);
            }
            Ok(_) => {}
            Err(_) => {
                let error = pipeline
                    .bus()
                    .and_then(|bus| bus.pop_filtered(&[gst::MessageType::Error]))
                    .and_then(|msg| match msg.view() {
                        gst::MessageView::Error(err) => {
                            let missing = self
                                .with_active(|a| std::mem::take(&mut a.missing_plugins))
                                .unwrap_or_default();
                            Some(uri::classify_error(err, &missing))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| {
                        PlayerError::Pipeline("the pipeline refused to start".into())
                    });
                drop(pipeline);
                self.teardown();
                self.fail_later(self.inner.generation.get(), error);
            }
        }
    }

    pub fn play(&self) {
        self.inner.want_playing.set(true);
        let Some((pipeline, buffering)) = self.with_active(|a| {
            if let Backend::Spotify(s) = &a.backend {
                s.play();
            }
            (a.pipeline.clone(), a.buffering)
        }) else {
            return;
        };
        // While buffering, the 100 % message resumes playback.
        if !buffering && let Err(e) = pipeline.set_state(gst::State::Playing) {
            log::warn!("could not resume playback: {e}");
        }
    }

    pub fn pause(&self) {
        self.inner.want_playing.set(false);
        let Some(pipeline) = self.with_active(|a| {
            if let Backend::Spotify(s) = &a.backend {
                s.pause();
            }
            a.pipeline.clone()
        }) else {
            return;
        };
        if let Err(e) = pipeline.set_state(gst::State::Paused) {
            log::warn!("could not pause playback: {e}");
        }
        self.set_state(PlaybackState::Paused);
    }

    pub fn toggle(&self) {
        if self.inner.want_playing.get() && self.inner.active.borrow().is_some() {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Stop and release the pipeline.
    pub fn stop(&self) {
        self.teardown();
        self.go_idle(self.inner.generation.get());
    }

    pub fn seek(&self, to: Duration) {
        let Some(duration) = self.with_active(|a| {
            match &mut a.backend {
                Backend::Spotify(s) => s.seek(to),
                Backend::Uri { .. } => {
                    // Before preroll the seek cannot be handled yet; retry on ASYNC_DONE.
                    a.pending_seek = if a.seek_now(to) { None } else { Some(to) };
                }
            }
            a.duration()
        }) else {
            return;
        };
        self.inner.last_position.set(Some((to, duration)));
        self.emit(&PlayerEvent::Position {
            position: to,
            duration,
        });
    }

    /// Linear volume, 0.0–1.0.
    pub fn set_volume(&self, v: f64) {
        let v = if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.inner.volume.set(v);
        self.with_active(|a| a.apply_volume(v, self.inner.muted.get()));
    }

    pub fn volume(&self) -> f64 {
        self.inner.volume.get()
    }

    pub fn set_muted(&self, m: bool) {
        self.inner.muted.set(m);
        self.with_active(|a| a.apply_volume(self.inner.volume.get(), m));
    }

    pub fn state(&self) -> PlaybackState {
        self.inner.state.get()
    }

    pub fn position(&self) -> Option<Duration> {
        self.active_ref(Active::position).flatten()
    }

    pub fn duration(&self) -> Option<Duration> {
        self.active_ref(Active::duration).flatten()
    }

    /// Pipelines built by this player that have not been disposed yet (0 or 1 when healthy).
    pub fn live_pipelines(&self) -> usize {
        let mut pipelines = self.inner.pipelines.borrow_mut();
        pipelines.retain(|w| w.upgrade().is_some());
        pipelines.len()
    }

    // ---- internals ----

    fn build_uri(
        &self,
        uri: &str,
        video: bool,
        headers: Vec<(String, String)>,
    ) -> Result<(Active, Option<gdk::Paintable>), PlayerError> {
        let video_sink = if video {
            let sink = self.inner.video_sink.as_deref();
            if sink.is_none() && !self.inner.warned_no_video.replace(true) {
                log::warn!("no video output is available; videos play audio-only");
            }
            sink
        } else {
            None
        };
        let (pipeline, paintable) = uri::build_playbin(
            self.inner.playbin,
            uri,
            &self.inner.audio_sink,
            video_sink,
            headers,
        )?;
        Ok((
            Active::new(
                pipeline,
                Backend::Uri {
                    video: video_sink.is_some(),
                },
            ),
            paintable,
        ))
    }

    fn build_spotify(
        &self,
        uri: &str,
        session: librespot_core::Session,
        generation: u64,
    ) -> Result<(Active, Option<gdk::Paintable>), PlayerError> {
        let (playback, pipeline, events) =
            SpotifyPlayback::start(uri, session, &self.inner.audio_sink)?;
        let mut active = Active::new(pipeline, Backend::Spotify(playback));
        let weak = Rc::downgrade(&self.inner);
        let task = glib::MainContext::ref_thread_default().spawn_local(async move {
            while let Ok(event) = events.recv().await {
                let Some(inner) = weak.upgrade() else { break };
                PlayerCore { inner }.on_spotify_event(generation, event);
            }
        });
        active.spotify_events = Some(TaskGuard(task));
        Ok((active, None))
    }

    fn watch_bus(&self, active: &mut Active, generation: u64) -> Result<(), PlayerError> {
        let bus = active
            .pipeline
            .bus()
            .ok_or_else(|| PlayerError::Pipeline("the pipeline has no message bus".into()))?;
        let weak = Rc::downgrade(&self.inner);
        let guard = bus
            .add_watch_local(move |_, msg| {
                if let Some(inner) = weak.upgrade() {
                    PlayerCore { inner }.on_bus_message(generation, msg);
                }
                glib::ControlFlow::Continue
            })
            .map_err(|e| PlayerError::Pipeline(format!("could not watch the pipeline bus: {e}")))?;
        active.bus_watch = Some(guard);
        Ok(())
    }

    fn start_ticker(&self) {
        let weak = Rc::downgrade(&self.inner);
        let task = glib::MainContext::ref_thread_default().spawn_local(async move {
            loop {
                glib::timeout_future(TICK_INTERVAL).await;
                let Some(inner) = weak.upgrade() else { break };
                PlayerCore { inner }.tick();
            }
        });
        *self.inner.ticker.borrow_mut() = Some(TaskGuard(task));
    }

    fn tick(&self) {
        let Some((Some(position), duration)) = self.active_ref(|a| (a.position(), a.duration()))
        else {
            return;
        };
        let current = Some((position, duration));
        if self.inner.last_position.get() == current {
            return;
        }
        self.inner.last_position.set(current);
        self.emit(&PlayerEvent::Position { position, duration });
    }

    fn on_bus_message(&self, generation: u64, msg: &gst::Message) {
        if self.stale(generation) {
            return;
        }
        use gst::MessageView;
        match msg.view() {
            MessageView::Eos(_) => self.finish_later(generation, PlayerEvent::Finished),
            MessageView::Error(err) => {
                log::warn!(
                    "playback error from {}: {} ({})",
                    err.src()
                        .map(|s| s.path_string().to_string())
                        .unwrap_or_default(),
                    err.error(),
                    err.debug().map(|d| d.to_string()).unwrap_or_default()
                );
                let missing = self
                    .with_active(|a| std::mem::take(&mut a.missing_plugins))
                    .unwrap_or_default();
                self.finish_later(
                    generation,
                    PlayerEvent::Error(uri::classify_error(err, &missing)),
                );
            }
            MessageView::Warning(w) => log::warn!("playback warning: {}", w.error()),
            MessageView::Buffering(b) => {
                self.on_buffering(u8::try_from(b.percent().clamp(0, 100)).unwrap_or(100))
            }
            MessageView::StateChanged(s) => {
                let from_pipeline = msg.src().is_some_and(|src| {
                    self.active_ref(|a| src == a.pipeline.upcast_ref::<gst::Object>())
                        .unwrap_or(false)
                });
                if from_pipeline {
                    // Handlers may load the next item in response; do it once this message
                    // (which references the pipeline) has been released.
                    let current = s.current();
                    self.defer(generation, move |core| core.on_pipeline_state(current));
                }
            }
            MessageView::AsyncDone(_) => {
                self.with_active(|a| {
                    if let Some(to) = a.pending_seek.take()
                        && !a.seek_now(to)
                    {
                        log::warn!("could not seek to {to:?}");
                    }
                });
            }
            MessageView::Element(e) => {
                if let Some(desc) = e.structure().and_then(uri::missing_plugin_description) {
                    log::warn!("missing GStreamer plugin: {desc}");
                    self.with_active(|a| a.missing_plugins.push(desc));
                }
            }
            MessageView::DurationChanged(_) => self.inner.last_position.set(None),
            _ => {}
        }
    }

    fn on_pipeline_state(&self, current: gst::State) {
        let buffering = self.active_ref(|a| a.buffering).unwrap_or(false);
        match current {
            gst::State::Playing if self.inner.want_playing.get() && !buffering => {
                self.set_state(PlaybackState::Playing);
            }
            gst::State::Paused if !self.inner.want_playing.get() => {
                self.set_state(PlaybackState::Paused)
            }
            _ => {}
        }
    }

    fn on_buffering(&self, percent: u8) {
        let Some((pipeline, is_live, was_buffering)) = self.with_active(|a| {
            let was = a.buffering;
            if !a.is_live {
                a.buffering = percent < 100;
            }
            (a.pipeline.clone(), a.is_live, was)
        }) else {
            return;
        };
        if is_live {
            return;
        }
        let want_playing = self.inner.want_playing.get();
        if percent < 100 {
            if want_playing {
                if !was_buffering && let Err(e) = pipeline.set_state(gst::State::Paused) {
                    log::warn!("could not pause for buffering: {e}");
                }
                self.set_state(PlaybackState::Buffering(percent));
            }
        } else if want_playing {
            if pipeline.current_state() == gst::State::Playing {
                self.set_state(PlaybackState::Playing);
            } else if let Err(e) = pipeline.set_state(gst::State::Playing) {
                log::warn!("could not resume after buffering: {e}");
            }
        } else if matches!(self.state(), PlaybackState::Buffering(_)) {
            self.set_state(PlaybackState::Paused);
        }
    }

    fn on_spotify_event(&self, generation: u64, event: SpotifyEvent) {
        if self.stale(generation) {
            return;
        }
        match event {
            SpotifyEvent::EndOfTrack => {
                self.with_active(|a| {
                    if let Backend::Spotify(s) = &mut a.backend {
                        s.end_of_stream();
                    }
                });
            }
            SpotifyEvent::Unavailable => {
                self.finish(PlayerEvent::Error(PlayerError::Spotify(
                    "Track unavailable".into(),
                )));
            }
            SpotifyEvent::Duration(duration) => {
                self.with_active(|a| {
                    if let Backend::Spotify(s) = &mut a.backend {
                        s.set_duration(duration);
                    }
                });
                self.inner.last_position.set(None);
            }
            SpotifyEvent::Loading | SpotifyEvent::Playing | SpotifyEvent::Paused => {
                log::debug!("librespot: {event:?}");
            }
        }
    }

    /// The item ended (EOS or error): release it, report `event`, and go idle unless a
    /// handler already loaded the next item.
    fn finish(&self, event: PlayerEvent) {
        self.teardown();
        let generation = self.inner.generation.get();
        self.emit(&event);
        if !self.stale(generation) {
            self.go_idle(generation);
        }
    }

    /// Run `f` from the main loop once the current dispatch returns, unless the item changed.
    /// Bus messages hold a reference to the pipeline, so tearing down (or letting a handler
    /// load the next item) from inside the watch would keep the old pipeline alive while the
    /// next one is built.
    fn defer(&self, generation: u64, f: impl FnOnce(&PlayerCore) + 'static) {
        let weak = Rc::downgrade(&self.inner);
        drop(
            glib::MainContext::ref_thread_default().spawn_local(async move {
                let Some(inner) = weak.upgrade() else { return };
                let core = PlayerCore { inner };
                if !core.stale(generation) {
                    f(&core);
                }
            }),
        );
    }

    fn finish_later(&self, generation: u64, event: PlayerEvent) {
        self.defer(generation, move |core| core.finish(event));
    }

    /// Report a failure to start from the main loop (never re-entrantly from `load`).
    fn fail_later(&self, generation: u64, error: PlayerError) {
        log::warn!("{error}");
        self.defer(generation, move |core| {
            core.emit(&PlayerEvent::Error(error));
            if !core.stale(generation) {
                core.go_idle(generation);
            }
        });
    }

    /// Drop the current pipeline and everything attached to it.
    fn teardown(&self) {
        self.inner
            .generation
            .set(self.inner.generation.get().wrapping_add(1));
        let ticker = self.inner.ticker.borrow_mut().take();
        drop(ticker);
        let active = self.inner.active.borrow_mut().take();
        drop(active);
        self.inner.last_position.set(None);
    }

    fn go_idle(&self, generation: u64) {
        self.inner.want_playing.set(false);
        if self.inner.paintable_shown.replace(false) {
            self.emit(&PlayerEvent::VideoPaintable(None));
            if self.stale(generation) {
                return;
            }
        }
        self.set_state(PlaybackState::Stopped);
    }

    fn stale(&self, generation: u64) -> bool {
        self.inner.generation.get() != generation
    }

    fn set_state(&self, state: PlaybackState) {
        if self.inner.state.replace(state) != state {
            self.emit(&PlayerEvent::State(state));
        }
    }

    fn emit(&self, event: &PlayerEvent) {
        let handlers = Rc::clone(&self.inner.handlers.borrow());
        for handler in handlers.iter() {
            handler(event);
        }
    }

    /// Run `f` on the active pipeline. `f` must not emit events (handlers may re-enter).
    fn with_active<R>(&self, f: impl FnOnce(&mut Active) -> R) -> Option<R> {
        self.inner.active.borrow_mut().as_mut().map(f)
    }

    fn active_ref<R>(&self, f: impl FnOnce(&Active) -> R) -> Option<R> {
        self.inner.active.borrow().as_ref().map(f)
    }
}
