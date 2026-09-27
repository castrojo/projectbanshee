# 13: Flatpak bundle: appstreamcli compose fails on icons in this environment

**What to build:** `flatpak run org.flatpak.Builder … build io.github.castrojo.Banshee.yaml` compiles the Rust app from the vendored `build-aux/cargo-sources.json` but fails at `appstreamcli compose` with `icon-file-read-error`: glycin loads icons via `flatpak-spawn --sandbox`, which the Flatpak portal rejects from this nested build sandbox (“Key file does not have group Application”). Host `flatpak-builder` fails earlier with `bwrap: Can't make symlink at /run/user/1000/.flatpak`. The same icons composed for 0.1.1 earlier the same day, so this looks environmental. No 0.2.0 bundle was produced; the stale 0.1.1 bundle was deleted.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Run the build from a normal host terminal (or CI) and confirm compose passes
- [ ] `flatpak build-bundle repo io.github.castrojo.Banshee.flatpak io.github.castrojo.Banshee` produces a 0.2.0 bundle
- [ ] Installed bundle: search, queue, play, MPRIS verified inside the sandbox

## Comments

- 2026-09-27: Worked around and shipped 0.2.0. `appstream-compose: false` skips the failing compose; `org.flatpak.Builder` then fails exporting with "not a valid icon: Format not recognized" (its image loaders can't sandbox, for SVG and PNG alike), so the finished `build/` is exported with the host `flatpak build-export` and bundled. Installed with `flatpak install --user --reinstall` and smoke-tested (MPRIS Identity "Banshee", OpenUri → Playing via the bundled yt-dlp). Remaining: find why glycin can't sandbox inside Builder so compose (AppStream catalog data) can be re-enabled.

- 2026-09-27: Retested for 25.09.1. Still fails the same way, now also when running `appstreamcli compose` on the finished tree via `flatpak run org.gnome.Sdk//50`: glycin's `flatpak-spawn --sandbox` is rejected with `Key file does not have group "Application"`. Tracked in `.scratch/release-25.09.1/issues/17-appstream-compose-disabled.md`.
