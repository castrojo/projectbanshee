# Spec: Project Banshee in Rust — queue-first player

Status: ready-for-agent

## Problem Statement

The Python prototype is a Mini Mode pill with the search and queue hidden in popovers. Search shells out to `yt-dlp` video search, so every keystroke-to-result cycle costs seconds, returns unofficial uploads, and shows no progress while it runs. Building a queue — the thing Banshee was loved for — means reopening a popover per item. There is no way to see playlists or the YouTube Music library, no Spotify, no videos or podcasts, and memory is never reclaimed from artwork or stream buffers over a long session.

## Solution

A Rust (`gtk4-rs` + `libadwaita-rs` + GStreamer) rebuild where **queueing is the default action**. The window centres a search entry that is focused on launch; typing shows ranked results immediately (local fuzzy matches first, structured catalog results a moment later), and `+`/Enter appends the item while keeping focus in the entry so a queue can be banged out at typing speed. One Queue holds music, videos and podcast episodes from YouTube Music, YouTube and Spotify in any order. The Library shows the signed-in user's playlists, artists, albums, podcasts and liked songs, cached on disk. The Now Playing bar plays through GStreamer; clicking artwork opens the artist on YouTube Music. MPRIS, a compact Mini Mode, Discord sharing, visible loading, and bounded memory complete it.

## User Stories

1. As a listener, I want the search entry focused when the window opens, so that I can start typing a queue immediately.
2. As a listener, I want results to appear while I type, so that I never wait on a blank list.
3. As a listener, I want results ranked by how well they fuzzy-match what I typed, so that typos and partial words still find the song.
4. As a listener, I want already-seen tracks matched locally and instantly, so that repeat searches feel instant.
5. As a listener, I want a visible spinner while the catalog is being queried, so that I know more results are coming.
6. As a listener, I want to press Enter to queue the top (or selected) result and keep typing, so that I can queue without the mouse.
7. As a listener, I want a `+` button on every result that queues it and leaves the page open, so that I can mass-queue with the mouse.
8. As a listener, I want to see the queue grow beside the results, so that I know what I have added.
9. As a listener, I want a toast confirming each queued item with Undo, so that mistakes are cheap.
10. As a listener, I want to filter search to Music, Videos or Podcasts, so that I can find the right media type.
11. As a listener, I want to search YouTube Music songs, so that I get official catalog tracks with album art.
12. As a listener, I want to search YouTube videos, so that I can queue music videos and other clips.
13. As a listener, I want to search podcasts and episodes, so that I can queue an episode between songs.
14. As a Spotify user, I want Spotify results in the same list, labelled by source, so that one search covers both services.
15. As a listener, I want to mix music, videos and podcast episodes in one queue in any order, so that the queue follows my mood, not the media type.
16. As a listener, I want to reorder queue items by drag and drop and by keyboard (move up/down), so that I can shape the queue.
17. As a listener, I want to remove items and clear the queue, so that I can prune it.
18. As a listener, I want "Play Next" in addition to "Add to Queue", so that I can insert an item right after the current one.
19. As a listener, I want shuffle and repeat (off / all / one), so that I get the classic player controls.
20. As a listener, I want activating a queue row to play it, so that I can jump around.
21. As a listener, I want the Now Playing bar to show artwork, title, artist, position and duration, so that I know what is playing.
22. As a listener, I want to click the artwork to open the artist's page on YouTube Music, so that I can explore the artist.
23. As a listener, I want play/pause, previous, next, seek and volume in the Now Playing bar, so that playback is always reachable.
24. As a listener, I want videos to display their picture when the window is large, so that the video queue item is actually a video.
25. As a listener, I want a buffering indicator while a stream resolves or buffers, so that silence is explained.
26. As a listener, I want a toast when a stream fails, with the queue advancing past it, so that one bad item does not stop the session.
27. As a YouTube Music user, I want to import my browser session from Flatpak Firefox, Chrome or Brave, so that my library appears.
28. As a YouTube Music user, I want to import a Netscape cookies.txt manually, so that non-standard setups work.
29. As a Spotify Premium user, I want to sign in through my browser once, so that Spotify search, library and playback work.
30. As a user, I want to sign out of each service, so that credentials are removed.
31. As a signed-in user, I want to see my playlists, so that I can browse them.
32. As a signed-in user, I want to open a playlist and see its tracks, so that I can pick from it.
33. As a signed-in user, I want to queue an entire playlist (or album / podcast) with one action, so that building a long queue is one click.
34. As a signed-in user, I want to see my subscribed artists, saved albums, liked songs and saved podcasts, so that my library is one place.
35. As a signed-in user, I want the library to load instantly from cache and refresh in the background, so that I am not waiting and YouTube is not hammered.
36. As a user, I want a Refresh action for the library, so that I can force-update the cache.
37. As a GNOME user, I want media keys, the Shell media widget and `playerctl` to control Banshee via MPRIS, so that it behaves like a native player.
38. As a GNOME user, I want MPRIS `OpenUri` with a YouTube or Spotify link to queue it, so that links from elsewhere feed the queue.
39. As a GNOME user, I want a compact Mini Mode showing only Now Playing, so that the classic Banshee pill is still there.
40. As a GNOME user, I want standard shortcuts (Ctrl+F, Ctrl+Q, Space, Ctrl+?, Ctrl+,), so that the app feels native.
41. As a GNOME user, I want the layout to adapt to narrow widths, so that the app works tiled or on a small screen.
42. As a Discord user, I want a Share to Discord action that puts the current track (or the whole queue) as links on my clipboard and opens the Discord Flatpak, so that I can paste it into a chat.
43. As a user on a long session, I want memory to stay flat, so that Banshee can run all day.
44. As a user, I want errors from network, auth and playback shown as toasts or status pages, so that nothing fails silently.

## Implementation Decisions

- **Language/toolkit**: Rust 2024, `gtk4-rs` 0.11 (GTK 4.20 API), `libadwaita-rs` 0.9 (Adw 1.8 API), `gstreamer-rs` 0.25, on the GNOME 50 SDK with the rust-stable extension (ADR 0005).
- **Backend**: hybrid (ADR 0006). YouTube Music structured search, library and metadata come from `ytmapi-rs` (native Rust InnerTube client, the Rust port of ytmusicapi). `yt-dlp` is used only to extract a stream URL for the item about to play. Spotify uses `librespot` for authentication (browser OAuth with PKCE on a loopback redirect) and audio, and the Spotify Web API (with the same OAuth token) for search and library.
- **Audio Source interface**: one async trait with `search(query, kind)`, `library()` (sections), `collection(id)` (playlist/album/podcast contents), and `resolve(track)` returning a `Playable` (a URI for GStreamer, or a Spotify URI for the librespot pipeline). Sources report typed errors: network, auth-required, not-found, unavailable, and extraction.
- **Track** gains `kind` (Music, Video, Episode), `source` (YouTubeMusic, Spotify), `artist_id` and `web_url`. `id` is namespaced by source.
- **Queue** (pure module): ordered list with a current index, append / play-next / move / remove / clear, shuffle that keeps the current item first and restores original order, repeat off/all/one. Items carry a unique queue-entry id so duplicates are distinct.
- **Search engine** (pure module + controller): each keystroke re-ranks a local index (every track seen this session and cached) with `nucleo-matcher`; a 120 ms debounce then fires remote searches per source; stale responses are dropped by generation counter; results are memoised in an in-memory LRU and merged with local results, then re-ranked by fuzzy score with the source's rank as tiebreak.
- **Cache** (pure module): JSON files under the user cache dir with a stored-at timestamp and TTL; reads return Fresh / Stale / Missing so the UI can show stale data and revalidate (stale-while-revalidate). Library TTL 6 h, collection TTL 1 h.
- **Artwork store**: downloads thumbnails once (disk cache with a byte cap, LRU by access time), decodes to `gdk::Texture`, and keeps an in-memory LRU bounded by decoded byte size. A periodic GC trims both and calls `malloc_trim` (ADR 0010).
- **Player Core**: `playbin3` for YouTube (audio-only `playbin3` flags for Music/Episode, video sink `gtk4paintablesink` for Video) with bounded `buffer-size`/`buffer-duration`; Spotify via librespot decoding into an `appsrc` pipeline with `max-bytes`. The pipeline is set to NULL and dropped between items so buffers are released.
- **MPRIS**: `org.mpris.MediaPlayer2` + `.Player` over `gio::DBusConnection`, including `OpenUri` for YouTube/Spotify links.
- **UI**: `AdwApplicationWindow` → `AdwToastOverlay` → `AdwToolbarView` with header (`AdwViewSwitcher`: Search, Library), content in an `AdwOverlaySplitView` whose sidebar is the Queue, and a bottom Now Playing bar. Mini Mode hides everything except Now Playing and shrinks the window (ADR 0007).
- **Discord**: Share copies links to the clipboard and launches the Discord Flatpak via its `discord://` URI handler through the OpenURI portal (ADR 0009).

## Testing Decisions

- Tests exercise public behaviour of pure modules only, no GTK: queue ordering and mixing across media kinds; fuzzy ranking; cache freshness/staleness/corruption; artwork store eviction under byte budgets; memory stability over an extended simulated session (RSS sampled from `/proc/self/status`) through the Player Core with real GStreamer pipelines on generated audio.
- Prior art: `tests/test_models.py` (queue) from the prototype — ported semantics, not wording.
- Network sources are verified by running the app against the live services, not by mocked unit tests.

## Out of Scope

Local library, radio, embedded web login, lyrics, editing remote playlists, Spotify Connect (remote control of other devices), and Discord Rich Presence.

## Further Notes

Spotify playback requires a Spotify Premium account (a librespot constraint). Credentials remain plaintext 0600 files (see README security limitation); Secret Service storage is filed as an open item.
