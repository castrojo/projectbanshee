# 13: Flatpak bundle: appstreamcli compose fails on icons in this environment

**What to build:** `flatpak run org.flatpak.Builder … build io.github.castrojo.Banshee.yaml` compiles the Rust app from the vendored `build-aux/cargo-sources.json` but fails at `appstreamcli compose` with `icon-file-read-error`: glycin loads icons via `flatpak-spawn --sandbox`, which the Flatpak portal rejects from this nested build sandbox (“Key file does not have group Application”). Host `flatpak-builder` fails earlier with `bwrap: Can't make symlink at /run/user/1000/.flatpak`. The same icons composed for 0.1.1 earlier the same day, so this looks environmental. No 0.2.0 bundle was produced; the stale 0.1.1 bundle was deleted.

**Blocked by:** None (can start immediately)

**Status:** ready-for-human

- [ ] Run the build from a normal host terminal (or CI) and confirm compose passes
- [ ] `flatpak build-bundle repo io.github.castrojo.Banshee.flatpak io.github.castrojo.Banshee` produces a 0.2.0 bundle
- [ ] Installed bundle: search, queue, play, MPRIS verified inside the sandbox
