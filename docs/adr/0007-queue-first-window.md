# ADR 0007: Queue-first window; Mini Mode becomes a presentation

## Status
Accepted. Supersedes ADR 0002.

## Context
ADR 0002 made Mini Mode the only window, with search and queue in popovers. Building a queue that way costs a popover round trip per item. The product rule is now *queueing is the default action*.

## Decision
- The main window is `AdwApplicationWindow` → `AdwToastOverlay` → `AdwToolbarView`.
  - Top: `AdwHeaderBar` with an `AdwViewSwitcher` (Search, Library), a queue sidebar toggle, and the primary menu.
  - Content: `AdwOverlaySplitView`; content is an `AdwViewStack` (Search page, Library `AdwNavigationView`); the sidebar (end side) is the Queue. It collapses to an overlay on narrow widths through an `AdwBreakpoint`.
  - Bottom: the Now Playing bar.
- The Search page centres an `AdwClamp` with a large `GtkSearchEntry`, focused on map. Enter queues the selected/top result and selects the entry text so the next query overwrites it. Each result row has a `+` (Add to Queue) and a menu with Play Now / Play Next. Adding never navigates away.
- The Queue is decoupled from the player: it is always visible (wide) and editable independently of playback.
- Mini Mode survives as a toggle (`win.mini-mode`, Ctrl+M) that hides the top bar and content, leaving the Now Playing bar, and shrinks the window to ~420×96.

## Consequences
- Popovers for search and queue are removed.
- The CONTEXT.md definition of Mini Mode changes from "primary and only window" to "compact presentation".
