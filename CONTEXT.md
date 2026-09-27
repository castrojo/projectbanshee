# Context & Domain Glossary: Project Banshee

A queue-first GNOME player (Rust, GTK4/libadwaita, GStreamer) that streams music, videos and podcast episodes from YouTube Music, YouTube and Spotify into one Queue. **Queueing is the default action, not playing.** There is no local library and no radio.

## Core Concepts & Glossary

- **Queue / Playback Queue**: The ordered list of **Queue Entries** the user builds. Mixes any Media Kind from any Audio Source in any order. Supports append (default), play-next, reorder, remove, clear, shuffle (current entry stays first; original order restored when turned off), and Repeat Mode (Off, All, One). Independent of the player: editing never interrupts playback.
- **Queue Entry**: One position in the Queue: a unique entry id plus a Track. The same Track may appear in several entries.
- **Track**: Normalized metadata for one playable item: namespaced `id`, `kind`, `source`, `title`, `artist`, `artist_id`, `album`, `duration`, `thumbnail_url`, `web_url`. Stream URLs are not part of a Track; they are resolved just before playback.
- **Media Kind**: `Music` (a song), `Video` (a YouTube video, shown with picture), `Episode` (a podcast episode).
- **Collection**: A playlist, album, or podcast. Browsable and queueable as a whole; queueing expands it into Queue Entries. Never itself a Queue Entry.
- **Audio Source**: Pluggable backend interface: search by Media Kind, library sections, collection contents, and resolving a Track to a **Playable**.
  - `YouTubeMusicSource`: structured search/library via the YouTube Music InnerTube API (`ytmapi-rs`); stream extraction by `yt-dlp` only at play time. Covers YouTube Music songs, YouTube videos, and YouTube Music podcasts.
  - `SpotifySource`: `librespot` for auth and audio; Spotify Web API for search and library. Requires Premium for playback.
- **Playable**: What Player Core needs to start an item: a GStreamer URI (YouTube) or a Spotify URI (librespot).
- **Player Core**: GStreamer engine. `playbin3` for YouTube (audio-only for Music/Episode, video for Video); librespot-decoded PCM through an `appsrc` pipeline for Spotify. Tracks state (Stopped, Loading, Playing, Paused, Buffering), position, duration, volume.
- **Search Engine**: Per-keystroke fuzzy ranking over the **Local Index** (every Track seen), plus debounced remote searches per Audio Source whose results merge into the same ranked list. Stale responses are discarded.
- **Library**: The signed-in user's surfaces per Audio Source: playlists, subscribed/followed artists, saved albums, liked songs, saved podcasts.
- **Cache**: Stale-while-revalidate JSON cache for Library and Collections (Fresh / Stale / Missing).
- **Artwork Store**: Byte-bounded memory LRU of decoded textures plus a byte-bounded disk cache; garbage-collected periodically.
- **App Memory**: Durable state that captures the user's musical wishes across quits and crashes (ADR 0012): the Queue with its **Resume Point** (current entry + second), **Recent Searches**, remembered search results, every Track seen (the persisted Local Index), and preferences. Distinct from the Cache, which is disposable.
- **Recent Searches**: Queries the user queued from, or whose results they stayed on for a moment (~2 s), newest first, shown on the empty Search page. Half-typed prefixes are never recorded.
- **Now Playing Bar**: Bottom bar with artwork (click opens the artist on YouTube Music), metadata, transport, seek, volume.
- **Mini Mode**: A compact, artwork-lit capsule that replaces the window content: blurred cover backdrop, track and Up next, transport, hairline seek bar, and a quick-add search that slides open under it (ADR 0014).
- **Touch Mode**: A fullscreen, artwork-lit presentation for tablets and 2-in-1s that replaces the window content: a large cover (swipe to skip), big transport and seek, and the Queue as tall touch rows — tap to play, drag the grip to reorder, swipe to remove (with Undo), long-press for the row menu. **Add** opens the search as a bottom sheet. Mutually exclusive with Mini Mode (ADR 0015).
- **Home**: YouTube Music's personalised shelves shown on the empty Search page under Recent Searches; songs queue on tap, collections open (ADR 0014).
- **Up next**: The entry that plays after the current one, shown in the Now Playing Bar and Mini Mode.
- **Keep Going**: Songs suggested when nothing is Up next (the last Queue Entry is playing, or playback ran out at the end): YouTube Music's own up-next for the last Queue Entry, then Home "Quick picks", leaving out anything already in the Queue. Shown under the Queue, in Touch Mode's queue and on Mini Mode's quick-add page. Never plays by itself; one tap appends a song, and if playback had run out at the end of the Queue, that song starts (ADR 0016).
- **Flatpak Browser Session Bridge**: YouTube auth — sign in with the real browser via `xdg-open`, then import cookies from Flatpak Firefox/Chrome/Brave profiles (or a cookies.txt) filtered to Google/YouTube domains, stored 0600. When YouTube rejects the session (the browser rotated its cookies), Banshee re-imports once from the same browser profile automatically.
- **Spotify Browser Sign-in**: OAuth PKCE in the real browser with a loopback redirect; refresh token stored 0600.
- **Discord Presence**: The playing Track (title, artist, artwork, link) shown as the user's Discord status over Discord's local IPC socket; needs a Discord application ID the user creates once (ADR 0013).
- **MPRIS Service**: `org.mpris.MediaPlayer2` and `.Player` on D-Bus for GNOME Shell, media keys and `playerctl`; `OpenUri` queues YouTube/Spotify links.
- **GNOME Compliance**: libadwaita widgets and HIG patterns, adaptive layout, standard shortcuts, AppStream metadata.
