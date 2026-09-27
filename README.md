# Project Banshee

Project Banshee is a queue-first GNOME player in the spirit of the classic Banshee. **Queueing is the default action, not playing.** Type, press Enter (or `+`), keep typing: one queue mixes YouTube Music songs, YouTube videos, podcast episodes and Spotify tracks in any order. It is written in Rust with GTK 4, libadwaita and GStreamer. It does not play local files or radio, and it is not affiliated with the original Banshee project.

## Install

Download `io.github.castrojo.Banshee.flatpak` from the [GitHub Releases](https://github.com/castrojo/projectbanshee/releases), then install it:

```sh
flatpak install --user --bundle ./io.github.castrojo.Banshee.flatpak
flatpak run io.github.castrojo.Banshee
```

The bundle requires the GNOME 50 runtime. Install it from Flathub if prompted.

## Use

- **Home** is the empty Search page: your recent searches as chips, then YouTube Music's shelves (Quick picks, mixes, albums, podcasts). Tap a song to queue it, tap a playlist or album to open it, or use `+` on any card.
- **Search** is focused when the window opens. Results appear as you type: matches from everything you have seen before show at once, and YouTube Music (and Spotify, if signed in) results merge in a moment later, ranked by fuzzy match. A spinner beside the entry shows when results are still coming.
- **Enter** adds the highlighted result to the queue and selects the text, so the next word you type starts the next search. **Shift+Enter** plays it next, **Ctrl+Enter** plays it now, **Up/Down** move the highlight. Every row also has `+`.
- **Filters**: All, Music, Videos, Podcasts.
- **Links**: paste a YouTube, YouTube Music or Spotify link into Search and press Enter to queue it.
- **Queue** is the sidebar (F9 toggles it). Drag rows to reorder, use `−` to remove (with Undo), activate a row to play it. Shuffle and repeat are in the Now Playing Bar.
- **Library** shows your YouTube Music and Spotify playlists, liked songs, albums, artists and podcasts. Open one to browse it or use `+` to queue all of it. Library data is cached and refreshed in the background.
- **Now Playing Bar**: click the artwork to open the artist on YouTube Music. Videos also show in the Video tab.
- **Mini Mode** (Ctrl+M or the button in the Now Playing Bar) turns the window into a capsule lit by the cover art: track, Up next, transport and a seek line. Press `+` or Ctrl+F in it to search and queue without leaving Mini Mode; Escape closes the search.
- **Discord**: turn on **Accounts → Discord → Show What I'm Playing** and Discord shows the song, artist and artwork as your status. Discord only accepts this from a registered application: create one named "Project Banshee" at <https://discord.com/developers/applications> and paste its Application ID there.
- **MPRIS**: GNOME Shell, media keys and `playerctl` control playback. `playerctl open <YouTube or Spotify link>` queues it.

### What Banshee remembers

Your queue, the current item and the second you were at, recent searches and their results, every track you have seen, volume and window layout are saved to `~/.var/app/io.github.castrojo.Banshee/data/banshee/`: the queue within half a second of every change and every ~5 s of playback, searches within two seconds, and everything again on quit, logout or `kill` (SIGTERM/SIGINT/SIGHUP). A hard crash or SIGKILL loses at most the last fraction of a second. Relaunching restores all of it; press Play to resume where you left off. See [ADR 0012](docs/adr/0012-durable-app-memory.md).

## Accounts

Open **Menu → Accounts** (Ctrl+,).

- **YouTube Music**: sign in to music.youtube.com in your browser, close the browser, then choose **Import from Firefox/Chrome/Brave (Flatpak)**, or import an exported `cookies.txt`. Only Flatpak browser profile directories are read (read-only). Only Google/YouTube cookies are kept, in a mode `0600` file in the app's private config directory. Search and playback work without signing in; the Library needs a session. Browsers rotate YouTube session cookies; when the imported session stops working, Banshee re-imports it from the same browser profile once, automatically, and otherwise asks you to import again.
- **Spotify**: **Sign In with Browser** opens Spotify's login page; after you approve, the browser returns to a local page on `127.0.0.1:8898` and Banshee stores a refresh token (mode `0600`). Playback requires Spotify Premium.

**Security limitation:** credentials are plaintext files protected by filesystem permissions, not Secret Service. Do not use Banshee on a shared account before accepting that limitation.

## How it works

- YouTube Music search, library and metadata use the YouTube Music web API directly from Rust ([`ytmapi-rs`](https://crates.io/crates/ytmapi-rs)); `yt-dlp` is used only to extract the stream for the item about to play (and the next one). Spotify uses [`librespot`](https://github.com/librespot-org/librespot) for sign-in and audio, and the Spotify Web API for search and library. See [ADR 0006](docs/adr/0006-backend-selection.md).
- Playback is GStreamer `playbin3`; Spotify audio is fed into GStreamer through `appsrc`.
- Artwork and stream buffers are bounded and garbage-collected every minute ([ADR 0010](docs/adr/0010-memory-bounds-and-gc.md)).

## Current limitations

- YouTube's web API and stream extraction are unofficial and can change; update the Flatpak when searches or playback break. The Flatpak bundles pinned `yt-dlp`, its EJS challenge scripts and Deno.
- Videos play at 360p: YouTube no longer offers higher-resolution single-file streams without a proof-of-origin token.
- Spotify playback requires Premium; Spotify has removed some Web API endpoints (artist top tracks falls back to search).
- Discord status needs a Discord application ID you create once (see Use).

## Build from source

Everything builds inside the GNOME SDK; no host development headers are needed.

```sh
flatpak install --user flathub org.gnome.Sdk//50 org.freedesktop.Sdk.Extension.rust-stable//25.08
build-aux/sdk-run.sh cargo build --release
build-aux/sdk-run.sh cargo run --release
build-aux/sdk-run.sh cargo test
```

`build-aux/sdk-run.sh` runs a command in the SDK with network, audio, Wayland and the session bus. `BANSHEE_AUDIO_SINK` overrides the audio output (for example `fakesink sync=true`); `BANSHEE_YTDLP` overrides the `yt-dlp` executable.

Flatpak bundle (regenerate `build-aux/cargo-sources.json` with [flatpak-cargo-generator](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo) after changing dependencies):

```sh
flatpak-builder --user --install-deps-from=flathub --force-clean \
  --repo=repo build io.github.castrojo.Banshee.yaml \
  && flatpak build-bundle repo io.github.castrojo.Banshee.flatpak io.github.castrojo.Banshee
```

Use the host `flatpak-builder` from a normal terminal. The Flatpak'd `org.flatpak.Builder` currently fails in `appstreamcli compose` because glycin cannot spawn its image-loader sandbox from inside it (see `.scratch/banshee-rust/issues/13-flatpak-bundle-compose.md`).

Live tests against YouTube Music and Spotify are `#[ignore]`d: `build-aux/sdk-run.sh cargo test --test youtube_live -- --ignored --nocapture`.

## Licensing and attribution

Application source is licensed under GPL-3.0-or-later; see [`LICENSE`](LICENSE). The original Banshee icon is under MIT; see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
