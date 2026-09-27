# ADR 0015: Touch Mode — fullscreen presentation built around touch queue management

## Status
Accepted. Extends ADR 0007 and ADR 0014.

## Context
Banshee runs on tablets and 2-in-1s, where the regular window's 34 px icon buttons, hover-revealed controls and mouse-oriented drag-and-drop make building and reshaping a Queue awkward. Queueing is Banshee's core action, so touch needs first-class queue management, not just bigger buttons. GNOME HIG (Pointer & Touch): don't rely on hover or double-click; the secondary action (long-press) opens a context menu and must not delete; top and bottom screen-edge drags belong to the system; left/right edges and two-finger gestures are free. libadwaita 1.8 offers `AdwBottomSheet`, `AdwBreakpointBin` and swipe/spring animations.

## Decision
- **Touch Mode** is a third presentation beside the regular window and Mini Mode. Like Mini Mode it swaps the window content; it also fullscreens the window. Entering it leaves Mini Mode. It is toggled from the main menu, F11, or its own exit button (Escape also leaves), and is remembered across launches.
- **Stage**: the whole screen is lit by the blurred, tinted cover (the Mini Mode treatment, scaled up). A large cover sits over title, artist, a thick seek bar and a 96 px play button. Swiping the cover horizontally skips to the next or previous entry.
- **Queue** is the other half of the screen (below the stage when stacked): 76 px rows and 48 px targets, headed by the Up next summary, **Add** and a menu with Clear Queue; the list follows the playing entry. Every row action works by touch without hover:
  - tap plays the entry;
  - the grip on each row reorders **live** — the entry moves as the finger crosses rows, with auto-scroll near the edges;
  - swiping a row sideways reveals a red remove underlay; past ~40% of the width (or a fling) it removes the entry with the usual Undo toast, otherwise it springs back;
  - the visible remove button and long-press menu stay as they are.
- Because `Controller::queue_changed` re-splices the whole list store, rows are rebound on every change and the list loses its scroll position. Reorder and swipe are therefore driven by one gesture on the list view, tracking the entry by id, never by row widget; the scroll position is held across each splice. Re-announcing the same items does not rebind tiles, so the current and played styling is applied to the live rows directly.
- **Add** opens the compact search (the Mini Mode quick-add page) in an `AdwBottomSheet`, so the on-screen keyboard and results share the screen with the Queue.
- Portrait (max aspect ratio 1:1) and windows too narrow for two columns (max width 860 sp, e.g. split screen) stack the stage, with a 180 px cover, above the queue. This uses Touch Mode's own `AdwBreakpointBin` so it never competes with the window's width breakpoint.
- Touch Mode is always dark, like the Mini Mode capsule, so the artwork light reads the same in any system style.

## Consequences
- Gesture thresholds (drag intent, dismiss distance/fling, cover skip velocity, edge auto-scroll) are pure functions in `banshee::touch`, unit-tested without GTK.
- The regular window keeps GTK drag-and-drop for mouse users; Touch Mode doesn't attach it.
- F11 means Touch Mode, not plain fullscreen.
