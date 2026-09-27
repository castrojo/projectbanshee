# ADR 0008: One queue for music, videos and podcast episodes

## Status
Accepted.

## Context
Banshee's distinctive trait was one play queue mixing any media. The prototype's `Track` was music-only.

## Decision
`Track` gains a `kind`: `Music`, `Video`, or `Episode`, and a `source`: `YouTubeMusic` or `Spotify`. The Queue holds `QueueEntry { entry_id, track }` with no ordering constraints by kind or source. Player Core picks the pipeline from `(source, kind)`: YouTube video → `playbin3` with video; YouTube music/episode → `playbin3` audio-only; Spotify → librespot → `appsrc`. Collections (playlist, album, podcast) are not queue items; queueing one expands it into entries.

## Consequences
- Repeat/shuffle and MPRIS work uniformly over entries.
- Podcast episodes use the same search → `+` flow as songs.
