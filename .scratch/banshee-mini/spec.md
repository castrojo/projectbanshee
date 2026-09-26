# Specification: Banshee (Mini Mode)

A modern, ultra-compact GTK4 and Libadwaita music player focused exclusively on Banshee's classic **Mini Mode**, streaming from YouTube Music (with modular architecture for Spotify), and integrating deeply into GNOME 4.

## 1. User Stories & Experience

- **As a desktop user**, I want Banshee to open in a compact "Mini Mode" toolbar (~380x82px) that sits unobtrusively on my screen.
- **As a listener**, I want to search tracks via a clean popover, queue songs, view album art, play/pause, skip, scrub tracks, and adjust volume immediately without logging in.
- **As a YouTube Music user**, I want to optionally import my session:
  - Open YouTube Music in the default browser to log in via Google.
  - One-click "Import Browser Session" from installed Flatpak browsers (Firefox, Brave, Chrome).
  - Or manually load an exported Netscape `cookies.txt` file.
- **As a GNOME user**, I want the player to appear in the GNOME Shell notification tray and respond to media keys / `playerctl` via standard MPRIS D-Bus interfaces.

## 2. Architecture & Modules

```
src/banshee/
├── __init__.py
├── main.py              # Application entry point & GApplication lifecycle
├── models.py            # Track, Queue, PlaybackState dataclasses
├── player.py            # GStreamer playbin3 playback engine & bus monitor
├── sources/
│   ├── base.py          # AudioSource abstract interface
│   └── ytm.py           # YouTubeMusicSource (yt-dlp + cookie parser)
├── ui/
│   ├── window.py        # MiniModeWindow (compact pill layout, headerbar, popovers)
│   ├── search_popover.py# Search input, results list, add-to-queue actions
│   ├── queue_popover.py # Playback queue list, current track indicator, clear/remove
│   ├── volume_popover.py# Volume slider, mute toggle
│   └── auth_dialog.py   # Auth dialog with browser open, Flatpak import, and cookie file loader
├── mpris.py             # org.mpris.MediaPlayer2 D-Bus service export
└── config.py            # Local state (~/.config/banshee/settings.json, cookies.txt)
```

## 3. UI Invariants & Layout (Mini Mode)

- Window Dimensions: Default width 380px, height 82px. Minimal margins and clean border radius.
- Widgets:
  - Left: 64x64 Cover art / thumbnail (rounded corners via CSS).
  - Center: Vertical box with Track Title (bold, ellipsize end), Artist & Album (dim label), and thin interactive scrub bar + time labels (`01:23 / 03:45`).
  - Right: Playback controls: Previous, Play/Pause (prominent primary button), Next.
  - Header / Action icons: Search popover button, Queue popover button, Volume popover button, Login / Auth dialog button.

## 4. GNOME Compliance

- ID: `io.github.castrojo.Banshee`
- Full Desktop Entry (`io.github.castrojo.Banshee.desktop`) with category `AudioVideo;Audio;Player;`
- AppStream Metainfo (`io.github.castrojo.Banshee.metainfo.xml`)
- SVG App Icon (`io.github.castrojo.Banshee.svg`)
- MPRIS v2 compliance: responds to `Play`, `Pause`, `PlayPause`, `Next`, `Previous`, `Stop`, `Seek`, `SetPosition`, `Volume`, `Metadata`.
- Pytest suite testing model invariants, queue ordering, source interface, and MPRIS metadata dictionary generation.
