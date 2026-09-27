# 13: Spotify OAuth loopback ignores `state` and takes the first request

**What to build:** librespot-oauth 0.8.0 discards its CSRF token (`authorize_url(CsrfToken::new_random)`, `lib.rs:245`) and takes the first connection to 127.0.0.1:8898 as the redirect, whatever its path (`lib.rs:181-197`). PKCE prevents code injection, but any local process or web page can end a pending sign-in. The fix needs Banshee's own listener or an upstream patch, which is too large for a release.

**Blocked by:** None

**Status:** ready-for-human

**GitHub:** https://github.com/castrojo/projectbanshee/issues/13

- [ ] Report upstream, or own the loopback listener (verify path and state, loop until a match)

## Comments

