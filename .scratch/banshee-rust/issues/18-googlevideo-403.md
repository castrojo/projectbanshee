# 18: Intermittent HTTP 403 from googlevideo on a freshly resolved stream

**What to build:** Once, "THE MARVELETTES please mr postman" (N33am370hTw) failed in GStreamer with `Forbidden` right after a fresh `yt-dlp` resolve. Eight follow-up probes (signed out and with a rotated jar) all returned HTTP 206/200 and streamed fine in `souphttpsrc`, so the cause is unknown (candidates: googlevideo rate limiting, a client needing a PO token, IP binding). Player errors now re-resolve once and resume at the same second before skipping; that path has not been observed firing live.

**Blocked by:** None (can start immediately)

**Status:** needs-info

- [ ] Capture yt-dlp stderr (warnings are no longer suppressed on resolve) and the failing URL's `c=`/`client` params the next time it happens
- [ ] Confirm the re-resolve retry recovers playback

## Comments

- 2026-09-26: Retry path verified with a simulated refusal (a fake yt-dlp returning a URL that answers 403): the player logged `Forbidden`, re-resolved once and reached `Playing` (16 s in after 20 s), with no error toast. The real-world 403 remains unexplained and was not reproduced with stale or fresh cookies.
- 2026-09-26: A cookie-less retry was tried and removed: no evidence cookies cause the 403, and it would break signed-in-only items on retry. Revisit if a real 403 log shows cookie involvement.
- 2026-09-26: Recurred on "Got That Feelin (Clean Version)" in an instance with no cookies at all (signed out), so cookies are not the cause. The re-resolve retry fired ("re-resolving once"); the instance then aborted on an unrelated UI reentrancy bug (fixed), so the retry outcome of that attempt is unknown.
- 2026-09-27: Captured one: two pre-resolves of the same entry ran back to back (two queue changes in quick succession), and the stream URL then returned 403 the moment it was played. The retry re-resolved it and it played (retry verified live). Pre-resolves are now deduplicated; two further runs with the same sequence had one pre-resolve per entry and no 403. Keep open to see whether 403s still occur.
