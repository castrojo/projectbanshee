# Release 25.09.1 audit

Release audit of Banshee for 25.09.1: versioning, correctness, packaging, security, dependencies and a design review against CONTEXT.md and docs/adr. Each finding is one issue under `issues/`, mirrored on GitHub as #1–#22 (label `release-25.09.1`). Fixed issues have their checkboxes ticked and a comment with the evidence. Deferred issues carry `ready-for-human`, `needs-triage` or `wontfix`.

Checks run on 2026-09-27 (GNOME SDK 50, rust-stable 1.98.1):
- `cargo build --release --locked`, `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`: pass (94 passed, 9 ignored)
- Ignored live tests with network access. `live_search_and_resolve` resolved song and video streams (googlevideo hosts) despite a cookie-rotation WARN. `live_collections_and_links` and `live_detect_browsers` passed. `live_home` passed when run on its own (11 shelves); in the combined run it bailed early by design. `memory_soak::real_https_playback` passed. The `spotify_live` tests skipped (no Spotify token on this machine), so **Spotify was not verified this release**. `live_signed_in_library` skipped (it needs `BANSHEE_LIVE_IMPORT`).
- Installed 25.09.1 bundle: MPRIS Identity `Banshee`; `Play` resumed playback in the sandbox (Stopped → Playing, position 68.7 s → 70.8 s); bundled yt-dlp with `--ignore-config` resolved format 251.
- `appstreamcli validate` (with network): pass; pedantic: one hint (issue 18)
- `desktop-file-validate`: pass
- `cargo audit`: 4 advisories (issue 15)
- yt-dlp 2026.8.19 and yt-dlp-ejs 0.8.0 are the latest on PyPI
