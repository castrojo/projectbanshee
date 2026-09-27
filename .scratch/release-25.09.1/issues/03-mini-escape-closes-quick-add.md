# 03: Escape does not close the Mini Mode quick-add while typing

**What to build:** README and ADR 0014 say Escape closes the Mini Mode search. The Escape arm lives in a bubble-phase `EventControllerKey` on the Mini root (`src/ui/mini.rs:109-127`). GtkSearchEntry binds Escape to `stop-search` (GTK 4.20 `gtk/gtksearchentry.c:674`), which consumes the key, and `SearchPage`'s `stop-search` handler only clears the text (`src/ui/search.rs:377-385`). So Escape never reached the Mini controller while the entry had focus.

**Blocked by:** None

**Status:** ready-for-agent

**GitHub:** https://github.com/castrojo/projectbanshee/issues/3

- [x] Mini connects to its own quick-add entry's `stop-search` and closes the revealer

## Comments

- 2026-09-27: Fixed in `src/ui/mini.rs`. Broadway run: opened quick-add, typed "radiohead", pressed Escape, and the capsule collapsed. The Mini Mode progress bar wasn't touched (maintainer directive).
