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
- **Recent Searches**: Queries the user queued from, newest first, shown on the empty Search page.
- **Now Playing Bar**: Bottom bar with artwork (click opens the artist on YouTube Music), metadata, transport, seek, volume.
- **Mini Mode**: A compact presentation of the window showing only the Now Playing Bar. Not the primary window any more (ADR 0007).
- **Flatpak Browser Session Bridge**: YouTube auth — sign in with the real browser via `xdg-open`, then import cookies from Flatpak Firefox/Chrome/Brave profiles (or a cookies.txt) filtered to Google/YouTube domains, stored 0600.
- **Spotify Browser Sign-in**: OAuth PKCE in the real browser with a loopback redirect; refresh token stored 0600.
- **Share to Discord**: Copies the current Track or the Queue as links to the clipboard and opens the Discord Flatpak via `discord://` (ADR 0009).
- **MPRIS Service**: `org.mpris.MediaPlayer2` and `.Player` on D-Bus for GNOME Shell, media keys and `playerctl`; `OpenUri` queues YouTube/Spotify links.
- **GNOME Compliance**: libadwaita widgets and HIG patterns, adaptive layout, standard shortcuts, AppStream metadata.
