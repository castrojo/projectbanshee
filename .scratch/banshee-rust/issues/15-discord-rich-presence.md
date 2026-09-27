# 15: Optional Discord Rich Presence

**What to build:** ADR 0009 chose clipboard + discord:// sharing. Rich Presence needs a registered Discord application ID, which only a human can create.

**Blocked by:** None (can start immediately)

**Status:** ready-for-human

- [ ] A Discord application is registered and its client ID recorded
- [ ] Then: Rich Presence over the Flatpak Discord IPC socket as an opt-in preference
