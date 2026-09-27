# 11: Automatic YouTube re-import can pick a browser after sign-out or a cookies.txt import

**What to build:** `refresh_session` uses `cookies::last_browser_spec()` or, failing that, the only detected Flatpak browser (`src/sources/youtube.rs:208-214`, from commit f239a07). Sign-out (`src/sources/cookies.rs:67-76`) and cookies.txt import both delete the saved browser spec on purpose. Any later AuthRequired (a Liked Songs or New Episodes collection load, or yt-dlp's rotated-cookies path) then silently imports from that browser, possibly as a different Google account. CONTEXT.md says re-import comes "from the same browser profile".

Fixing it changes when auto re-import happens: legacy jars that have no saved spec would stop auto-refreshing. That's a behavior change, so it needs a decision.

**Blocked by:** None

**Status:** ready-for-human

- [ ] Decide: drop the detect-browsers fallback, or allow it only when a jar exists and it came from a browser
- [ ] Sign-out stays signed out across AuthRequired events

## Comments

