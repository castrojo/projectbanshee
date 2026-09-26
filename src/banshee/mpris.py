"""
MPRIS v2 D-Bus interface export for GNOME Shell and playerctl integration.
"""
import gi
gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib

from banshee.models import PlaybackState, Track

MPRIS_INTROSPECTION_XML = """
<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.freedesktop.DBus.Properties">
    <method name="Get">
      <arg direction="in" type="s" name="interface_name"/>
      <arg direction="in" type="s" name="property_name"/>
      <arg direction="out" type="v" name="value"/>
    </method>
    <method name="GetAll">
      <arg direction="in" type="s" name="interface_name"/>
      <arg direction="out" type="a{sv}" name="properties"/>
    </method>
    <method name="Set">
      <arg direction="in" type="s" name="interface_name"/>
      <arg direction="in" type="s" name="property_name"/>
      <arg direction="in" type="v" name="value"/>
    </method>
    <signal name="PropertiesChanged">
      <arg type="s" name="interface_name"/>
      <arg type="a{sv}" name="changed_properties"/>
      <arg type="as" name="invalidated_properties"/>
    </signal>
  </interface>
  <interface name="org.mpris.MediaPlayer2">
    <method name="Raise"/>
    <method name="Quit"/>
    <property name="CanQuit" type="b" access="read"/>
    <property name="CanRaise" type="b" access="read"/>
    <property name="HasTrackList" type="b" access="read"/>
    <property name="Identity" type="s" access="read"/>
    <property name="DesktopEntry" type="s" access="read"/>
    <property name="SupportedUriSchemes" type="as" access="read"/>
    <property name="SupportedMimeTypes" type="as" access="read"/>
  </interface>
  <interface name="org.mpris.MediaPlayer2.Player">
    <method name="Next"/>
    <method name="Previous"/>
    <method name="Pause"/>
    <method name="PlayPause"/>
    <method name="Stop"/>
    <method name="Play"/>
    <method name="Seek">
      <arg direction="in" type="x" name="Offset"/>
    </method>
    <method name="SetPosition">
      <arg direction="in" type="o" name="TrackId"/>
      <arg direction="in" type="x" name="Position"/>
    </method>
    <method name="OpenUri">
      <arg direction="in" type="s" name="Uri"/>
    </method>
    <property name="PlaybackStatus" type="s" access="read"/>
    <property name="Rate" type="d" access="readwrite"/>
    <property name="Metadata" type="a{sv}" access="read"/>
    <property name="Volume" type="d" access="readwrite"/>
    <property name="Position" type="x" access="read"/>
    <property name="MinimumRate" type="d" access="read"/>
    <property name="MaximumRate" type="d" access="read"/>
    <property name="CanGoNext" type="b" access="read"/>
    <property name="CanGoPrevious" type="b" access="read"/>
    <property name="CanPlay" type="b" access="read"/>
    <property name="CanPause" type="b" access="read"/>
    <property name="CanSeek" type="b" access="read"/>
    <property name="CanControl" type="b" access="read"/>
  </interface>
</node>
"""

class MPRISService:
    def __init__(self, app, player, queue):
        self.app = app
        self.player = player
        self.queue = queue
        self.bus = None
        self.registration_id = 0

        self.node_info = Gio.DBusNodeInfo.new_for_xml(MPRIS_INTROSPECTION_XML)
        
        # Connect to GObject signal
        self.player.connect("state-changed", self._on_player_state_changed)

        Gio.bus_own_name(
            Gio.BusType.SESSION,
            "org.mpris.MediaPlayer2.banshee",
            Gio.BusNameOwnerFlags.NONE,
            self._on_bus_acquired,
            None,
            None
        )

    def _on_player_state_changed(self, player, state):
        self.notify_property_changed("org.mpris.MediaPlayer2.Player", {
            "PlaybackStatus": GLib.Variant("s", self._playback_status()),
            "Metadata": GLib.Variant("a{sv}", self._metadata())
        })

    def _on_bus_acquired(self, connection, name):
        self.bus = connection
        for iface in self.node_info.interfaces:
            if iface.name in ("org.mpris.MediaPlayer2", "org.mpris.MediaPlayer2.Player"):
                connection.register_object(
                    "/org/mpris/MediaPlayer2",
                    iface,
                    self._handle_method_call,
                    self._handle_get_property,
                    self._handle_set_property
                )

    def _playback_status(self) -> str:
        if self.player.state == PlaybackState.PLAYING:
            return "Playing"
        elif self.player.state == PlaybackState.PAUSED:
            return "Paused"
        return "Stopped"

    def _metadata(self) -> dict:
        track = self.queue.current_track()
        if not track:
            return {}
        
        safe_id = "".join([c if (c.isalnum() or c == '_') else f"_{ord(c):02x}" for c in (track.id or "0")])
        meta = {
            "mpris:trackId": GLib.Variant("o", f"/org/mpris/MediaPlayer2/track/{safe_id}"),
            "xesam:title": GLib.Variant("s", track.title),
            "xesam:artist": GLib.Variant("as", [track.artist]),
            "xesam:album": GLib.Variant("s", track.album or "YouTube Music"),
            "mpris:length": GLib.Variant("x", int(track.duration * 1000000))
        }
        if track.thumbnail_url:
            meta["mpris:artUrl"] = GLib.Variant("s", track.thumbnail_url)
        return meta

    def _handle_get_property(self, connection, sender, path, iface_name, prop_name):
        if iface_name == "org.mpris.MediaPlayer2":
            props = {
                "CanQuit": GLib.Variant("b", True),
                "CanRaise": GLib.Variant("b", True),
                "HasTrackList": GLib.Variant("b", False),
                "Identity": GLib.Variant("s", "Banshee"),
                "DesktopEntry": GLib.Variant("s", "io.github.castrojo.Banshee"),
                "SupportedUriSchemes": GLib.Variant("as", ["http", "https"]),
                "SupportedMimeTypes": GLib.Variant("as", ["audio/mpeg", "audio/ogg", "audio/webm"])
            }
            return props.get(prop_name)
        elif iface_name == "org.mpris.MediaPlayer2.Player":
            pos, _ = self.player.get_position()
            props = {
                "PlaybackStatus": GLib.Variant("s", self._playback_status()),
                "Rate": GLib.Variant("d", 1.0),
                "Metadata": GLib.Variant("a{sv}", self._metadata()),
                "Volume": GLib.Variant("d", self.player.volume),
                "Position": GLib.Variant("x", int(pos * 1000000)),
                "MinimumRate": GLib.Variant("d", 1.0),
                "MaximumRate": GLib.Variant("d", 1.0),
                "CanGoNext": GLib.Variant("b", len(self.queue.tracks) > 1),
                "CanGoPrevious": GLib.Variant("b", len(self.queue.tracks) > 1),
                "CanPlay": GLib.Variant("b", bool(self.queue.tracks)),
                "CanPause": GLib.Variant("b", True),
                "CanSeek": GLib.Variant("b", True),
                "CanControl": GLib.Variant("b", True),
            }
            return props.get(prop_name)
        return None

    def _handle_set_property(self, connection, sender, path, iface_name, prop_name, value):
        if iface_name == "org.mpris.MediaPlayer2.Player" and prop_name == "Volume":
            self.player.set_volume(value.get_double())
            return True
        return False

    def _handle_method_call(self, connection, sender, path, iface_name, method_name, parameters, invocation):
        if iface_name == "org.mpris.MediaPlayer2":
            if method_name == "Raise":
                win = self.app.get_active_window()
                if win:
                    win.present()
                invocation.return_value(None)
            elif method_name == "Quit":
                self.app.quit()
                invocation.return_value(None)
        elif iface_name == "org.mpris.MediaPlayer2.Player":
            if method_name == "Play":
                self.player.play()
            elif method_name == "Pause":
                self.player.pause()
            elif method_name == "PlayPause":
                self.player.toggle_play()
            elif method_name == "Stop":
                self.player.stop()
            elif method_name == "Next":
                win = self.app.get_active_window()
                if win:
                    win._on_next()
            elif method_name == "Previous":
                win = self.app.get_active_window()
                if win:
                    win._on_prev(None)
            elif method_name == "Seek":
                offset_usec = parameters.unpack()[0]
                pos, dur = self.player.get_position()
                new_pos = max(0.0, pos + (offset_usec / 1000000.0))
                self.player.seek(new_pos)
            elif method_name == "SetPosition":
                track_id, pos_usec = parameters.unpack()
                self.player.seek(pos_usec / 1000000.0)
            invocation.return_value(None)

    def notify_property_changed(self, iface_name: str, changed_props: dict):
        if not self.bus:
            return
        self.bus.emit_signal(
            None,
            "/org/mpris/MediaPlayer2",
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            GLib.Variant("(sa{sv}as)", (iface_name, changed_props, []))
        )

