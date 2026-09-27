# ADR 0011: Stale-while-revalidate disk cache for library surfaces

## Status
Accepted.

## Context
Library surfaces (playlists, artists, albums, liked songs, podcasts) must not hammer the YouTube Music or Spotify APIs and must render instantly.

## Decision
A JSON cache keyed by `(source, surface, id)` under `$XDG_CACHE_HOME/banshee/`. Each entry stores `stored_at` and is read as Fresh (within TTL), Stale, or Missing; corrupt entries read as Missing and are deleted. The UI renders Fresh/Stale immediately; Stale or Missing triggers one background fetch (in-flight requests are coalesced) that rewrites the entry. TTLs: library index 6 h, collection contents 1 h, search 10 min (search memo is memory-only). Refresh forces a fetch. Signing out deletes that source's entries.

## Consequences
- Library opens instantly after the first load.
- Data can be up to one TTL old unless refreshed.
