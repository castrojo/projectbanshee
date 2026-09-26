# Issue 03: YouTube Music Source with Browser/File Auth Fallback

Status: ready-for-agent
Blocked by: 01

## Context
Implement `YouTubeMusicSource` implementing `AudioSource`. Needs to support anonymous search and streaming out-of-the-box, plus cookie-based authentication via browser cookie extraction or `cookies.txt` file import (with WebKitGTK as experimental UI).

## Acceptance Criteria
1. Implement `YouTubeMusicSource(AudioSource)` using `yt-dlp` executable.
2. `search(query: str, limit: int = 10) -> list[Track]`: searches YouTube Music / YouTube audio and returns parsed `Track` objects.
3. `get_stream_url(track: Track) -> str`: resolves direct audio stream URL with `--no-playlist -f bestaudio/best`.
4. `load_cookies(cookie_file_path: str)`: configures `yt-dlp` to use Netscape cookie file.
5. `import_browser_cookies(browser_name: str) -> bool`: imports cookies via `yt-dlp --cookies-from-browser <browser>`.
6. Unit tests with mocked subprocess in `tests/test_ytm_source.py`.
