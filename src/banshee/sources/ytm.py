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
        self._yt_dlp = shutil.which("yt-dlp") or "yt-dlp"

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
        """Detect Flatpak browser profiles without probing native host paths."""
        import configparser
        import glob

        browsers = {}
        flatpak_root = os.path.expanduser("~/.var/app")

        firefox_root = os.path.join(flatpak_root, "org.mozilla.firefox/config/mozilla/firefox")
        profiles_ini = os.path.join(firefox_root, "profiles.ini")
        profiles = configparser.ConfigParser()
        if profiles.read(profiles_ini):
            profile_sections = [s for s in profiles.sections() if s.startswith("Profile")]
            default_paths = []
            for section in profiles.sections():
                if section.startswith("Install"):
                    path = profiles.get(section, "Default", fallback=None)
                    if path:
                        default_paths.append(path)
            for section in profile_sections:
                if profiles.getboolean(section, "Default", fallback=False):
                    path = profiles.get(section, "Path", fallback=None)
                    if path:
                        default_paths.append(path)
            for profile in default_paths:
                profile_path = profile if os.path.isabs(profile) else os.path.join(firefox_root, profile)
                if os.path.isfile(os.path.join(profile_path, "cookies.sqlite")):
                    browsers["Firefox (Flatpak)"] = f"firefox:{profile_path}"
                    break

        def add_chromium_profile(label: str, browser: str, root: str) -> None:
            candidates = [os.path.join(root, "Default")]
            candidates.extend(sorted(glob.glob(os.path.join(root, "Profile *"))))
            for profile in candidates:
                if os.path.isfile(os.path.join(profile, "Cookies")) or os.path.isfile(os.path.join(profile, "Network", "Cookies")):
                    browsers[label] = f"{browser}:{profile}"
                    return

        add_chromium_profile(
            "Brave (Flatpak)", "brave",
            os.path.join(flatpak_root, "com.brave.Browser/config/BraveSoftware/Brave-Browser")
        )
        add_chromium_profile(
            "Chrome (Flatpak)", "chrome",
            os.path.join(flatpak_root, "com.google.Chrome/config/google-chrome")
        )
        return browsers

    def import_browser_cookies(self, browser_spec: str) -> bool:
        """Import Flatpak browser cookies into a filtered, private app jar."""
        import tempfile

        fd, tmp_path = tempfile.mkstemp(dir=self.config_dir, prefix="ytm_import_", suffix=".txt")
        with os.fdopen(fd, "w") as temp_file:
            temp_file.write("# Netscape HTTP Cookie File\n")

        cmd = [
            self._yt_dlp,
            "--cookies-from-browser", browser_spec,
            "--cookies", tmp_path,
            "--dump-json",
            "ytsearch1:ping",
            "--no-download"
        ]
        try:
            res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=15)
            if res.returncode != 0 or not os.path.exists(tmp_path):
                return False
            if not self._filter_cookies_file(source_file=tmp_path):
                return False
            os.replace(tmp_path, self.cookies_path)
            return True
        except (OSError, subprocess.SubprocessError) as e:
            print(f"[Cookie Import Warning] {type(e).__name__}")
            return False
        finally:
            if os.path.exists(tmp_path):
                try:
                    os.remove(tmp_path)
                except OSError:
                    pass

    def _filter_cookies_file(self, source_file: str) -> bool:
        """Keep only valid Google/YouTube cookies and atomically write them mode 0600."""
        import tempfile

        if not os.path.exists(source_file):
            return False
        kept_lines = ["# Netscape HTTP Cookie File\n", "# Filtered by Banshee.\n\n"]
        auth_cookie_names = {"sid", "sapisid", "__secure-3psid", "__secure-1psid", "login_info"}
        has_auth_cookie = False

        with open(source_file, "r", errors="ignore") as cookie_file:
            for line in cookie_file:
                raw = line.rstrip("\r\n")
                if not raw or (raw.startswith("#") and not raw.startswith("#HttpOnly_")):
                    continue
                parts = raw.split("\t")
                if len(parts) != 7:
                    continue

                domain_field = parts[0]
                clean_domain = domain_field.removeprefix("#HttpOnly_").lower().lstrip(".")
                if not (
                    clean_domain == "youtube.com"
                    or clean_domain.endswith(".youtube.com")
                    or clean_domain == "google.com"
                    or clean_domain.endswith(".google.com")
                ):
                    continue

                kept_lines.append("\t".join(parts) + "\n")
                if parts[5].lower() in auth_cookie_names:
                    has_auth_cookie = True

        fd, filtered_path = tempfile.mkstemp(dir=os.path.dirname(source_file), prefix="ytm_filtered_", suffix=".txt")
        try:
            with os.fdopen(fd, "w") as filtered_file:
                filtered_file.writelines(kept_lines)
            os.chmod(filtered_path, 0o600)
            os.replace(filtered_path, source_file)
        finally:
            if os.path.exists(filtered_path):
                os.remove(filtered_path)
        return has_auth_cookie

    def load_cookie_file(self, source_path: str) -> bool:
        """Import an exported Netscape jar without persisting unrelated cookies."""
        import tempfile

        fd, tmp_path = tempfile.mkstemp(dir=self.config_dir, prefix="ytm_import_", suffix=".txt")
        os.close(fd)
        try:
            shutil.copyfile(source_path, tmp_path)
            if not self._filter_cookies_file(source_file=tmp_path):
                return False
            os.replace(tmp_path, self.cookies_path)
            return True
        except OSError as e:
            print(f"[Cookie Copy Error] {type(e).__name__}")
            return False
        finally:
            if os.path.exists(tmp_path):
                os.remove(tmp_path)
