# ADR 0010: Bounded artwork and stream buffers with periodic GC

## Status
Accepted.

## Context
A long session loads thousands of thumbnails and many streams. Without bounds, decoded textures and GStreamer/librespot buffers accumulate and glibc keeps freed heap mapped.

## Decision
- **Artwork**: an in-memory LRU of decoded `gdk::Texture`s bounded by decoded bytes (64 MiB default) and a disk cache bounded by bytes (256 MiB) evicted by last access. Requests for the same URL are coalesced. Thumbnail URLs are rewritten to the smallest adequate size (YouTube `=w…-h…` / `w…-h…` suffixes).
- **Streams**: `playbin3` uses `buffer-size` 4 MiB / `buffer-duration` 10 s, with the source bin's `high-watermark` lowered to 0.05 so playback starts after ~0.5 s of audio instead of GStreamer's default 60 % (~3 s) — measured Next-to-Playing went from 3.9 s to 0.5 s with a pre-resolved stream. The pipeline is set to `NULL` and dropped when an item ends or changes, which frees decoder and queue buffers. The Spotify pipeline uses `appsrc` `max-bytes` 2 MiB with blocking pushes, and librespot runs without an audio-file cache.
- **GC**: a 60 s main-loop timer trims the artwork LRU, prunes the disk cache and expired JSON cache entries, and calls `malloc_trim(0)` to return freed heap to the OS.
- **Verification**: an integration test runs an extended simulated session (many pipelines on generated audio and thousands of artwork insertions) and asserts RSS growth stays within a fixed bound; the app is also run with RSS sampled over a live session.

## Consequences
- Scrolling back to old results may refetch artwork from the disk cache.
- Limits are constants in one module; tuning is a one-line change.
