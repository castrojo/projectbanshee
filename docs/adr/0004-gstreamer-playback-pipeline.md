# ADR 0004: GStreamer playbin3 Pipeline with yt-dlp Dynamic URL Resolution

## Status
Accepted

## Context
Online audio from YouTube Music has ephemeral URLs that expire within hours. We need low-latency, glitch-free audio playback with seek support, volume, and state notification.

## Decision
Host smoke test selected an HTTPS audio format with yt-dlp and passed it to GStreamer.
1. Use GStreamer `playbin3` (falling back to `playbin` if unavailable).
2. Resolve audio stream URL dynamically prior to play.
3. Hook GstBus signals for state transitions (`PLAYING`, `PAUSED`, `STOPPED`, `BUFFERING`) and stream position/duration queries.
4. Pass resolved HTTPS stream URL directly to GStreamer's `uri` property.

## Verification
The host-side GStreamer smoke test decoded 2.1 seconds of audio. This proves the host pipeline only; it did not exercise search-to-play in the built Flatpak.
