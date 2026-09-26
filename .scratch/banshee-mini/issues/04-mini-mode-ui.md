# Issue 04: Classic Banshee Mini Mode Window and Controls

Status: ready-for-agent
Blocked by: 01, 02, 03

## Context
Build the iconic, ultra-compact Banshee Mini Mode window (~380x82px) using GTK4 and Libadwaita, hosting album art, track details, scrub bar, and playback buttons.

## Acceptance Criteria
1. Subclass `Adw.ApplicationWindow` set to compact fixed height (82px) and default width (380px).
2. Left: 64x64 rounded thumbnail / cover image (with generic music note placeholder fallback).
3. Center: Title label (bold, ellipsized), Artist/Album label (dimmed, ellipsized), interactive scale/scrubber showing current progress, elapsed/total time labels.
4. Right: Previous, Play/Pause toggle (Adw suggested action), Next.
5. Top/Action bar: Compact buttons for Search popover, Queue popover, Volume popover, Pin on top toggle, and Settings/Auth dialog.
6. Connect player position updates and state changes to UI widgets via GLib idle/timeout dispatch.
