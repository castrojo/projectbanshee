import json
import os
import sys
import unittest
from unittest.mock import patch, MagicMock
from tempfile import TemporaryDirectory

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "../src")))

from banshee.models import Track
from banshee.sources.ytm import YouTubeMusicSource

class TestYouTubeMusicSource(unittest.TestCase):
    def setUp(self):
        self.source = YouTubeMusicSource(cookies_path="/tmp/nonexistent_cookies.txt")

    def test_name(self):
        self.assertEqual(self.source.name, "YouTube Music")

    def test_is_authenticated_false_when_empty_or_missing(self):
        self.assertFalse(self.source.is_authenticated())

    @patch("subprocess.run")
    def test_search_parsing(self, mock_run):
        mock_output = "\n".join([
            json.dumps({
                "id": "vid123",
                "title": "Around the World",
                "uploader": "Daft Punk",
                "album": "Homework",
                "duration": 429,
                "thumbnail": "https://example.com/thumb.jpg"
            }),
            json.dumps({
                "id": "vid456",
                "title": "One More Time",
                "uploader": "Daft Punk",
                "duration": 320,
                "thumbnails": [{"url": "https://example.com/thumb2.jpg"}]
            })
        ])
        mock_run.return_value = MagicMock(stdout=mock_output, returncode=0)

        tracks = self.source.search("Daft Punk", limit=2)
        self.assertEqual(len(tracks), 2)

        t1 = tracks[0]
        self.assertEqual(t1.id, "vid123")
        self.assertEqual(t1.title, "Around the World")
        self.assertEqual(t1.artist, "Daft Punk")
        self.assertEqual(t1.duration, 429.0)
        self.assertEqual(t1.thumbnail_url, "https://example.com/thumb.jpg")

        t2 = tracks[1]
        self.assertEqual(t2.id, "vid456")
        self.assertEqual(t2.thumbnail_url, "https://example.com/thumb2.jpg")

    @patch("subprocess.run")
    def test_get_stream_url(self, mock_run):
        mock_run.return_value = MagicMock(stdout="https://googlevideo.com/playback_stream\n", returncode=0)
        track = Track(id="xyz", title="Sample", artist="Artist")
        url = self.source.get_stream_url(track)
        self.assertEqual(url, "https://googlevideo.com/playback_stream")

    def test_filter_cookie_file_keeps_httponly_auth_and_drops_other_domains(self):
        with TemporaryDirectory() as tmpdir:
            cookie_path = os.path.join(tmpdir, "cookies.txt")
            with open(cookie_path, "w") as cookie_file:
                cookie_file.write("# Netscape HTTP Cookie File\n")
                cookie_file.write("#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1999999999\tSAPISID\tfake-secret\n")
                cookie_file.write("#HttpOnly_accounts.google.com\tFALSE\t/\tTRUE\t1999999999\t__Secure-3PSID\tfake-secret\n")
                cookie_file.write("discord.com\tFALSE\t/\tTRUE\t1999999999\tcf_clearance\tfake-secret\n")
                cookie_file.write("evilyoutube.com\tFALSE\t/\tTRUE\t1999999999\tSID\tfake-secret\n")
                cookie_file.write(".youtube.com\tTRUE\t/\tTRUE\t1999999999\tSID\tfake-secret\tLax\n")

            self.assertTrue(self.source._filter_cookies_file(cookie_path))
            self.assertEqual(os.stat(cookie_path).st_mode & 0o777, 0o600)
            with open(cookie_path) as cookie_file:
                saved = cookie_file.read()
            self.assertIn("#HttpOnly_.youtube.com", saved)
            self.assertIn("#HttpOnly_accounts.google.com", saved)
            self.assertNotIn("discord.com", saved)
            self.assertNotIn("evilyoutube.com", saved)
            self.assertNotIn("\tLax", saved)

    def test_filter_cookie_file_requires_auth_cookie(self):
        with TemporaryDirectory() as tmpdir:
            cookie_path = os.path.join(tmpdir, "cookies.txt")
            with open(cookie_path, "w") as cookie_file:
                cookie_file.write(".youtube.com\tTRUE\t/\tTRUE\t1999999999\tVISITOR_INFO1_LIVE\tvisitor\n")

            self.assertFalse(self.source._filter_cookies_file(cookie_path))

if __name__ == "__main__":
    unittest.main()
