# ADR 0015: Touch Mode — fullscreen presentation built around touch queue management

## Status
Accepted. Extends ADR 0007 and ADR 0014.

## Context
Banshee runs on tablets and 2-in-1s, where the regular window's 34 px icon buttons, hover-revealed controls and mouse-oriented drag-and-drop make building and reshaping a Queue awkward. Queueing is Banshee's core action, so touch needs first-class queue management, not just bigger buttons. GNOME HIG (Pointer & Touch): don't rely on hover or double-click; the secondary action (long-press) opens a context menu and must not delete; top and bottom screen-edge drags belong to the system; left/right edges and two-finger gestures are free. libadwaita 1.8 offers `AdwBottomSheet`, `AdwBreakpointBin` and swipe/spring animations.

### Research (context7, 2026-09-27)
- libadwaita (`/websites/gnome_pages_gitlab_gnome_libadwaita_doc_1-latest`):
  - [Adaptive layouts](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/adaptive-layouts.html) and [AdwMultiLayoutView](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.MultiLayoutView.html): a secondary pane becomes a bottom sheet at narrow sizes via breakpoints.
  - [AdwBottomSheet](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/class.BottomSheet.html) "is not adaptive"; larger screens should use sidebars. So the sheet holds the transient search, not the Queue.
  - [AdwToolbarView reveal-bottom-bars](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/method.ToolbarView.set_reveal_bottom_bars.html) for fullscreen chrome.
  - [AdwSwipeTracker::end-swipe](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/signal.SwipeTracker.end-swipe.html): animate from the release point with the release velocity, `AdwSpringAnimation` "usually a good fit". Swipe release uses the same model. `AdwSwipeTracker` itself drives an `AdwSwipeable` container (e.g. `AdwCarousel`), so it can't follow recycled list rows.
  - [Style classes](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1-latest/style-classes.html): `.pill` for prominent standalone buttons, `.osd` for overlay buttons.
- gtk4-rs (`/gtk-rs/gtk4-rs`): gestures (`GestureDrag`, `GestureSwipe`, `GestureLongPress`) claiming or denying event sequences, and `ScrolledWindow` kinetic scrolling for touch, which a row gesture must yield to on vertical drags.
- GNOME HIG pages ([Pointer & Touch](https://developer.gnome.org/hig/guidelines/pointer-touch.html), [Adaptiveness](https://developer.gnome.org/hig/guidelines/adaptive.html)) aren't in context7; read directly.

## Decision
- **Touch Mode** is a third presentation beside the regular window and Mini Mode. Like Mini Mode it swaps the window content (built once, at window construction); it also fullscreens the window. Entering either mode leaves the other, and Mini Mode entered from Touch Mode returns to the remembered window size, not the fullscreen one. It is toggled from the main menu, F11, or its own exit button, and is remembered across launches. Escape works in order: cancel a reorder or swipe in progress (HIG: Esc cancels a pointer operation), close the search sheet, leave Touch Mode.
- **Stage**: the whole screen is lit by the blurred, tinted cover (the Mini Mode treatment, scaled up). A large cover sits over title, artist, a thick seek bar and a 96 px play button. Swiping the cover horizontally skips to the next or previous entry.
- **Queue** is the other half of the screen (below the stage when stacked): 76 px rows and 48 px targets, headed by the Up next summary, **Add** and a menu with Clear Queue. The list opens with the playing entry at the top and follows track changes, except within 10 s of the user scrolling it, so a track ending never moves the rows under their finger. Every row action works by touch without hover:
  - tap plays the entry;
  - the grip on each row reorders **live** — the entry moves as the finger crosses rows, with auto-scroll near the edges;
  - swiping a row sideways reveals a red remove underlay; past ~40% of the width (or a fling) it removes the entry with the usual Undo toast, otherwise it springs back;
  - the visible remove button and the long-press menu stay; the ⋮ button is dropped from Touch rows to give titles room (long-press opens the same menu);
  - a cancelled gesture never commits: a swipe springs back, and a reorder puts the entry back where the drag began (live moves are undone).
- Undo toasts last libadwaita's default 5 s (other toasts 3 s): on touch, a tap arriving after an expired toast lands on the queue row underneath.
- `Controller::queue_changed` splices only the changed range of the list store (`banshee::queue::splice_range`, unit-tested), so unchanged rows keep their widgets and lists keep their scroll position; rows therefore resolve their position by entry id when an action runs, not from the index they were bound with. Reorder and swipe run from one gesture on the list view, tracking the entry by id, never by row widget. Removing the row that holds the list's focus still sends a GtkListView to the top, so Touch Mode holds the position across removals. Re-announcing the same items does not rebind tiles, so the current and played styling is applied to the live rows directly (in the queue sidebar too).
- **Add** opens the compact search (the Mini Mode quick-add page) in an `AdwBottomSheet`, so the on-screen keyboard and results share the screen with the Queue.
- Breakpoints live on Touch Mode's own `AdwBreakpointBin`, so they never compete with the window's width breakpoint. Short landscape (max height 760 sp, e.g. a 2-in-1 at 125–150 %) shrinks the cover to 200 px and tightens the stage. Portrait (max aspect ratio 1:1) and windows too narrow for two columns (max width 860 sp, e.g. split screen) stack a compact stage above the queue: a 112 px cover beside the title, transport and seek, no shuffle/repeat/volume row. The queue gets most of the screen, and this breakpoint is added last so it wins when both match. The Add sheet opens from its button, never from a bottom-edge drag, which GNOME reserves for the Shell.
- Touch Mode is always dark, like the Mini Mode capsule, so the artwork light reads the same in any system style.

## Consequences
- Gesture thresholds (drag intent, dismiss distance/fling, cover skip velocity, edge auto-scroll) are pure functions in `banshee::touch`, unit-tested without GTK.
- The regular window keeps GTK drag-and-drop for mouse users; Touch Mode doesn't attach it.
- F11 means Touch Mode, not plain fullscreen.
