# ADR 0002: Dedicated Mini Mode UI Architecture

## Status
Accepted

## Context
The user requested Banshee "minimode" as the default, specifically choosing Mini Mode only without full-library mode. The classic Banshee mini mode packed:
- Album art thumbnail
- Track title and artist
- Play/Pause, Previous, Next
- Scrub bar / time display
- Search, volume, and queue controls

## Decision
The main window will be an `Adw.ApplicationWindow` configured specifically for Mini Mode:
- Default size: ~380x80 pixels.
- Resizable horizontally, fixed/compact vertically.
- Keep Above / Pin toggle option so it can float over workspaces.
- HeaderBar hidden or embedded ultra-compactly with window controls.
- Search and Queue presented as popovers attached to compact icon buttons, preserving the micro-footprint.
- Volume popover slider with mute toggle.

## Consequences
- Clean, focused UI with zero clutter.
- No wasted window space or unwanted library navigation trees.
- Fast startup and minimal memory footprint.
