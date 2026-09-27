# ADR 0014: Home feed on the Search page; Mini Mode as an artwork capsule

## Status
Accepted. Refines ADR 0007.

## Context
The user found the UI "too traditional music player": transport controls sat awkwardly in both views, Mini Mode was a squeezed copy of the full bar, and an empty search showed only a hint. They asked for a YouTube Music-style Home with content and recommendations, and for Mini Mode to get real investment.

## Decision
- **Home**: the empty Search page shows Recent Searches as chips, then YouTube Music's Home shelves (`FEmusic_home`: Quick picks, Listen again, mixes, albums, podcasts…) as horizontal rows of artwork cards. Tapping a song queues it (queueing stays the default); tapping a playlist, album or artist opens it; every card has `+`. Home is cached for 30 minutes (stale-while-revalidate) and refreshes an expired session like the Library.
- **Library** uses the same card shelves (first 20 per section) with a See All page holding a virtualised list, instead of preference-style rows.
- **Now Playing Bar**: a `GtkCenterBox` — track and "Up next" on the left, transport and seek bar centred on the window, shuffle/repeat/volume, Mini Mode and the menu on the right. The play button takes a colour derived from the artwork, darkened until white text meets WCAG AA.
- **Mini Mode**: replaces the window content with a capsule. The cover, blurred and dimmed, lights the background; title, artist and "Up next · N more" sit beside a 72 px cover; transport on the right; a hairline seek bar along the bottom edge; `+` (or Ctrl+F) slides open a quick-add search under the capsule, so a queue can be built without leaving Mini Mode. Escape closes it.

## Consequences
- The Search page is both the start screen and the queue-building surface.
- Mini Mode no longer shares widgets with the full bar; both are composed from shared player widgets (track info, transport, progress, extras).
