# Banshee

A small GTK4/libadwaita music player that opens in a compact Banshee-inspired Mini Mode. It streams YouTube audio through GStreamer; it does not play local music. The original Banshee application icon is retained as a throwback.

## Install

Download `io.github.castrojo.Banshee.flatpak` from the [GitHub Releases](https://github.com/castrojo/banshee/releases), then install it:

```sh
flatpak install --user --bundle ./io.github.castrojo.Banshee.flatpak
flatpak run io.github.castrojo.Banshee
```

The bundle requires the GNOME 50 runtime. Install it from Flathub if prompted.

## Use

- Search from the magnifier popover; select a result to play or queue it.
- Use the queue and volume popovers to manage playback.
- Use the avatar button to open YouTube Music in the system browser, sign in there, then import the browser session.
- MPRIS integration exposes playback controls and metadata to GNOME Shell and `playerctl`.

## YouTube Music session import

Only Flatpak-installed Firefox, Chrome, and Brave profile locations are considered. The app requests read-only access to those profile directories. Cookie extraction retains only Google/YouTube domain entries and writes the resulting Netscape jar with mode `0600` under the app's private configuration directory. Close the browser first if its cookie database is locked.

**Security limitation:** the filtered session jar is currently stored as a plaintext file protected by filesystem permissions. Moving this session into Secret Service/libsecret remains a release follow-up; do not use Banshee on a shared account before accepting that limitation.

## Current limitations

- Search currently uses yt-dlp's YouTube video search, not the structured YouTube Music song catalog. Results can include unofficial uploads.
- `yt-dlp` resolves ephemeral stream URLs at playback time; YouTube can change extraction behavior. The Flatpak bundles the pinned yt-dlp, EJS challenge scripts, and Deno runtime.
- Spotify and local-library playback are not implemented.
- WebKit login is intentionally not embedded; authentication uses the actual desktop browser.

## Build from source

Use the GNOME SDK, not host development headers:

```sh
flatpak run org.flatpak.Builder --user --install-deps-from=flathub \
  --force-clean --repo=repo build io.github.castrojo.Banshee.yaml
```

Build a distributable bundle after the build succeeds:

```sh
flatpak build-bundle repo io.github.castrojo.Banshee.flatpak io.github.castrojo.Banshee
```

Unit tests run with `meson test -C builddir` in an environment with Meson and PyGObject available.

## Licensing and attribution

Application source is licensed under GPL-3.0-or-later; see [`LICENSE`](LICENSE). The original Banshee icon is under MIT; see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
