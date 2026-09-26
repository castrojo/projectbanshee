import os
import sys
import unittest
from unittest.mock import MagicMock, patch

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "../src")))

from banshee.models import Track, PlaybackState
from banshee.player import Player

class TestPlayer(unittest.TestCase):
    def setUp(self):
        self.player = Player()

    def test_volume_bounds(self):
        self.player.set_volume(0.75)
        self.assertEqual(self.player.volume, 0.75)

        self.player.set_volume(1.5)
        self.assertEqual(self.player.volume, 1.0)

        self.player.set_volume(-0.5)
        self.assertEqual(self.player.volume, 0.0)

    def test_mute_toggle(self):
        self.player.set_volume(0.8)
        muted = self.player.toggle_mute()
        self.assertTrue(muted)
        self.assertTrue(self.player.muted)

        unmuted = self.player.toggle_mute()
        self.assertFalse(unmuted)
        self.assertFalse(self.player.muted)
        self.assertEqual(self.player.volume, 0.8)

    def test_state_changed_signal(self):
        received_states = []
        self.player.connect("state-changed", lambda p, s: received_states.append(s))
        
        self.player._set_state(PlaybackState.PLAYING)
        self.assertEqual(received_states, [PlaybackState.PLAYING])

        self.player._set_state(PlaybackState.PAUSED)
        self.assertEqual(received_states, [PlaybackState.PLAYING, PlaybackState.PAUSED])

    def test_load_track_missing_url(self):
        errors = []
        self.player.connect("playback-error", lambda p, err: errors.append(err))
        track = Track(id="1", title="No URL", artist="Artist", stream_url="")
        self.player.load_track(track)
        self.assertEqual(len(errors), 1)
        self.assertIn("no stream url", errors[0].lower())

if __name__ == "__main__":
    unittest.main()
