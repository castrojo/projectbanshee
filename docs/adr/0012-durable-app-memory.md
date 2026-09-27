# ADR 0012: Durable app memory — nothing the user asked for is lost

## Status
Accepted.

## Context
The user's instruction: "I never want to lose my queue if I quit, same with search results. The entire app should have my musical wishes captured." A queue-first app whose queue evaporates on quit or crash betrays its main promise.

## Decision
Four JSON files in `$XDG_DATA_HOME/banshee/` (Flatpak: `~/.var/app/io.github.castrojo.Banshee/data/banshee/`), written atomically (temp file + rename, mode 0600):

| File | Contents | Written |
| --- | --- | --- |
| `session.json` | The whole Queue (entries, current entry, shuffle order and pre-shuffle order, repeat mode, next entry id) and the resume position in seconds | 400 ms after any queue change, every ~5 s of playback, on pause, on window close and on quit |
| `search.json` | Recent Searches (50, newest first, case-insensitive dedup; a query is recorded when the user queues from it or stays on its results for ~2 s), the last 300 result lists keyed by (source, filter, normalised query), and the last query + filter | 2 s after a change, on quit |
| `seen.json` | Every Track seen (search results, library, collections, queue), up to 20 000 — the persisted Local Index that powers instant fuzzy results | 5 s after a change, on quit |
| `prefs.json` | Volume, window size/maximized, queue sidebar visibility, Mini Mode | On change (volume) and window close |

On launch the Queue, current entry and resume point are restored without auto-playing; pressing Play resumes at the saved second. The last query and filter are restored with their results shown instantly from `search.json`, then refreshed in the background. Typing any previously-seen query shows its remembered results before the network answers.

A file that fails to parse is renamed to `<name>.json.corrupt` (never deleted) and the app starts with defaults, logging an error. Write failures surface as an error toast.

Library and collection data stay in the disposable cache (ADR 0011); `search.json` is state, not cache, because it records user intent. Signing out of a source purges that source's remembered result lists.

## Consequences
- Crash loss is bounded to the last ~0.4 s of queue edits.
- `search.json` can grow to a few MiB at the caps; it is loaded once at startup.
