"""
YouTube Music AudioSource implementation with dual-path auth and fallback.
"""
import json
import os
import shutil
import subprocess
from typing import List, Optional
from banshee.models import Track
from banshee.sources.base import AudioSource

class YouTubeMusicSource(AudioSource):
    def __init__(self, cookies_path: Optional[str] = None):
        self._yt_dlp = shutil.which("yt-dlp") or "/home/linuxbrew/.linuxbrew/bin/yt-dlp"
        if not os.path.exists(self._yt_dlp):
            self._yt_dlp = "yt-dlp"

        from gi.repository import GLib
        self.config_dir = os.path.join(GLib.get_user_config_dir(), "banshee")
        os.makedirs(self.config_dir, exist_ok=True)
        self.cookies_path = cookies_path or os.path.join(self.config_dir, "ytm_cookies.txt")

    @property
    def name(self) -> str:
        return "YouTube Music"

    def is_authenticated(self) -> bool:
        return os.path.exists(self.cookies_path) and os.path.getsize(self.cookies_path) > 32

    def search(self, query: str, limit: int = 10) -> List[Track]:
        """Search tracks via yt-dlp."""
        if not query.strip():
            return []

        search_target = f"ytsearch{limit}:{query}"
        cmd = [
            self._yt_dlp,
            "--default-search", "ytsearch",
            "--dump-json",
            "--no-playlist",
            "--flat-playlist",
            search_target
        ]
        if self.is_authenticated():
            cmd.extend(["--cookies", self.cookies_path])

        try:
            res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=True)
        except Exception as e:
            print(f"[YTM Search Error] {e}")
            return []

        tracks = []
        for line in res.stdout.strip().splitlines():
            if not line.strip():
                continue
            try:
                data = json.loads(line)
                duration = float(data.get("duration") or 0.0)
                # Find thumbnail
                thumb = data.get("thumbnail") or ""
                if not thumb and data.get("thumbnails"):
                    thumb = data["thumbnails"][-1].get("url", "")

                track = Track(
                    id=data.get("id", ""),
                    title=data.get("title", "Unknown Title"),
                    artist=data.get("uploader") or data.get("channel") or "Unknown Artist",
                    album=data.get("album") or "YouTube Music",
                    duration=duration,
                    thumbnail_url=thumb,
                    source_name=self.name
                )
                tracks.append(track)
            except Exception as parse_err:
                continue
        return tracks

    def get_stream_url(self, track: Track) -> str:
        """Resolve high-quality direct audio URL."""
        target = f"https://www.youtube.com/watch?v={track.id}" if track.id else track.title
        cmd = [
            self._yt_dlp,
            "-f", "bestaudio/best",
            "-g",
            "--no-playlist",
            target
        ]
        if self.is_authenticated():
            cmd.extend(["--cookies", self.cookies_path])

        res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=True)
        urls = res.stdout.strip().splitlines()
        if not urls:
            raise RuntimeError(f"No stream found for track {track.id}")
        return urls[-1]

    def get_detected_browsers(self) -> dict[str, str]:
        """Detect available desktop and Flatpak browsers."""
        import glob
        browsers = {}
        
        # Check Flatpak Firefox
        ff_dirs = glob.glob(os.path.expanduser("~/.var/app/org.mozilla.firefox/config/mozilla/firefox/*.default*"))
        for p in ff_dirs:
            if os.path.exists(os.path.join(p, "cookies.sqlite")):
                browsers["Firefox (Flatpak)"] = f"firefox:{p}"
                break

        # Check Flatpak Brave
        brave_dirs = glob.glob(os.path.expanduser("~/.var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser/*"))
        for p in brave_dirs:
            if os.path.exists(os.path.join(p, "Cookies")) or os.path.exists(os.path.join(p, "Network", "Cookies")):
                browsers["Brave (Flatpak)"] = f"brave:{p}"
                break

        # Check Flatpak Chrome
        chrome_dirs = glob.glob(os.path.expanduser("~/.var/app/com.google.Chrome/config/google-chrome/*"))
        for p in chrome_dirs:
            if os.path.exists(os.path.join(p, "Cookies")) or os.path.exists(os.path.join(p, "Network", "Cookies")):
                browsers["Chrome (Flatpak)"] = f"chrome:{p}"
                break

        return browsers

    def import_browser_cookies(self, browser_spec: str) -> bool:
        """Import cookies directly from browser or flatpak profile using yt-dlp."""
        cmd = [
            self._yt_dlp,
            "--cookies-from-browser", browser_spec,
            "--cookies", self.cookies_path,
            "--dump-json",
            "ytsearch1:ping",
            "--no-download"
        ]
        try:
            res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=15)
            if res.returncode == 0 and os.path.exists(self.cookies_path):
                return True
            print(f"[Cookie Import Warning] {res.stderr}")
            return False
        except Exception as e:
            print(f"[Cookie Import Error] {e}")
            return False
    def load_cookie_file(self, source_path: str) -> bool:
        """Copy a selected Netscape cookies.txt file to Banshee config."""
        try:
            shutil.copyfile(source_path, self.cookies_path)
            return True
        except Exception as e:
            print(f"[Cookie Copy Error] {e}")
            return False
