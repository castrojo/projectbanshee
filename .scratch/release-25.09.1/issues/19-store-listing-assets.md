# 19: Metainfo has no screenshots or branding colors

**What to build:** `data/io.github.castrojo.Banshee.metainfo.xml` has no `<screenshots>` (grep count 0) and no `<branding>`. Validation passes without them, but GNOME Software and Flathub listings rely on them. They need hosted images.

**Blocked by:** None

**Status:** ready-for-human

**GitHub:** https://github.com/castrojo/projectbanshee/issues/19

- [ ] Capture screenshots (Home, Queue, Mini Mode), host them, add `<screenshots>` and `<branding>`

## Comments

