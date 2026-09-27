# 01: Adopt calendar versioning and set 25.09.1

**What to build:** Move from 0.2.x to freedesktop-sdk-style `YY.MM.patch` and release 25.09.1 everywhere a version is carried. Cargo's SemVer forbids leading zeros (`cargo` 1.98.1: "invalid leading zero in minor version number"), so follow the Helix precedent: Cargo holds `25.9.1`, and the user-facing string is re-padded from `CARGO_PKG_VERSION_{MAJOR,MINOR,PATCH}`. Research with primary sources: `docs/research/calendar-versioning.md`.

**Blocked by:** None

**Status:** ready-for-agent

- [x] `Cargo.toml` / `Cargo.lock`: `25.9.1`
- [x] `build.rs` emits `BANSHEE_VERSION=25.09.1`; About dialog uses it (`src/main.rs:135`)
- [x] Metainfo `<release version="25.09.1" date="2026-09-27">` as the newest entry
- [x] `build-aux/release-flatpak.sh`: derives `25.09.1` from `Cargo.toml` (re-pads the month like `build.rs`), refuses to build if the metainfo's newest release differs, export subject `Banshee 25.09.1`
- [x] Manifest: no version field exists (flatpak-builder manifest docs); nothing to set
- [x] README "Versioning and releases" documents the scheme and mapping

## Comments

- 2026-09-27: Done. Debug binary contains the About literals `Banshee` `25.09.1` `Jorge O.` side by side; About dialog shown at 25.09.1 on a Broadway run. `appstreamcli validate` passes with 25.09.1 above 0.2.2 (vercmp strips leading zeros; 25.09.1 > 0.2.2).
