//! Spotify playback (ADR 0006, ADR 0010): librespot decodes the track and a custom `Sink`
//! pushes interleaved S16LE PCM into an `appsrc`, which feeds
//! `audioconvert ! audioresample ! volume ! <audio sink>`.
//!
//! Buffers are timestamped from a frame counter. A seek resets the counter and flushes the
//! appsrc so the new position starts playing at running time 0; `seek_base` maps pipeline
//! stream time back to the position inside the track.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use gst::prelude::*;
use librespot_core::{Session, SpotifyUri};
use librespot_playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot_playback::config::PlayerConfig;
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::NoOpVolume;
use librespot_playback::player::{Player, PlayerEvent as LibrespotEvent};
use librespot_playback::{NUM_CHANNELS, SAMPLE_RATE};

use super::PlayerError;
use crate::runtime;

/// ADR 0010: the appsrc queue holds at most 2 MiB (~12 s of 16-bit stereo PCM).
const APPSRC_MAX_BYTES: u64 = 2 * 1024 * 1024;
const BYTES_PER_SAMPLE: usize = 2;

/// librespot events Player Core cares about, forwarded to the main thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SpotifyEvent {
    Loading,
    Playing,
    Paused,
    /// The decoder delivered the last sample; the appsrc queue still has to drain.
    EndOfTrack,
    Unavailable,
    Duration(Duration),
}

/// State shared between the main thread and the librespot player thread.
struct Shared {
    /// Frames pushed since the last flush. Held across timestamping *and* pushing so a
    /// seek flush can never interleave with a buffer stamped for the old timeline.
    frames: Mutex<u64>,
    /// Set before the pipeline goes to NULL; the sink then drops samples instead of pushing.
    closed: AtomicBool,
}

impl Shared {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            frames: Mutex::new(0),
            closed: AtomicBool::new(false),
        })
    }
}

fn frames_to_time(frames: u64) -> gst::ClockTime {
    let ns = u128::from(frames) * 1_000_000_000 / u128::from(SAMPLE_RATE);
    gst::ClockTime::from_nseconds(u64::try_from(ns).unwrap_or(u64::MAX))
}

/// The librespot `Sink`: converts decoded f64 samples to S16LE and pushes them into appsrc.
struct AppSrcSink {
    appsrc: gst_app::AppSrc,
    shared: Arc<Shared>,
}

impl AppSrcSink {
    fn push_samples(&self, samples: &[f64], converter: &mut Converter) -> SinkResult<()> {
        if self.shared.closed.load(Ordering::Acquire) || samples.is_empty() {
            return Ok(());
        }
        let frames = (samples.len() / usize::from(NUM_CHANNELS)) as u64;
        let mut buffer = gst::Buffer::with_size(samples.len() * BYTES_PER_SAMPLE)
            .map_err(|e| SinkError::OnWrite(format!("could not allocate an audio buffer: {e}")))?;
        let buf = buffer
            .get_mut()
            .ok_or_else(|| SinkError::OnWrite("freshly allocated audio buffer is shared".into()))?;
        {
            let mut map = buf
                .map_writable()
                .map_err(|e| SinkError::OnWrite(format!("could not map an audio buffer: {e}")))?;
            for (out, sample) in map.chunks_exact_mut(BYTES_PER_SAMPLE).zip(samples) {
                // Same conversion as `Converter::f64_to_s16`, written straight into the buffer.
                out.copy_from_slice(&(converter.scale(*sample, 0) as i16).to_le_bytes());
            }
        }
        let mut counter = self
            .shared
            .frames
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let start = *counter;
        *counter = start + frames;
        let pts = frames_to_time(start);
        buf.set_pts(pts);
        buf.set_duration(frames_to_time(start + frames).saturating_sub(pts));
        let pushed = self.appsrc.push_buffer(buffer);
        drop(counter);
        match pushed {
            // Flushing: a seek or teardown is in progress; the samples are obsolete.
            Ok(_) | Err(gst::FlowError::Flushing) | Err(gst::FlowError::Eos) => Ok(()),
            Err(e) => Err(SinkError::OnWrite(format!(
                "the audio pipeline refused data: {e:?}"
            ))),
        }
    }
}

impl Sink for AppSrcSink {
    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        match packet {
            AudioPacket::Samples(samples) => self.push_samples(&samples, converter),
            AudioPacket::Raw(_) => Err(SinkError::InvalidParams(
                "passthrough (undecoded Ogg) packets are not supported".into(),
            )),
        }
    }
}

/// The GStreamer half: `appsrc ! audioconvert ! audioresample ! volume ! sink`.
struct Output {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    volume: gst::Element,
}

fn make(factory: &str) -> Result<gst::Element, PlayerError> {
    gst::ElementFactory::make(factory)
        .build()
        .map_err(|_| PlayerError::MissingPlugin(format!("GStreamer element “{factory}”")))
}

fn build_output(audio_sink: &str) -> Result<Output, PlayerError> {
    let info = gst_audio::AudioInfo::builder(
        gst_audio::AudioFormat::S16le,
        SAMPLE_RATE,
        u32::from(NUM_CHANNELS),
    )
    .layout(gst_audio::AudioLayout::Interleaved)
    .build()
    .map_err(|e| PlayerError::Pipeline(format!("invalid Spotify audio format: {e}")))?;
    let caps = info
        .to_caps()
        .map_err(|e| PlayerError::Pipeline(format!("invalid Spotify audio caps: {e}")))?;
    let appsrc = gst_app::AppSrc::builder()
        .caps(&caps)
        .format(gst::Format::Time)
        .stream_type(gst_app::AppStreamType::Stream)
        .is_live(false)
        .max_bytes(APPSRC_MAX_BYTES)
        .block(true)
        .build();
    let convert = make("audioconvert")?;
    let resample = make("audioresample")?;
    let volume = make("volume")?;
    let sink = gst::parse::bin_from_description(audio_sink, true)
        .map_err(|e| PlayerError::Pipeline(format!("invalid audio output “{audio_sink}”: {e}")))?;
    let pipeline = gst::Pipeline::with_name("spotify");
    pipeline
        .add_many([
            appsrc.upcast_ref(),
            &convert,
            &resample,
            &volume,
            sink.upcast_ref(),
        ])
        .map_err(|e| {
            PlayerError::Pipeline(format!("could not assemble the Spotify pipeline: {e}"))
        })?;
    gst::Element::link_many([
        appsrc.upcast_ref(),
        &convert,
        &resample,
        &volume,
        sink.upcast_ref(),
    ])
    .map_err(|e| PlayerError::Pipeline(format!("could not link the Spotify pipeline: {e}")))?;
    Ok(Output {
        pipeline,
        appsrc,
        volume,
    })
}

/// Reset the timeline to 0 and discard everything queued in appsrc and downstream.
fn flush_timeline(appsrc: &gst_app::AppSrc, shared: &Shared) {
    // flush-start unblocks a push stuck on a full queue, so the lock below is short.
    appsrc.send_event(gst::event::FlushStart::new());
    let mut counter = shared.frames.lock().unwrap_or_else(PoisonError::into_inner);
    *counter = 0;
    appsrc.send_event(gst::event::FlushStop::new(true));
    drop(counter);
}

/// One loaded Spotify track: the librespot player and the appsrc side of the pipeline.
pub(super) struct SpotifyPlayback {
    player: Option<Arc<Player>>,
    appsrc: gst_app::AppSrc,
    volume: gst::Element,
    shared: Arc<Shared>,
    seek_base: Duration,
    duration: Option<Duration>,
    eos_sent: bool,
}

impl SpotifyPlayback {
    /// Build the pipeline and start loading `uri` (`spotify:track:…` / `spotify:episode:…`).
    pub(super) fn start(
        uri: &str,
        session: Session,
        audio_sink: &str,
    ) -> Result<(Self, gst::Element, async_channel::Receiver<SpotifyEvent>), PlayerError> {
        let track = SpotifyUri::from_uri(uri).map_err(|e| {
            PlayerError::Spotify(format!("“{uri}” is not a playable Spotify link: {e}"))
        })?;
        if !track.is_playable() {
            return Err(PlayerError::Spotify(format!(
                "“{uri}” is not a track or episode"
            )));
        }
        let output = build_output(audio_sink)?;
        let shared = Shared::new();
        let sink = AppSrcSink {
            appsrc: output.appsrc.clone(),
            shared: Arc::clone(&shared),
        };

        let config = PlayerConfig {
            normalisation: false,
            ..PlayerConfig::default()
        };
        let player = {
            let _rt = runtime::runtime().enter();
            Player::new(config, session, Box::new(NoOpVolume), move || {
                Box::new(sink)
            })
        };

        // Subscribe before loading: commands are processed in order, so no event is missed.
        let mut librespot_events = player.get_player_event_channel();
        let (tx, rx) = async_channel::unbounded();
        runtime::runtime().spawn(async move {
            while let Some(event) = librespot_events.recv().await {
                let forwarded = match event {
                    LibrespotEvent::Loading { .. } => SpotifyEvent::Loading,
                    LibrespotEvent::Playing { .. } => SpotifyEvent::Playing,
                    LibrespotEvent::Paused { .. } => SpotifyEvent::Paused,
                    LibrespotEvent::EndOfTrack { .. } => SpotifyEvent::EndOfTrack,
                    LibrespotEvent::Unavailable { .. } => SpotifyEvent::Unavailable,
                    LibrespotEvent::TrackChanged { audio_item } => SpotifyEvent::Duration(
                        Duration::from_millis(u64::from(audio_item.duration_ms)),
                    ),
                    _ => continue,
                };
                if tx.send(forwarded).await.is_err() {
                    break;
                }
            }
        });
        player.load(track, true, 0);

        let playback = Self {
            player: Some(player),
            appsrc: output.appsrc,
            volume: output.volume,
            shared,
            seek_base: Duration::ZERO,
            duration: None,
            eos_sent: false,
        };
        Ok((playback, output.pipeline.upcast(), rx))
    }

    pub(super) fn play(&self) {
        if let Some(player) = &self.player {
            player.play();
        }
    }

    pub(super) fn pause(&self) {
        if let Some(player) = &self.player {
            player.pause();
        }
    }

    pub(super) fn seek(&mut self, to: Duration) {
        let Some(player) = &self.player else { return };
        self.seek_base = to;
        self.eos_sent = false;
        flush_timeline(&self.appsrc, &self.shared);
        player.seek(u32::try_from(to.as_millis()).unwrap_or(u32::MAX));
    }

    /// librespot has delivered every sample; let appsrc drain and post EOS.
    pub(super) fn end_of_stream(&mut self) {
        if !self.eos_sent {
            self.eos_sent = true;
            if let Err(e) = self.appsrc.end_of_stream() {
                log::warn!("could not signal end of the Spotify stream: {e:?}");
            }
        }
    }

    pub(super) fn set_volume(&self, volume: f64, muted: bool) {
        self.volume.set_property("volume", volume);
        self.volume.set_property("mute", muted);
    }

    pub(super) fn set_duration(&mut self, duration: Duration) {
        self.duration = Some(duration);
    }

    pub(super) fn duration(&self) -> Option<Duration> {
        self.duration
    }

    pub(super) fn position(&self, pipeline: &gst::Element) -> Option<Duration> {
        let since_seek = pipeline.query_position::<gst::ClockTime>()?;
        Some(self.seek_base + Duration::from_nanos(since_seek.nseconds()))
    }

    /// Stop feeding appsrc; called right before the pipeline goes to NULL.
    pub(super) fn close(&self) {
        self.shared.closed.store(true, Ordering::Release);
    }
}

impl Drop for SpotifyPlayback {
    fn drop(&mut self) {
        self.close();
        if let Some(player) = self.player.take() {
            // Dropping the librespot Player joins its decoder thread, which may be waiting on
            // the network; never do that on the GTK main thread.
            runtime::runtime().spawn_blocking(move || drop(player));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(frames: usize, offset: usize) -> Vec<f64> {
        (0..frames)
            .flat_map(|i| {
                let v = ((offset + i) as f64 * 440.0 * std::f64::consts::TAU
                    / f64::from(SAMPLE_RATE))
                .sin()
                    * 0.2;
                [v, v]
            })
            .collect()
    }

    /// The sink timestamps from its frame counter, and a seek flush restarts the timeline so
    /// pipeline position + seek base tracks the position inside the track.
    #[test]
    fn appsrc_sink_timestamps_and_seek_flush() {
        gst::init().expect("gst init");
        let output = build_output("fakesink sync=true").expect("output");
        let shared = Shared::new();
        let sink = AppSrcSink {
            appsrc: output.appsrc.clone(),
            shared: Arc::clone(&shared),
        };
        output
            .pipeline
            .set_state(gst::State::Playing)
            .expect("play");

        let feeder_shared = Arc::clone(&shared);
        let feeder = std::thread::spawn(move || {
            let mut converter = Converter::new(None);
            let mut offset = 0;
            // 30 s of audio in 20 ms packets, like librespot's decoder output. More than the
            // 2 MiB appsrc limit, so pushes block and the flush has to unblock them.
            while offset < SAMPLE_RATE as usize * 30
                && !feeder_shared.closed.load(Ordering::Acquire)
            {
                let packet = sine(882, offset);
                sink.push_samples(&packet, &mut converter).expect("push");
                offset += 882;
            }
        });

        std::thread::sleep(Duration::from_millis(1200));
        let before = output
            .pipeline
            .query_position::<gst::ClockTime>()
            .expect("position");
        assert!(
            before >= gst::ClockTime::from_mseconds(900),
            "position advanced: {before}"
        );

        flush_timeline(&output.appsrc, &shared);
        std::thread::sleep(Duration::from_millis(600));
        let after = output
            .pipeline
            .query_position::<gst::ClockTime>()
            .expect("position after flush");
        println!("appsrc position before flush {before}, 600 ms after flush {after}");
        assert!(
            after >= gst::ClockTime::from_mseconds(300)
                && after < gst::ClockTime::from_mseconds(900),
            "timeline restarted and kept playing after flush (got {after}, was {before})"
        );

        shared.closed.store(true, Ordering::Release);
        output.pipeline.set_state(gst::State::Null).expect("null");
        feeder.join().expect("feeder");
    }
}
