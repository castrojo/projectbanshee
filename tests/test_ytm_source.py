import json
import os
import sys
import unittest
from unittest.mock import patch, MagicMock

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

if __name__ == "__main__":
    unittest.main()
