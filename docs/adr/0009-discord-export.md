# ADR 0009: Share to Discord via clipboard + discord:// launch

## Status
Superseded by ADR 0013.

## Context
"Export music to the Discord Flatpak" is ambiguous. Options:
1. **Discord Rich Presence** over the IPC socket (`$XDG_RUNTIME_DIR/app/com.discordapp.Discord/discord-ipc-0`). Requires a registered Discord application ID for the handshake; Banshee has none, and a status line is not an export action.
2. **Upload audio files**. There are no local files (streams only) and redistribution would be inappropriate.
3. **Share links**: put the current track or the queue as links on the clipboard and bring Discord up to paste them.

## Decision
Option 3. The Share to Discord action (Now Playing menu and queue menu) writes a formatted message — `title — artist` plus the canonical YouTube Music / YouTube / Spotify URL, one line per item for the queue — to the GDK clipboard, then launches Discord with `gtk::UriLauncher` on `discord://-/channels/@me`. Inside Flatpak this goes through the OpenURI portal to the Discord Flatpak's registered `x-scheme-handler/discord`. A toast confirms "Copied — paste in Discord". If no handler exists, the launch error is shown as a toast and the clipboard still holds the message.

## Consequences
- No Discord permissions or credentials are needed.
- Rich Presence is recorded as an open item requiring a human to register a Discord application.
