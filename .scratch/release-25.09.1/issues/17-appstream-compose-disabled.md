# 17: appstream-compose stays disabled; `flatpak info` shows no version

**What to build:** Re-enabling `appstream-compose` was retested on 2026-09-27 and still fails inside `org.flatpak.Builder` with `icon-file-read-error`. Running `appstreamcli compose` on the finished tree via `flatpak run org.gnome.Sdk//50` fails the same way. glycin spawns its loader with `flatpak-spawn --sandbox`, and the portal rejects it: `Portal call failed: Authorization error: Key file does not have group "Application"`. This is environmental on this host (see `.scratch/banshee-rust/issues/13`). Without composed AppStream data, `flatpak list` shows an empty version for Banshee (freedesktop Platform shows `freedesktop-sdk-25.08.17`). The release script therefore puts the version in the export commit subject (`Banshee 25.09.1`, visible in `flatpak info`), and the bundle ships the metainfo in `/app/share/metainfo`.

**Blocked by:** None

**Status:** ready-for-human

**GitHub:** https://github.com/castrojo/projectbanshee/issues/17

- [ ] Build on CI or a host where glycin's sandbox works, set `appstream-compose: true`, drop the host export workaround

## Comments

