# 12: Spotify token file could be re-created after sign-out

**What to build:** `sign_in` and the token refresh released the `token` mutex before calling `auth::save`, and `sign_out` deleted the file outside the lock. A sign-out landing in that window deleted the file, then the save wrote it back, so sign-out didn't survive a relaunch.

**Blocked by:** None

**Status:** ready-for-agent

**GitHub:** https://github.com/castrojo/projectbanshee/issues/12

- [x] `sign_out` clears the token and deletes the file under the token lock
- [x] `sign_in` saves under the lock and returns `SIGNED_OUT_MEANWHILE` if the token is gone
- [x] Refresh saves under the lock, only when the token is still current

## Comments

- 2026-09-27: Fixed in `src/sources/spotify.rs`; clippy and tests pass. Not exercised live: there is no Spotify sign-in on this machine (`spotify_live` tests skipped: token file not found).
