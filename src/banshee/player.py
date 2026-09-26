"""
GStreamer playbin3 playback engine as a GObject with signals.
"""
import gi
gi.require_version('Gst', '1.0')
gi.require_version('GLib', '2.0')
gi.require_version('GObject', '2.0')
from gi.repository import Gst, GLib, GObject
from banshee.models import PlaybackState, Track

class Player(GObject.Object):
    __gsignals__ = {
        'state-changed': (GObject.SignalFlags.RUN_FIRST, None, (object,)),
        'position-changed': (GObject.SignalFlags.RUN_FIRST, None, (float, float)),
        'track-finished': (GObject.SignalFlags.RUN_FIRST, None, ()),
        'playback-error': (GObject.SignalFlags.RUN_FIRST, None, (str,)),
    }

    def __init__(self):
        super().__init__()
        Gst.init(None)
        self.playbin = Gst.ElementFactory.make("playbin3", "banshee-playbin")
        if not self.playbin:
            self.playbin = Gst.ElementFactory.make("playbin", "banshee-playbin")
        
        self.current_track: Track | None = None
        self.state = PlaybackState.STOPPED
        self.volume = 1.0  # 0.0 to 1.0
        self.muted = False
        self._prev_volume = 1.0

        bus = self.playbin.get_bus()
        bus.add_signal_watch()
        bus.connect("message", self._on_bus_message)

        # Periodic ticker for position reporting
        GLib.timeout_add(250, self._tick_position)

    def load_track(self, track: Track, autoplay: bool = True):
        self.stop()
        self.current_track = track
        if not track.stream_url:
            self.emit('playback-error', "Track has no stream URL")
            return

        self.playbin.set_property("uri", track.stream_url)
        if autoplay:
            self.play()

    def play(self):
        if not self.current_track:
            return
        res = self.playbin.set_state(Gst.State.PLAYING)
        if res == Gst.StateChangeReturn.FAILURE:
            self._set_state(PlaybackState.STOPPED)
            self.emit('playback-error', "Failed to start GStreamer playback")
        else:
            self._set_state(PlaybackState.PLAYING)

    def pause(self):
        self.playbin.set_state(Gst.State.PAUSED)
        self._set_state(PlaybackState.PAUSED)

    def toggle_play(self):
        if self.state == PlaybackState.PLAYING:
            self.pause()
        else:
            self.play()

    def stop(self):
        self.playbin.set_state(Gst.State.NULL)
        self._set_state(PlaybackState.STOPPED)

    def seek(self, position_seconds: float):
        if self.state == PlaybackState.STOPPED:
            return
        seek_ns = int(max(0, position_seconds) * Gst.SECOND)
        self.playbin.seek_simple(
            Gst.Format.TIME,
            Gst.SeekFlags.FLUSH | Gst.SeekFlags.KEY_UNIT,
            seek_ns
        )

    def set_volume(self, volume: float):
        self.volume = max(0.0, min(1.0, volume))
        if not self.muted:
            self.playbin.set_property("volume", self.volume)

    def toggle_mute(self) -> bool:
        self.muted = not self.muted
        if self.muted:
            self._prev_volume = self.volume
            self.playbin.set_property("volume", 0.0)
        else:
            self.playbin.set_property("volume", self.volume)
        return self.muted

    def get_position(self) -> tuple[float, float]:
        """Returns (position_seconds, duration_seconds)."""
        pos_ok, pos = self.playbin.query_position(Gst.Format.TIME)
        dur_ok, dur = self.playbin.query_duration(Gst.Format.TIME)
        p = pos / Gst.SECOND if pos_ok else 0.0
        d = dur / Gst.SECOND if dur_ok else (self.current_track.duration if self.current_track else 0.0)
        return (p, d)

    def _set_state(self, new_state: PlaybackState):
        if self.state != new_state:
            self.state = new_state
            self.emit('state-changed', new_state)

    def _on_bus_message(self, bus, msg):
        t = msg.type
        if t == Gst.MessageType.EOS:
            self._set_state(PlaybackState.STOPPED)
            self.emit('track-finished')
        elif t == Gst.MessageType.ERROR:
            err, dbg = msg.parse_error()
            self._set_state(PlaybackState.STOPPED)
            self.emit('playback-error', f"Playback error: {err.message}")
        elif t == Gst.MessageType.BUFFERING:
            percent = msg.parse_buffering()
            if percent < 100:
                self._set_state(PlaybackState.BUFFERING)
            elif self.state == PlaybackState.BUFFERING:
                self._set_state(PlaybackState.PLAYING)
        return True

    def _tick_position(self) -> bool:
        if self.state in (PlaybackState.PLAYING, PlaybackState.BUFFERING):
            pos, dur = self.get_position()
            self.emit('position-changed', pos, dur)
        return True
