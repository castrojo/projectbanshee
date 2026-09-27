# 22: Search showed "Searching…" for ~10 s in two Broadway runs

**What to build:** While auditing on GTK's Broadway backend, a typed search (once in the Mini Mode quick-add, once in the full window of a fresh profile, with no Mini Mode involved) kept showing "Searching…" for about 10 s. The debug log puts the matching remote search at 374 ms once it started (`search YouTube "radiohead": 374.736066ms`), so the request began late, or the keystrokes reached GTK late. Both slow cases came right after a Broadway page (re)load, when input delivery was visibly unreliable (a blank page until reload, keystrokes not arriving). The one clean timed run on a live page took 0.6 s from the last keystroke to results (typed until 12:18:15.536Z, logged at 12:18:16.139Z). A search restored at launch took 906 ms.

Not reproduced outside the harness. The Mini Mode Escape fix (issue 03) only calls `add_toggle.set_active(false)`, the same close as the `+` toggle, and doesn't touch search state.

**Blocked by:** None

**Status:** needs-info

**GitHub:** https://github.com/castrojo/projectbanshee/issues/22

- [ ] Reproduce on a real Wayland session: fresh profile, type a query right after launch, note the time to results

## Comments

