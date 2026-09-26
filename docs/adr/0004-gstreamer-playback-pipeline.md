# ADR 0004: GStreamer playbin3 Pipeline with yt-dlp Dynamic URL Resolution

## Status
Accepted

## Context
Online audio from YouTube Music has ephemeral URLs that expire within hours. We need low-latency, glitch-free audio playback with seek support, volume, and state notification.

## Decision
Validated via `.scratch/prototype_gst_ytm.py`:
1. Use GStreamer `playbin3` (falling back to `playbin` if unavailable).
2. Resolve audio stream URL dynamically prior to play.
3. Hook GstBus signals for state transitions (`PLAYING`, `PAUSED`, `STOPPED`, `BUFFERING`) and stream position/duration queries.
4. Pass resolved HTTPS stream URL directly to GStreamer's `uri` property.

## Verification
Prototype verified in 5.92s: resolved stream URL, initialized playbin3, buffered and decoded 2.1s of audio cleanly through the GStreamer pipeline without error.
