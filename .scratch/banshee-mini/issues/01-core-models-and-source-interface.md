# Issue 01: Core Models and Pluggable AudioSource Interface

Status: ready-for-agent
Blocked by: none

## Context
Define core data structures and pluggable audio backend contract to decouple player logic from any specific streaming service (YouTube Music now, Spotify later).

## Acceptance Criteria
1. Implement `Track` dataclass: `id`, `title`, `artist`, `album`, `duration` (seconds), `thumbnail_url`, `stream_url`, `source_name`.
2. Implement `Queue` model: ordered list of tracks, current index pointer, `add()`, `remove()`, `clear()`, `next()`, `prev()`, shuffle toggle, repeat mode (OFF, ONE, ALL).
3. Implement `PlaybackState` enum: `STOPPED`, `PLAYING`, `PAUSED`, `BUFFERING`.
4. Implement `AudioSource` abstract base class defining:
   - `search(query: str, limit: int = 10) -> list[Track]`
   - `get_stream_url(track: Track) -> str`
   - `is_authenticated() -> bool`
5. Unit tests in `tests/test_models.py` verifying Queue transitions and Track serialization.
