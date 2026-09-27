# 18: Intermittent HTTP 403 from googlevideo on a freshly resolved stream

**What to build:** Once, "THE MARVELETTES please mr postman" (N33am370hTw) failed in GStreamer with `Forbidden` right after a fresh `yt-dlp` resolve. Eight follow-up probes (signed out and with a rotated jar) all returned HTTP 206/200 and streamed fine in `souphttpsrc`, so the cause is unknown (candidates: googlevideo rate limiting, a client needing a PO token, IP binding). Player errors now re-resolve once and resume at the same second before skipping; that path has not been observed firing live.

**Blocked by:** None (can start immediately)

**Status:** needs-info

- [ ] Capture yt-dlp stderr (warnings are no longer suppressed on resolve) and the failing URL's `c=`/`client` params the next time it happens
- [ ] Confirm the re-resolve retry recovers playback

## Comments

- 2026-09-26: Retry path verified with a simulated refusal (a fake yt-dlp returning a URL that answers 403 when cookies are passed): the player logged `Forbidden`, re-resolved once without cookies and reached `Playing` (16 s in after 20 s), with no error toast. The real-world 403 remains unexplained and was not reproduced with stale or fresh cookies.
