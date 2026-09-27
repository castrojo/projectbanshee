# 14: yt-dlp loaded stray configuration files

**What to build:** None of the three yt-dlp invocations passed `--ignore-config`, so a `yt-dlp.conf` in the working directory or the user config could inject options (such as `--exec`) or break the `-J` JSON contract.

**Blocked by:** None

**Status:** ready-for-agent

**GitHub:** https://github.com/castrojo/projectbanshee/issues/14

- [x] `--ignore-config` added to the cookie import (`src/sources/cookies.rs`), metadata, and stream resolve (`src/sources/youtube.rs`)

## Comments

- 2026-09-27: Fixed. `youtube_live::live_search_and_resolve` still resolves song and video streams (googlevideo hosts) with the flag.
