# 05: Shortcuts dialog misses registered accels and Mini Mode keys

**What to build:** `win.focus-search` is bound to Ctrl+F, Ctrl+L and Alt+1 (`src/main.rs` accels), but the dialog listed only Ctrl+F. Library sat under "Queueing". The Mini Mode keys (Space, Escape, Ctrl+F) weren't listed.

**Blocked by:** None

**Status:** ready-for-agent

- [x] Search shows `Ctrl+F Ctrl+L Alt+1`
- [x] Library moved to General
- [x] New "Mini Mode" section: Search and add to queue, Close search, Play or pause

## Comments

- 2026-09-27: Fixed; the dialog was checked on Broadway (every section visible after scrolling).
