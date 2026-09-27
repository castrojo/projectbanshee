# ADR 0016: Keep Going — suggest, don't play, when the Queue runs out

## Status
Accepted. Extends ADR 0007, ADR 0014 and ADR 0015.

## Context
Banshee is queue-first (ADR 0007): the user builds the Queue, and nothing is played that they didn't add. When the last entry plays, or playback reaches the end of the Queue, the player simply stops, and the user has to go back to Search to keep listening. Streaming players answer this with radio or autoplay, which Banshee deliberately doesn't have (CONTEXT.md: "no radio"). YouTube Music already computes an up-next list for every song (the automix radio its web player queues after the song, the `next` endpoint with playlist `RDAMVM<video id>`), and the Home feed (ADR 0014) carries a personalised "Quick picks" shelf that Banshee caches for 30 minutes. Spotify's recommendations API is closed to new applications.

## Decision
- **Keep Going** suggests songs when nothing is Up next: the Queue is non-empty and `Controller::up_next()` is `None` (the last entry is playing, or playback ended at the end). Repeat All / One means something is Up next, so it is hidden then.
- **Content**: up to 8 songs, Tracks only. First YouTube Music's up-next for the last Queue Entry (the seed), when the seed comes from YouTube Music / YouTube, fetched with `ytmapi-rs` `get_watch_playlist_from_video_id` on the anonymous client and mapped like other watch-playlist rows; then the songs of the cached Home "Quick picks" shelf. Spotify seeds use Quick picks only. Anything already queued (including the seed itself) is left out, and each Track (by `Track::key`) appears once. The selection is a pure function, `banshee::suggest::keep_going`, unit-tested in `tests/suggest.rs`.
- **Fetching**: the controller fetches the seed's up-next once per seed entry id, off the main thread like every source call, and keeps it for as long as that entry is queued. Quick picks come from the Home cache (read once, then updated whenever Home refreshes); Keep Going never fetches Home itself. The list is recomputed when the Queue, the current entry or Repeat changes, and on change the controller emits `AppEvent::KeepGoing(tracks)` (empty: hide). A failed fetch is logged and leaves Quick picks only; suggestions never raise an error toast.
- **Nothing plays by itself.** Tapping a suggestion or its `+` goes through the normal enqueue path. If playback had run out at the end of the Queue (the queue advanced past its last entry and stopped, and that entry is still last), the added song starts playing, since continuing is the point; otherwise it is only appended. This rule lives in `Controller::enqueue` (and `enqueue_many`), so any add after the Queue ran out continues playback the same way.
- **Surfaces**, all built from `ItemRow` quick-add rows under a "Keep Going" heading, shown only while there are suggestions:
  - the Queue sidebar, under the list (the rows scroll past 300 px; the end of the list, where the playing entry is, stays in view);
  - Touch Mode's queue slab, with touch rows (76 px, 48 px targets, long-press menu, no ⋮);
  - the compact search's empty page (Mini Mode's quick-add and Touch Mode's Add sheet), under Recent Searches and Up Next.

## Consequences
- The "no radio" rule stays true: suggestions are only ever added by a tap.
- Adding a suggestion while the last entry is still playing makes it Up next, so Keep Going hides until that song is the last one playing; the new seed's suggestions then leave out everything queued.
- Adding anything (from Search too) after playback ran out at the end of the Queue now resumes playback with that item, instead of leaving the player stopped.
- Suggestions depend on YouTube Music's unofficial API, like search; when it fails, Keep Going falls back to Quick picks or shows nothing.
