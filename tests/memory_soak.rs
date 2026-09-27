//! ADR 0010 soak test: an extended session of back-to-back items through Player Core must not
//! grow memory or leak pipelines, and failures must leave the player Stopped and empty.
//!
//! Run: `build-aux/sdk-run.sh cargo test --release --test memory_soak -- --nocapture`

use std::cell::{Cell, RefCell};
use std::io::Write;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use banshee::memory;
use banshee::model::Playable;
use banshee::player::{PlaybackState, PlayerCore, PlayerError, PlayerEvent};
use gst::glib;
use gst::prelude::*;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

const ITEMS: usize = 320;
const WARMUP_ITEMS: usize = 20;
const MAX_RSS_GROWTH: u64 = 20 * 1024 * 1024;
const SOAK_TIMEOUT: Duration = Duration::from_secs(240);
const MIB: f64 = 1024.0 * 1024.0;

/// 16-bit stereo PCM WAV with a hand-written RIFF header.
fn write_wav(path: &Path, seconds: u32) {
    const RATE: u32 = 44_100;
    const CHANNELS: u16 = 2;
    const BITS: u16 = 16;
    let block_align = CHANNELS * BITS / 8;
    let frames = RATE * seconds;
    let data_len = frames * u32::from(block_align);
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&BITS.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        let v = ((f64::from(i) * 440.0 * std::f64::consts::TAU / f64::from(RATE)).sin() * 8000.0)
            as i16;
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&v.to_le_bytes());
    }
    let mut file = std::fs::File::create(path).expect("create wav");
    file.write_all(&out).expect("write wav");
}

/// Encode a 1 s 160×120 test pattern into Matroska with the first available encoder.
fn encode_video(path: &Path) -> Option<&'static str> {
    for encoder in [
        "vp8enc deadline=1",
        "x264enc speed-preset=ultrafast",
        "theoraenc",
        "avenc_mpeg4",
    ] {
        let factory = encoder.split_whitespace().next().unwrap_or(encoder);
        if gst::ElementFactory::find(factory).is_none() {
            continue;
        }
        let desc = format!(
            "videotestsrc num-buffers=30 ! video/x-raw,width=160,height=120,framerate=30/1 ! videoconvert ! \
             {encoder} ! matroskamux ! filesink location=\"{}\"",
            path.display()
        );
        let Ok(pipeline) = gst::parse::launch(&desc) else {
            continue;
        };
        let Some(bus) = pipeline.bus() else { continue };
        if pipeline.set_state(gst::State::Playing).is_err() {
            let _ = pipeline.set_state(gst::State::Null);
            continue;
        }
        let msg = bus.timed_pop_filtered(
            gst::ClockTime::from_seconds(30),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );
        let _ = pipeline.set_state(gst::State::Null);
        let ok = matches!(
            msg.as_ref().map(|m| m.view()),
            Some(gst::MessageView::Eos(_))
        ) && std::fs::metadata(path)
            .map(|m| m.len() > 0)
            .unwrap_or(false);
        if ok {
            return Some(factory);
        }
    }
    None
}

fn file_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .expect("absolute path")
        .to_string()
}

fn rss() -> u64 {
    memory::rss_bytes().expect("VmRSS readable")
}

struct Soak {
    player: RefCell<Option<PlayerCore>>,
    items: Vec<Playable>,
    next: Cell<usize>,
    acted: Cell<bool>,
    finished: Cell<usize>,
    replaced: Cell<usize>,
    seeks: Cell<usize>,
    pauses: Cell<usize>,
    errors: RefCell<Vec<PlayerError>>,
    max_live: Cell<usize>,
    max_live_at: Cell<usize>,
    baseline: Cell<Option<u64>>,
    peak: Cell<u64>,
    rng: RefCell<StdRng>,
    main_loop: glib::MainLoop,
}

impl Soak {
    fn player(&self) -> PlayerCore {
        self.player
            .borrow()
            .clone()
            .expect("player alive during soak")
    }

    /// Record, don't assert: a panic inside a GLib callback aborts the process.
    fn check_live(&self) {
        let live = self.player().live_pipelines();
        if live > self.max_live.get() {
            self.max_live.set(live);
            self.max_live_at.set(self.next.get());
        }
    }

    fn advance(self: &Rc<Self>) {
        let index = self.next.get();
        if index == WARMUP_ITEMS {
            memory::trim_heap();
            let base = rss();
            self.baseline.set(Some(base));
            self.peak.set(base);
        }
        if self.baseline.get().is_some() {
            self.peak.set(self.peak.get().max(rss()));
        }
        if index >= self.items.len() {
            self.main_loop.quit();
            return;
        }
        self.next.set(index + 1);
        self.acted.set(false);
        self.player().load(self.items[index].clone(), None);
        self.check_live();
    }

    fn on_event(self: &Rc<Self>, event: &PlayerEvent) {
        match event {
            PlayerEvent::Finished => {
                self.finished.set(self.finished.get() + 1);
                self.advance();
            }
            PlayerEvent::Error(e) => {
                self.errors.borrow_mut().push(e.clone());
                self.advance();
            }
            PlayerEvent::State(PlaybackState::Playing) if !self.acted.get() => {
                self.acted.set(true);
                let roll = self.rng.borrow_mut().random_range(0..10);
                match roll {
                    0 => {
                        // Skip before the end: the running pipeline is replaced.
                        self.replaced.set(self.replaced.get() + 1);
                        self.advance();
                    }
                    1 => {
                        self.seeks.set(self.seeks.get() + 1);
                        self.player().seek(Duration::from_millis(500));
                    }
                    2 => {
                        self.pauses.set(self.pauses.get() + 1);
                        self.player().pause();
                        let soak = Rc::clone(self);
                        glib::MainContext::ref_thread_default().spawn_local(async move {
                            glib::timeout_future(Duration::from_millis(15)).await;
                            if let Some(player) = soak.player.borrow().as_ref() {
                                player.play();
                            }
                        });
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        if self.player.borrow().is_some() {
            self.check_live();
        }
    }
}

#[test]
fn extended_session_keeps_memory_and_pipelines_bounded() {
    gst::init().expect("gst init");
    let dir = tempfile::tempdir().expect("tempdir");
    let wav = dir.path().join("tone.wav");
    write_wav(&wav, 2);
    let mkv = dir.path().join("pattern.mkv");
    let video_encoder = encode_video(&mkv);
    match video_encoder {
        Some(enc) => println!("soak: video items encoded with {enc}"),
        None => println!("soak: no usable video encoder in this runtime; audio-only soak"),
    }

    let items: Vec<Playable> = (0..ITEMS)
        .map(|i| match video_encoder {
            Some(_) if i % 4 == 3 => Playable::Uri {
                uri: file_uri(&mkv),
                video: true,
                headers: Vec::new(),
            },
            _ => Playable::Uri {
                uri: file_uri(&wav),
                video: false,
                headers: Vec::new(),
            },
        })
        .collect();

    let ctx = glib::MainContext::new();
    ctx.with_thread_default(|| {
        let player =
            PlayerCore::with_sinks("fakesink sync=false", Some("fakesink sync=false")).expect("player");
        let soak = Rc::new(Soak {
            player: RefCell::new(Some(player.clone())),
            items,
            next: Cell::new(0),
            acted: Cell::new(false),
            finished: Cell::new(0),
            replaced: Cell::new(0),
            seeks: Cell::new(0),
            pauses: Cell::new(0),
            errors: RefCell::new(Vec::new()),
            max_live: Cell::new(0),
            max_live_at: Cell::new(0),
            baseline: Cell::new(None),
            peak: Cell::new(0),
            rng: RefCell::new(StdRng::seed_from_u64(0x0BA4_54EE)),
            main_loop: glib::MainLoop::new(Some(&ctx), false),
        });
        let weak = Rc::downgrade(&soak);
        player.connect_event(move |event| {
            if let Some(soak) = weak.upgrade() {
                soak.on_event(event);
            }
        });

        let timed_out = Rc::new(Cell::new(false));
        let watchdog = {
            let timed_out = Rc::clone(&timed_out);
            let main_loop = soak.main_loop.clone();
            ctx.spawn_local(async move {
                glib::timeout_future(SOAK_TIMEOUT).await;
                timed_out.set(true);
                main_loop.quit();
            })
        };

        let started = Instant::now();
        soak.advance();
        soak.main_loop.run();
        watchdog.abort();
        let elapsed = started.elapsed();

        player.stop();
        assert_eq!(player.state(), PlaybackState::Stopped);
        soak.player.replace(None);
        while ctx.iteration(false) {}
        let live_after_stop = player.live_pipelines();
        drop(player);
        memory::trim_heap();
        let final_rss = rss();

        let baseline = soak.baseline.get().expect("warm-up completed");
        let peak = soak.peak.get().max(final_rss);
        println!(
            "soak: {} items in {:.1?} — finished {}, replaced before EOS {}, seeks {}, pause/play {}, max live pipelines {}",
            soak.next.get(),
            elapsed,
            soak.finished.get(),
            soak.replaced.get(),
            soak.seeks.get(),
            soak.pauses.get(),
            soak.max_live.get()
        );
        println!(
            "soak: RSS baseline {:.1} MiB, peak {:.1} MiB, final {:.1} MiB (growth {:+.2} MiB)",
            baseline as f64 / MIB,
            peak as f64 / MIB,
            final_rss as f64 / MIB,
            (final_rss as f64 - baseline as f64) / MIB
        );

        assert!(!timed_out.get(), "soak stalled after {} items", soak.next.get());
        assert!(soak.errors.borrow().is_empty(), "playback errors: {:?}", soak.errors.borrow());
        assert_eq!(soak.finished.get() + soak.replaced.get(), ITEMS, "every item finished or was replaced");
        assert_eq!(live_after_stop, 0, "pipelines alive after stop");
        assert!(
            soak.max_live.get() <= 1,
            "{} pipelines alive at once (item {})",
            soak.max_live.get(),
            soak.max_live_at.get()
        );
        assert!(
            final_rss.saturating_sub(baseline) < MAX_RSS_GROWTH,
            "RSS grew by {:.1} MiB",
            (final_rss - baseline) as f64 / MIB
        );
    })
    .expect("thread-default main context");
}

/// Run `load` and wait for the resulting `Error`.
fn expect_error(ctx: &glib::MainContext, player: &PlayerCore, playable: Playable) -> PlayerError {
    let main_loop = glib::MainLoop::new(Some(ctx), false);
    let got: Rc<RefCell<Option<PlayerError>>> = Rc::new(RefCell::new(None));
    {
        let got = Rc::clone(&got);
        let main_loop = main_loop.clone();
        player.connect_event(move |event| {
            if let PlayerEvent::Error(e) = event
                && got.borrow().is_none()
            {
                got.replace(Some(e.clone()));
                main_loop.quit();
            }
        });
    }
    let watchdog = {
        let main_loop = main_loop.clone();
        ctx.spawn_local(async move {
            glib::timeout_future(Duration::from_secs(15)).await;
            main_loop.quit();
        })
    };
    player.load(playable, None);
    main_loop.run();
    watchdog.abort();
    got.take().expect("an Error event")
}

#[test]
fn failed_loads_report_error_and_leave_player_stopped() {
    gst::init().expect("gst init");
    let ctx = glib::MainContext::new();
    ctx.with_thread_default(|| {
        let cases = [
            // Rejected synchronously: no element handles the scheme.
            Playable::Uri {
                uri: "bogus-scheme://nowhere/track".into(),
                video: false,
                headers: Vec::new(),
            },
            // Fails asynchronously on the bus.
            Playable::Uri {
                uri: "file:///nonexistent/banshee-soak.wav".into(),
                video: false,
                headers: Vec::new(),
            },
        ];
        for playable in cases {
            let player = PlayerCore::with_sinks("fakesink sync=false", None).expect("player");
            let error = expect_error(&ctx, &player, playable.clone());
            println!("{playable:?} → {error}");
            assert!(
                matches!(
                    error,
                    PlayerError::Stream(_) | PlayerError::MissingPlugin(_)
                ),
                "{error:?}"
            );
            assert_eq!(player.state(), PlaybackState::Stopped);
            assert_eq!(player.live_pipelines(), 0);
            assert_eq!(player.position(), None);
        }

        let player = PlayerCore::with_sinks("fakesink sync=false", None).expect("player");
        let error = expect_error(
            &ctx,
            &player,
            Playable::Spotify {
                uri: "spotify:track:4uLU6hMCjMI75M1A2tKUQC".into(),
            },
        );
        assert_eq!(
            error,
            PlayerError::Spotify("Not signed in to Spotify".into())
        );
        assert_eq!(player.state(), PlaybackState::Stopped);
        assert_eq!(player.live_pipelines(), 0);
    })
    .expect("thread-default main context");
}

/// Real network playback through the desktop outputs. Run with
/// `build-aux/sdk-run.sh cargo test --release --test memory_soak -- --ignored --nocapture`.
#[test]
#[ignore = "needs network access and an audio device"]
fn real_https_playback() {
    const URI: &str = "https://gstreamer.freedesktop.org/data/media/sintel_trailer-480p.webm";
    let gtk_ok = gtk::init().is_ok();
    let ctx = glib::MainContext::default();
    let _owner = ctx.acquire().expect("default main context");
    let player = if gtk_ok {
        PlayerCore::new()
    } else {
        PlayerCore::with_sinks("autoaudiosink", None)
    }
    .expect("player");
    player.set_volume(0.3);

    let positions: Rc<RefCell<Vec<Duration>>> = Rc::new(RefCell::new(Vec::new()));
    let states: Rc<RefCell<Vec<PlaybackState>>> = Rc::new(RefCell::new(Vec::new()));
    let paintable = Rc::new(Cell::new(false));
    let failure: Rc<RefCell<Option<PlayerError>>> = Rc::new(RefCell::new(None));
    {
        let (positions, states, paintable, failure) = (
            Rc::clone(&positions),
            Rc::clone(&states),
            Rc::clone(&paintable),
            Rc::clone(&failure),
        );
        player.connect_event(move |event| match event {
            PlayerEvent::Position { position, duration } => {
                println!("tick {position:.2?} / {duration:.2?}");
                positions.borrow_mut().push(*position);
            }
            PlayerEvent::State(s) => {
                println!("state {s:?}");
                states.borrow_mut().push(*s);
            }
            PlayerEvent::VideoPaintable(p) => {
                println!(
                    "video paintable: {}",
                    if p.is_some() { "some" } else { "none" }
                );
                paintable.set(p.is_some());
            }
            PlayerEvent::Error(e) => {
                println!("error: {e}");
                failure.replace(Some(e.clone()));
            }
            PlayerEvent::Finished => println!("finished"),
        });
    }

    let run_for = |duration: Duration| {
        let main_loop = glib::MainLoop::new(Some(&ctx), false);
        let quit = main_loop.clone();
        let task = ctx.spawn_local(async move {
            glib::timeout_future(duration).await;
            quit.quit();
        });
        main_loop.run();
        task.abort();
    };

    let headers = vec![(
        "User-Agent".to_string(),
        "Banshee/0.2 (soak test)".to_string(),
    )];
    println!("-- audio only, 5 s");
    player.load(
        Playable::Uri {
            uri: URI.into(),
            video: false,
            headers: headers.clone(),
        },
        None,
    );
    run_for(Duration::from_secs(5));
    assert!(
        failure.borrow().is_none(),
        "audio playback failed: {:?}",
        failure.borrow()
    );
    assert!(
        states.borrow().contains(&PlaybackState::Playing),
        "reached Playing"
    );
    let audio_max = positions.borrow().iter().max().copied().unwrap_or_default();
    assert!(
        audio_max >= Duration::from_secs(3),
        "position advanced to {audio_max:?}"
    );

    if gtk_ok {
        println!("-- video, 4 s (gtk4paintablesink)");
        positions.borrow_mut().clear();
        player.load(
            Playable::Uri {
                uri: URI.into(),
                video: true,
                headers,
            },
            None,
        );
        run_for(Duration::from_secs(4));
        assert!(
            failure.borrow().is_none(),
            "video playback failed: {:?}",
            failure.borrow()
        );
        assert!(paintable.get(), "a video paintable was provided");
        let video_max = positions.borrow().iter().max().copied().unwrap_or_default();
        assert!(
            video_max >= Duration::from_secs(2),
            "video position advanced to {video_max:?}"
        );
    }

    player.stop();
    assert_eq!(player.state(), PlaybackState::Stopped);
    assert_eq!(player.live_pipelines(), 0);
    assert!(!paintable.get(), "paintable cleared on stop");
}
