# 10: Unused `--talk-name=org.freedesktop.secrets`

**What to build:** The manifest granted Secret Service access, but no code uses it (grep finds no secret-service, libsecret, oo7 or keyring usage in `src/`). README says credentials are 0600 files, not Secret Service. gnome-keyring doesn't isolate apps, so this exposed the whole login keyring to a sandbox that runs yt-dlp and Deno. yt-dlp would only use it through the Python `secretstorage` module (for Chromium v11 cookies), and neither the bundle nor its GNOME 50 runtime has that module: `flatpak run --command=python3 io.github.castrojo.Banshee -c 'import secretstorage'` → `ModuleNotFoundError`. Removing the permission therefore changes no behavior.

**Blocked by:** None

**Status:** ready-for-agent

**GitHub:** https://github.com/castrojo/projectbanshee/issues/10

- [x] Permission removed from `io.github.castrojo.Banshee.yaml`
- [x] `.scratch/banshee-rust/issues/14` notes it must be re-added with that work

## Comments

- 2026-09-27: Fixed. `flatpak info --show-permissions io.github.castrojo.Banshee` on the rebuilt 25.09.1 bundle lists only `org.mpris.MediaPlayer2.banshee=own` under the session bus policy. In-sandbox playback still works (MPRIS Play → Playing, position advancing).
