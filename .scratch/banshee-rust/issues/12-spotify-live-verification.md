# 12: Verify Spotify sign-in, search, library and Premium playback end to end

**What to build:** Spotify code paths are implemented and unit-tested, and error mapping was exercised against the real API with fake tokens, but no real Spotify account was signed in during this work (sign-in needs the user's browser).

**Blocked by:** None (can start immediately)

**Status:** ready-for-human

- [ ] Accounts → Spotify → Sign In with Browser completes and shows “Signed in as …”
- [ ] Spotify results appear in Search with the Spotify badge
- [ ] Library shows Spotify playlists/liked/albums/artists/podcasts
- [ ] A Spotify track plays through GStreamer (Premium) and advances to the next Queue Entry
- [ ] Non-Premium account shows the Premium-required toast
