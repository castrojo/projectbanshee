# Context & Domain Glossary: Banshee (Mini Mode)

A modern GTK4/Libadwaita music player focused exclusively on Banshee's iconic **Mini Mode** interface, streaming music from YouTube Music (and future streaming backends) with zero local library overhead and full GNOME 4 compliance.

## Core Concepts & Glossary

- **Mini Mode**: The primary (and only) window mode of Banshee. A compact, high-density pill window (~380x82px) providing immediate playback control, cover art, track progress, volume control, search, and queue management without consuming screen real estate.
- **Player Core**: Central audio engine orchestrating playback using GStreamer (`playbin3`), tracking stream position, duration, volume, and playback state (Stopped, Playing, Paused, Buffering).
- **Audio Source**: Pluggable streaming backend interface (`Source`) implementing authentication, search, stream URL resolution, and user library/likes retrieval.
  - `YouTubeMusicSource`: YouTube Music implementation utilizing `yt-dlp` and session cookies imported from Flatpak browser profiles.
  - `SpotifySource`: (Deferred / Planned) Future backend adhering to the identical `Source` interface.
- **Queue / Playback Queue**: An ordered list of `Track` models. Supports shuffle, repeat mode (Off, Track, All), manual re-ordering, and search-to-queue.
- **Track**: Normalized audio metadata struct (`id`, `title`, `artist`, `album`, `duration`, `thumbnail_url`, `stream_url`, `source_name`).
- **Flatpak Browser Session Bridge**: Authentication flow delegating to the user's default browser via `xdg-open`, importing authenticated cookies from Flatpak browser profiles into the app-scoped config directory (filtered to Google/YouTube domains).
- **MPRIS Service**: Full implementation of `org.mpris.MediaPlayer2` and `org.mpris.MediaPlayer2.Player` D-Bus interfaces to integrate with GNOME Shell top-bar/notifications, media keys, and `playerctl`.
- **GNOME Compliance**: Adherence to GNOME Human Interface Guidelines (HIG): Libadwaita styling, dark/light style manager synchronization, standard keyboard shortcuts, AppStream metadata, desktop action hooks, and desktop notification dispatch.
