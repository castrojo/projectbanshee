# ADR 0001: Python 3 with PyGObject (GTK4 + Libadwaita + GStreamer)

## Status
Accepted

## Context
The application requires:
1. Native GTK4 and Libadwaita integration for GNOME 4 compliance.
2. Robust, low-latency audio streaming and decoding via GStreamer.
3. WebKitGTK 6.0 integration to authenticate against YouTube Music.
4. Fast metadata extraction and audio stream resolution using `yt-dlp` / Innertube without heavy compilation steps.

Environment verification confirmed:
- Python 3.12+ with PyGObject bindings for Gtk 4.0, Adw 1, Gst 1.0, and WebKit 6.0 are pre-installed and functional.
- `yt-dlp` is available locally in `/home/linuxbrew/.linuxbrew/bin/yt-dlp` or can be invoked via python subprocessing.

## Decision
Build Banshee using Python 3, PyGObject (`Gtk 4.0`, `Adw 1`, `Gst 1.0`, `WebKit 6.0`), and standard Meson build system.

## Consequences
- Immediate development velocity with no C/Rust compilation bottlenecks.
- Full access to all modern Libadwaita widgets (`Adw.ApplicationWindow`, `Adw.HeaderBar`, `Adw.StyleManager`).
- Seamless D-Bus integration via `Gio.DBusExportedObject` for MPRIS.
- Easy distribution and Flatpak packaging via `gnome-runtime` SDK.
