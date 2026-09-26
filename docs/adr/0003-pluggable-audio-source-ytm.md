# ADR 0003: Dual-Mode YouTube Music Authentication (WebKit Login + Browser/File Fallback)

## Status
Accepted (Login Status: **UNVERIFIED**)

## Context
Google frequently blocks OAuth and password logins inside embedded WebViews (`disallowed_useragent` or "This browser or app may not be secure" error). Relying exclusively on embedded WebKitGTK login creates a fatal failure mode if Google's security heuristics block the prompt.

Live end-to-end authentication against `accounts.google.com` inside WebKitGTK cannot be verified headlessly in CI and remains unproven until a user signs in interactively.

## Decision
Provide a robust dual-path authentication system in Banshee:
1. **Primary path (WebKitGTK Bridge)**:
   - Launch an `Adw.Window` containing `WebKit.WebView` with Chrome/Edge desktop User-Agent.
   - Set persistent cookie storage to `~/.config/banshee/ytm_cookies.txt` using `WebKit.CookiePersistentStorage.TEXT` (Netscape format).
   - User signs in at `music.youtube.com`.
   - **Risk**: Google may reject the login as an unsecure browser. Marked **UNVERIFIED**.
2. **Fallback path (Direct Browser Cookie Extraction / Header Paste)**:
   - Option A: "Import from Browser" button leveraging `yt-dlp --cookies-from-browser firefox` (or chrome, chromium, brave, edge).
   - Option B: Direct Netscape `cookies.txt` file picker or raw cookie header paste in Settings for hardened Google accounts.
3. **Anonymous Fallback (Guaranteed Working)**:
   - The app remains 100% operational for search, radio, and playback without any login.

## Consequences
- The app does not hard-depend on embedded Google sign-in working.
- If embedded sign-in fails or is blocked by Google, users import from their normal desktop browser in one click or paste cookies.
- Anonymous search and streaming work out of the box with zero configuration.
