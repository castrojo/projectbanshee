# ADR 0005: Rust with gtk4-rs, libadwaita-rs and gstreamer-rs

## Status
Accepted. Supersedes ADR 0001.

## Context
The Python prototype proved the concept but search latency, background work, and memory control were weak points. The rebuild must be production quality: responsive background loading, bounded memory, full error handling. The workstation is image-based; development uses the GNOME SDK, not host headers.

## Decision
Rewrite in Rust (edition 2024) using `gtk4` 0.11 (GTK 4.20 API level), `libadwaita` 0.9 (Adw 1.8 API level), and `gstreamer` 0.25 against the GNOME 50 runtime. A Tokio multi-threaded runtime runs network and subprocess work; results return to the GTK main loop via `glib::spawn_future_local` awaiting Tokio join handles. Build with `cargo` inside `org.gnome.Sdk//50` plus `org.freedesktop.Sdk.Extension.rust-stable//25.08` (`build-aux/sdk-run.sh cargo …`), and package with flatpak-builder using vendored cargo sources.

## Consequences
- The Python modules, Meson Python install, and PyGObject tests are removed.
- No GIL, real threads for I/O; memory ownership is explicit, which makes the GC work in ADR 0010 verifiable.
- `yt-dlp` stays as a bundled executable for stream extraction only (ADR 0006).
