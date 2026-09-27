# ADR 0006: Backend selection — native YouTube Music InnerTube + yt-dlp streams, librespot + Web API for Spotify

## Status
Accepted. Refines ADR 0003 (auth) and ADR 0004 (URL resolution).

## Context
Perceived search latency is the primary quality bar. Two candidate stacks were given:

- **Option A**: `ytmusicapi` through a Python bridge for YouTube Music, `rspotify` for Spotify.
- **Option B**: `yt-dlp` for playback, the Invidious API for search, `librespot` for Spotify.

Requirements: structured catalog results (songs/albums/artists/podcasts, not arbitrary uploads), authenticated library (playlists, subscriptions, liked songs, podcasts), low latency, low maintenance in a Rust app, fit with the pluggable Audio Source interface, and actual audio playback for both services.

### Evaluation
| Concern | A: ytmusicapi bridge + rspotify | B: Invidious + yt-dlp + librespot |
| --- | --- | --- |
| Search structure | Full YTM catalog (songs, videos, podcasts, episodes, artists) | YouTube video search only; no YTM songs/albums/podcasts |
| Search latency | One InnerTube round trip (~0.3–0.6 s) plus Python start/IPC; a long-lived bridge process is needed to avoid ~1 s interpreter start per call | Public instances frequently rate-limit, go down, or are blocked by YouTube; latency varies widely |
| Library | Yes (cookie auth) | Invidious has no YouTube Music library; would need account on the instance |
| Maintenance | Python interpreter, a bridge protocol, and pip deps inside a Rust Flatpak | Dependence on third-party instances |
| Spotify | `rspotify` is Web API only: it cannot stream audio (Web API only remote-controls other Spotify Connect devices) | `librespot` streams and decodes audio, supports browser OAuth |

Neither option meets every requirement. A hybrid is allowed.

## Decision
- **YouTube Music / YouTube search, library, metadata**: `ytmapi-rs`, a native Rust implementation of the same InnerTube `WEB_REMIX` API that `ytmusicapi` uses (it is a port of ytmusicapi). This gives Option A's structure without a Python bridge, in-process with connection reuse (HTTP/2, one TLS session) so warm searches are a single round trip. Browser cookies (ADR 0003 import) authenticate library calls.
- **YouTube stream extraction**: `yt-dlp`, invoked only when an item is about to play (and pre-resolved for the next item), never for search. It remains the most robust extractor for signature/n-challenge changes; the Flatpak bundles it with EJS and Deno as before.
- **Spotify playback + auth**: `librespot` (core, playback, oauth). Browser OAuth (PKCE, loopback redirect `http://127.0.0.1:8898/login`, librespot's client ID) is the "simple browser auth" requested; the refresh token is stored like the YouTube cookie jar. Decoded PCM is fed into Banshee's GStreamer pipeline via `appsrc`, keeping GStreamer as the single output path.
- **Spotify search/library**: the Spotify Web API over `reqwest` with the OAuth access token (search tracks/episodes/shows, `me/playlists`, `me/tracks`, `me/following`, `me/shows`, playlist/show items). Plain typed requests, no `rspotify` dependency.
- Invidious is rejected (instance reliability, no structured music catalog). The Python bridge is rejected (maintenance and cold-start latency). `rspotify` is rejected (no audio; the needed endpoints are few).

## Consequences
- Search never spawns a process; the first result list is one HTTPS round trip, and local fuzzy matches render before it (spec: search engine).
- InnerTube is an unofficial API; response-shape changes break `ytmapi-rs` parsing. Parse errors surface as a toast naming the source, and the crate is actively maintained upstream (youtui). Bumping it is the fix path.
- Spotify playback requires Premium. The Web API can rate-limit (HTTP 429); the source honours `Retry-After` and reports it.
- A browser-imported YouTube session goes stale when the browser rotates its cookies; InnerTube then answers signed-out (`logged_in=0` in `responseContext`). The source reports that as AuthRequired and the app re-imports once from the remembered browser profile.
- Both sources implement the same Audio Source trait; `resolve` returns either a GStreamer URI or a Spotify URI.
