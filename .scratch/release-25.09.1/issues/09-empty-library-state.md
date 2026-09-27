# 09: Signed-in source with an empty library shows nothing

**What to build:** In `LibraryState::Ready`, empty sections are skipped (`src/ui/library.rs:196`, `continue`), and nothing is rendered when all of them are empty. The source disappears from the Library with no sign that sign-in worked. The SignedOut, Loading and Failed states all render a titled group. Adding an empty state means new user-facing copy, so the wording needs a decision.

**Blocked by:** None

**Status:** ready-for-human

**GitHub:** https://github.com/castrojo/projectbanshee/issues/9

- [ ] Decide the copy and add an empty state for a signed-in source with no items

## Comments

