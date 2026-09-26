# Issue 02: GStreamer Playback Engine

Status: ready-for-agent
Blocked by: 01

## Context
Implement low-latency, glitch-free audio playback using GStreamer `playbin3` (fallback `playbin`), supporting stream URL switching, play/pause, seek, volume control, and GstBus signal handling.

## Acceptance Criteria
1. Implement `Player` class wrapping GStreamer pipeline.
2. Methods: `load_track(track: Track)`, `play()`, `pause()`, `toggle_play()`, `stop()`, `seek(position_seconds: float)`, `set_volume(level: float)`.
3. Signal/callbacks for: `state_changed(PlaybackState)`, `position_changed(position: float, duration: float)`, `track_finished()`, `error(str)`.
4. Graceful handling of invalid or expired stream URIs with error dispatch.
5. Unit test in `tests/test_player.py` validating state transitions and volume boundaries.
