# ADR 0001: Python 3 with PyGObject (GTK4 + Libadwaita + GStreamer)

## Status
Accepted

## Context
The application requires GTK4 and Libadwaita, GStreamer playback, and integration with YouTube streaming tools. The workstation is image-based, so development and packaging must use the GNOME SDK/Platform instead of host development headers or pip installs.

## Decision
Python 3 with PyGObject and Meson is selected on the user's behalf. It keeps the GTK/GStreamer layer in the GNOME SDK and allows using yt-dlp's supported Python package; the Flatpak manifest bundles yt-dlp, its EJS scripts, and Deno.

## Consequences
- The GNOME SDK supplies GTK4, Libadwaita, GStreamer, Python, PyGObject, Meson, and Ninja; host development packages are not required.
- yt-dlp, yt-dlp-ejs, and Deno are packaged with the Flatpak for YouTube challenge solving.
