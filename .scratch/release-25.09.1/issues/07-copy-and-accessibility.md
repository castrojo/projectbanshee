# 07: Copy drift and accessible-label mismatches

**What to build:**
- The metainfo advertised "Share to Discord", which is the ADR 0009 clipboard feature, superseded by ADR 0013 (Discord Presence).
- The Discord copy used straight apostrophes ("you're", "Show What I'm Playing", `src/ui/accounts.rs:98,101`), and so did the Spotify error "can't be played" (`src/sources/spotify.rs:737`). The rest of the UI uses typographic ones.
- Every Home card's `+` had the accessible label "Add to queue", while collections have the tooltip "Add All to Queue" (`src/ui/home.rs:241`).

**Blocked by:** None

**Status:** ready-for-agent

**GitHub:** https://github.com/castrojo/projectbanshee/issues/7

- [x] Metainfo: "your current song as your Discord status"
- [x] Typographic apostrophes in those strings and in README
- [x] Home card accessible label matches the tooltip

## Comments

- 2026-09-27: Fixed; `appstreamcli validate` passes.
