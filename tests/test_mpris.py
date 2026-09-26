import os
import sys
import unittest
from unittest.mock import MagicMock

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "../src")))

from banshee.models import Track, Queue, PlaybackState
from banshee.player import Player
from banshee.mpris import MPRISService

class TestMPRISService(unittest.TestCase):
    def setUp(self):
        self.app = MagicMock()
        self.player = Player()
        self.queue = Queue()
        self.mpris = MPRISService(self.app, self.player, self.queue)

    def test_playback_status(self):
        self.assertEqual(self.mpris._playback_status(), "Stopped")

        self.player._set_state(PlaybackState.PLAYING)
        self.assertEqual(self.mpris._playback_status(), "Playing")

        self.player._set_state(PlaybackState.PAUSED)
        self.assertEqual(self.mpris._playback_status(), "Paused")

    def test_metadata_generation(self):
        track = Track(
            id="abc12345",
            title="Starboy",
            artist="The Weeknd",
            album="Starboy",
            duration=230.5,
            thumbnail_url="https://example.com/art.jpg"
        )
        self.queue.add(track)
        self.queue.next()

        meta = self.mpris._metadata()
        self.assertEqual(meta["xesam:title"].get_string(), "Starboy")
        self.assertEqual(meta["xesam:artist"].unpack(), ["The Weeknd"])
        self.assertEqual(meta["xesam:album"].get_string(), "Starboy")
        self.assertEqual(meta["mpris:length"].get_int64(), 230500000)
        self.assertEqual(meta["mpris:artUrl"].get_string(), "https://example.com/art.jpg")

        res = self.mpris._handle_get_property(
            None, None, "/org/mpris/MediaPlayer2", "org.mpris.MediaPlayer2", "Identity"
        )
        self.assertEqual(res.get_string(), "Banshee")

if __name__ == "__main__":
    unittest.main()
