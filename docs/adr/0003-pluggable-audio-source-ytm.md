# ADR 0003: Simple Browser-Based YouTube Music Authentication & Flatpak Import

## Status
Accepted

## Context
Google routinely blocks logins originating from embedded WebKitGTK WebViews with security warnings ("This browser or app may not be secure"). Attempting to embed a full Chromium or WebKit browser inside a music player adds huge complexity and breaks Google's bot detection heuristics.
Furthermore, on modern Linux systems (such as Project Bluefin), browsers like Firefox, Brave, and Chrome run as Flatpaks under `~/.var/app/`.

## Decision
1. **Remove embedded WebKit login**:
   - Do not embed a WebView or attempt to render Google account login dialogues inside Banshee.
2. **"Open in Browser" + Native/Flatpak Import**:
   - Provide a direct "Open music.youtube.com in Browser" button that delegates to the user's real desktop browser via `xdg-open`.
   - Automatically detect Flatpak browser cookie profiles (e.g. `~/.var/app/org.mozilla.firefox/config/mozilla/firefox/*.default*/cookies.sqlite`, Brave, Chrome).
   - Import the session via `yt-dlp --cookies-from-browser` directly into `GLib.get_user_config_dir()/banshee/ytm_cookies.txt`, filtering lines strictly to `.youtube.com` and `.google.com` (0600 file permissions).
   - Provide a manual Netscape `cookies.txt` file chooser for custom setups.
3. **Flatpak Permissions**:
   - Grant read-only access in `org.projectbluefin.Banshee.yaml` to Flatpak browser directories (`--filesystem=~/.var/app/org.mozilla.firefox:ro`, etc.) to permit cookie import inside the sandbox.
4. **Anonymous Fallback**:
   - Search and playback remain fully operational without any login.
