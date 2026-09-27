# 02: Content overflows the window below ~424 px (target 360 px)

**What to build:** The window declares `.width_request(360)` (`src/ui/window.rs:21`), but `AdwApplicationWindow` (a breakpoint bin) doesn't propagate its child's minimum width, so narrower content gets clipped: header window buttons, the view switcher bar and the Now Playing Bar transport are cut off.

Measured with `gtk::Widget::measure` on Broadway runs:

- 800 px window (wide mode, breakpoint off, fixed widths active: `info.root.set_width_request(220)` + `margin_end(18)`, `progress.root.set_width_request(280)` in `src/ui/now_playing.rs`, plus shuffle/repeat/volume): Now Playing Bar min 746 px. The breakpoint switches at 720sp, so windows between about 720 and 746 px still clip in wide mode. This band is untouched by the partial fix.
- 360 px window, after the partial fix (the narrow breakpoint resets both width requests): Now Playing Bar min 424 px, Search page min 402 px, header 275 px. The window still clips below about 424 px. Screenshots before the fix, at 360 px, showed the header window buttons, view switcher bar and transport cut off.

Remaining work is a layout design decision: which Now Playing Bar elements shrink or hide at phone widths, what in the Search page forces 402 px, and whether the collection page header (Artwork 160 + 24 px spacing + two `.pill` buttons in a horizontal box, `src/ui/library.rs:363-375`, not measured) should stack vertically.

**Blocked by:** None

**Status:** ready-for-human

- [x] Now Playing Bar fixed widths only apply when wide (breakpoint setters in `src/ui/window.rs`)
- [ ] Wide-mode minimum (746 px) fits under the 720sp breakpoint, or the breakpoint moves up (clipping between ~720 and 746 px)
- [ ] Now Playing Bar fits 360 px
- [ ] Search page fits 360 px
- [ ] Collection page header fits 360 px

## Comments

- 2026-09-27: Partial fix shipped in 25.09.1 (screenshots taken before and after on Broadway: less clipping, still clipped at 360). The rest needs design input and isn't release-sized.
