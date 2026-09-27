//! `playbin3` pipelines for `Playable::Uri` (YouTube audio, video and podcast streams).

use gst::glib;
use gst::prelude::*;
use gtk::gdk;

use super::PlayerError;

/// ADR 0010: bounded network buffering.
const BUFFER_SIZE_BYTES: i32 = 4 * 1024 * 1024;
const BUFFER_DURATION_NS: i64 = 10 * 1_000_000_000;
/// Fraction of the buffer that must fill before playback starts or resumes. GStreamer's
/// default (0.6 of 10 s) made every track wait ~3 s; 0.15 starts after ~1.5 s of audio while still
/// downloading up to 10 s ahead to ride out network stalls.
const START_WATERMARK: f64 = 0.15;

/// The video sink that renders into a `gdk::Paintable` for the GTK UI.
pub(super) const PAINTABLE_SINK: &str = "gtk4paintablesink";

/// Build a playbin for `uri`. `video_sink` is `Some` only when the item is a video and a
/// video output exists; otherwise video (and subtitle) decoding is disabled entirely.
pub(super) fn build_playbin(
    factory: &str,
    uri: &str,
    audio_sink: &str,
    video_sink: Option<&str>,
    headers: Vec<(String, String)>,
) -> Result<(gst::Element, Option<gdk::Paintable>), PlayerError> {
    let playbin = gst::ElementFactory::make(factory)
        .property("uri", uri)
        .property("buffer-size", BUFFER_SIZE_BYTES)
        .property("buffer-duration", BUFFER_DURATION_NS)
        .build()
        .map_err(|_| PlayerError::MissingPlugin(format!("GStreamer element “{factory}”")))?;
    if let Some(bin) = playbin.downcast_ref::<gst::Bin>() {
        bin.connect_deep_element_added(|_, _, element| {
            let is_source_bin = element
                .factory()
                .is_some_and(|f| f.name() == "urisourcebin");
            if is_source_bin && element.has_property("high-watermark") {
                element.set_property("high-watermark", START_WATERMARK);
            }
        });
    }
    let audio = gst::parse::bin_from_description(audio_sink, true)
        .map_err(|e| PlayerError::Pipeline(format!("invalid audio output “{audio_sink}”: {e}")))?;
    playbin.set_property("audio-sink", &audio);

    let mut paintable = None;
    let mut with_video = false;
    if let Some(desc) = video_sink {
        match make_video_sink(desc) {
            Ok((sink, p)) => {
                playbin.set_property("video-sink", &sink);
                paintable = p;
                with_video = true;
            }
            Err(e) => log::warn!("{e}; playing this video as audio only"),
        }
    }
    set_flags(&playbin, with_video);

    if !headers.is_empty() {
        playbin.connect("source-setup", false, move |args| {
            if let Some(Ok(source)) = args.get(1).map(|v| v.get::<gst::Element>()) {
                apply_headers(&source, &headers);
            }
            None
        });
    }
    Ok((playbin, paintable))
}

/// Audio-only items never decode video; video items skip subtitles and visualisations.
fn set_flags(playbin: &gst::Element, video: bool) {
    let Some(pspec) = playbin.find_property("flags") else {
        return;
    };
    let Some(class) = glib::FlagsClass::with_type(pspec.value_type()) else {
        return;
    };
    let Some(builder) = class.builder_with_value(playbin.property_value("flags")) else {
        return;
    };
    let builder = builder
        .set_by_nick("audio")
        .set_by_nick("soft-volume")
        .unset_by_nick("text")
        .unset_by_nick("vis");
    let builder = if video {
        builder.set_by_nick("video").set_by_nick("deinterlace")
    } else {
        builder
            .unset_by_nick("video")
            .unset_by_nick("deinterlace")
            .unset_by_nick("soft-colorbalance")
    };
    match builder.build() {
        Some(value) => playbin.set_property_from_value("flags", &value),
        None => log::warn!("could not compute playbin flags; using defaults"),
    }
}

fn has_property(obj: &gst::Element, name: &str, ty: glib::Type) -> bool {
    obj.find_property(name)
        .is_some_and(|p| p.value_type() == ty)
}

/// Apply the stream host's expected HTTP headers to souphttpsrc (or any source exposing
/// the same properties).
fn apply_headers(source: &gst::Element, headers: &[(String, String)]) {
    let mut extra = gst::Structure::builder("extra-headers");
    let mut extra_count = 0;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("user-agent") {
            if has_property(source, "user-agent", glib::Type::STRING) {
                source.set_property("user-agent", value);
            }
        } else {
            extra = extra.field(name.as_str(), value.as_str());
            extra_count += 1;
        }
    }
    if extra_count > 0 && has_property(source, "extra-headers", gst::Structure::static_type()) {
        source.set_property("extra-headers", extra.build());
    }
}

/// Build the video output. For `gtk4paintablesink` the paintable is returned for the UI, and
/// the sink is wrapped in `glsinkbin` when the paintable has a GL context (zero-copy upload).
pub(super) fn make_video_sink(
    desc: &str,
) -> Result<(gst::Element, Option<gdk::Paintable>), PlayerError> {
    if desc.trim() == PAINTABLE_SINK {
        let sink = gst::ElementFactory::make(PAINTABLE_SINK)
            .build()
            .map_err(|_| {
                PlayerError::MissingPlugin(format!("GStreamer element “{PAINTABLE_SINK}”"))
            })?;
        let paintable = object_property::<gdk::Paintable>(sink.upcast_ref(), "paintable")
            .ok_or_else(|| {
                PlayerError::Pipeline(format!("{PAINTABLE_SINK} did not provide a paintable"))
            })?;
        let has_gl =
            object_property::<gdk::GLContext>(paintable.upcast_ref(), "gl-context").is_some();
        if has_gl
            && let Ok(bin) = gst::ElementFactory::make("glsinkbin")
                .property("sink", &sink)
                .build()
        {
            return Ok((bin, Some(paintable)));
        }
        return Ok((sink, Some(paintable)));
    }
    let bin = gst::parse::bin_from_description(desc, true)
        .map_err(|e| PlayerError::Pipeline(format!("invalid video output “{desc}”: {e}")))?;
    let paintable = bin
        .iterate_recurse()
        .into_iter()
        .flatten()
        .find_map(|el| object_property::<gdk::Paintable>(el.upcast_ref(), "paintable"));
    Ok((bin.upcast(), paintable))
}

/// Read an object-typed property if it exists with a compatible type and is set.
fn object_property<T: IsA<glib::Object>>(obj: &glib::Object, name: &str) -> Option<T> {
    let pspec = obj.find_property(name)?;
    if !pspec.value_type().is_a(glib::Object::static_type()) {
        return None;
    }
    obj.property_value(name)
        .get::<Option<glib::Object>>()
        .ok()
        .flatten()?
        .downcast::<T>()
        .ok()
}

/// Description of a `missing-plugin` element message (posted by decodebin before the error).
pub(super) fn missing_plugin_description(s: &gst::StructureRef) -> Option<String> {
    if s.name() != "missing-plugin" {
        return None;
    }
    if let Ok(name) = s.get::<String>("name") {
        return Some(name);
    }
    let kind = s.get::<String>("type").unwrap_or_else(|_| "plugin".into());
    let detail = s
        .get::<gst::Caps>("detail")
        .map(|caps| caps.to_string())
        .or_else(|_| s.get::<String>("detail"))
        .unwrap_or_default();
    Some(if detail.is_empty() {
        kind
    } else {
        format!("{kind} for {detail}")
    })
}

/// Map a bus error to a user-facing `PlayerError`.
pub(super) fn classify_error(err: &gst::message::Error, missing_plugins: &[String]) -> PlayerError {
    let error = err.error();
    if !missing_plugins.is_empty() {
        return PlayerError::MissingPlugin(missing_plugins.join(", "));
    }
    if error.matches(gst::CoreError::MissingPlugin)
        || error.matches(gst::StreamError::CodecNotFound)
    {
        return PlayerError::MissingPlugin(error.message().to_string());
    }
    PlayerError::Stream(error.message().to_string())
}
