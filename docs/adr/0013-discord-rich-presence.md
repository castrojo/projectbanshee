# ADR 0013: Discord Rich Presence instead of clipboard sharing

## Status
Accepted. Supersedes ADR 0009.

## Context
ADR 0009 read "export to Discord" as copying links and opening the Discord Flatpak. The user clarified the intent: Discord should show what Banshee is playing as the user's status, with the song title, artist and artwork.

## Decision
- Banshee speaks Discord's local RPC protocol over its IPC socket (`$XDG_RUNTIME_DIR/app/com.discordapp.Discord/discord-ipc-N` for the Flatpak, then the native, Snap and Vesktop locations). The Flatpak manifest grants `--filesystem=xdg-run/app/com.discordapp.Discord`.
- `SET_ACTIVITY` with `type: 2` (Listening): details = title, state = "by <artist>" (or "Paused · <artist>"), start/end timestamps while playing, `large_image` = the track's https artwork URL, one button linking to YouTube Music or Spotify.
- Updates are coalesced (at most one per 2 s, newest wins, unchanged states skipped), the activity is cleared on disable, stop and exit, and the client reconnects with backoff when Discord restarts.
- Discord accepts presence only from a registered application ID; an unregistered ID is closed with code 4000 (verified against the user's Discord). The user creates an application once and pastes its ID in Accounts → Discord; the switch and the ID are preferences.
- The clipboard/`discord://` share is removed.

## Consequences
- Works with the Discord Flatpak without extra portals; nothing leaves the machine except through Discord.
- Requires a one-time Discord developer application created by the user (its name — "Banshee" — is what Discord shows as "Listening to …").
