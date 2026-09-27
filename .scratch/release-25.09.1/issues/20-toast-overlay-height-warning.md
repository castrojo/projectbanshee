# 20: Adwaita warns that the toast overlay exceeds window height when leaving Mini Mode

**What to build:** Leaving Mini Mode on a Broadway run logged a burst of `Adwaita-WARNING: AdwToastOverlay … exceeds AdwApplicationWindow height: requested 232 px, 128 px available`, falling to 129 px over about 150 ms. The window is still at the capsule height while the full content is restored. No visible glitch was confirmed.

**Blocked by:** None

**Status:** needs-triage

**GitHub:** https://github.com/castrojo/projectbanshee/issues/20

## Comments

