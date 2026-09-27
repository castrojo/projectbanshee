# 21: Manifest used the deprecated `app-id` key

**What to build:** flatpak-builder documents `id` and keeps `app-id` as a deprecated alias.

**Blocked by:** None

**Status:** ready-for-agent

- [x] `id: io.github.castrojo.Banshee`

## Comments

- 2026-09-27: Fixed; `build-aux/release-flatpak.sh` builds with it.
