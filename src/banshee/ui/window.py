"""
Iconic Banshee Mini Mode Window implementation using GTK4 and Libadwaita.
"""
import os
import threading
import urllib.request
import gi

gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
gi.require_version('Gdk', '4.0')
gi.require_version('GdkPixbuf', '2.0')
from gi.repository import Gtk, Adw, Gdk, GdkPixbuf, GLib

from banshee.models import PlaybackState, Track
from banshee.ui.popovers import SearchPopover, QueuePopover, VolumePopover, AuthDialog

CSS_STYLING = """
.banshee-mini-window {
    background-color: @window_bg_color;
    border-radius: 12px;
}
.banshee-cover-art {
    border-radius: 8px;
    background-color: @card_bg_color;
}
.banshee-track-title {
    font-weight: 700;
    font-size: 1.05em;
}
.banshee-track-artist {
    color: alpha(@window_fg_color, 0.7);
    font-size: 0.85em;
}
.banshee-time-label {
    font-size: 0.75em;
    font-feature-settings: "tnum";
    color: alpha(@window_fg_color, 0.6);
}
.banshee-play-btn {
    border-radius: 9999px;
    min-width: 36px;
    min-height: 36px;
    padding: 0;
}
"""

class MiniModeWindow(Adw.ApplicationWindow):
    def __init__(self, app, player, queue, source):
        super().__init__(application=app, title="Banshee")
        self.player = player
        self.queue = queue
        self.source = source
        self._user_seeking = False

        self._load_css()

        # Window settings for strict Mini Mode
        self.set_default_size(380, 82)
        self.set_resizable(True)
        self.add_css_class("banshee-mini-window")

        # Main horizontal layout
        root_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)

        # Compact custom headerbar
        self.headerbar = Adw.HeaderBar()
        self.headerbar.add_css_class("flat")
        self.headerbar.set_show_title(False)

        # Right header: Search, Queue, Volume, Auth
        self.btn_search = Gtk.Button(icon_name="edit-find-symbolic")
        self.btn_search.add_css_class("flat")
        self.btn_search.set_tooltip_text("Search Tracks")
        self.headerbar.pack_end(self.btn_search)

        self.btn_queue = Gtk.Button(icon_name="view-list-symbolic")
        self.btn_queue.add_css_class("flat")
        self.btn_queue.set_tooltip_text("Playback Queue")
        self.headerbar.pack_end(self.btn_queue)

        self.btn_volume = Gtk.Button(icon_name="audio-volume-high-symbolic")
        self.btn_volume.add_css_class("flat")
        self.btn_volume.set_tooltip_text("Volume")
        self.headerbar.pack_end(self.btn_volume)

        self.btn_auth = Gtk.Button(icon_name="avatar-default-symbolic")
        self.btn_auth.add_css_class("flat")
        self.btn_auth.set_tooltip_text("YouTube Music Login")
        self.btn_auth.connect("clicked", self._open_auth_dialog)
        self.headerbar.pack_end(self.btn_auth)

        root_box.append(self.headerbar)

        # Body: [ Cover Art ] [ Info & Progress ] [ Prev Play Next ]
        body = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        body.set_margin_start(10)
        body.set_margin_end(10)
        body.set_margin_bottom(10)

        # 1. Cover Art (60x60)
        self.cover_image = Gtk.Image.new_from_icon_name("audio-x-generic-symbolic")
        self.cover_image.set_pixel_size(56)
        self.cover_image.add_css_class("banshee-cover-art")
        body.append(self.cover_image)

        # 2. Center: Info + Scrubber
        center_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        center_box.set_hexpand(True)
        center_box.set_valign(Gtk.Align.CENTER)

        self.lbl_title = Gtk.Label(label="Banshee Mini")
        self.lbl_title.add_css_class("banshee-track-title")
        self.lbl_title.set_halign(Gtk.Align.START)
        self.lbl_title.set_ellipsize(3)  # Pango.EllipsizeMode.END
        center_box.append(self.lbl_title)

        self.lbl_artist = Gtk.Label(label="Ready to Play")
        self.lbl_artist.add_css_class("banshee-track-artist")
        self.lbl_artist.set_halign(Gtk.Align.START)
        self.lbl_artist.set_ellipsize(3)
        center_box.append(self.lbl_artist)

        # Scrubber + Timestamps
        progress_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self.lbl_time_cur = Gtk.Label(label="00:00")
        self.lbl_time_cur.add_css_class("banshee-time-label")
        progress_box.append(self.lbl_time_cur)

        self.scale_progress = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0.0, 100.0, 1.0)
        self.scale_progress.set_hexpand(True)
        self.scale_progress.set_draw_value(False)
        self.scale_progress.connect("change-value", self._on_user_seek)
        progress_box.append(self.scale_progress)

        self.lbl_time_dur = Gtk.Label(label="--:--")
        self.lbl_time_dur.add_css_class("banshee-time-label")
        progress_box.append(self.lbl_time_dur)

        center_box.append(progress_box)
        body.append(center_box)

        # 3. Controls: Prev, Play/Pause, Next
        controls_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
        controls_box.set_valign(Gtk.Align.CENTER)

        self.btn_prev = Gtk.Button(icon_name="media-skip-backward-symbolic")
        self.btn_prev.add_css_class("flat")
        self.btn_prev.connect("clicked", self._on_prev)
        controls_box.append(self.btn_prev)

        self.btn_play = Gtk.Button(icon_name="media-playback-start-symbolic")
        self.btn_play.add_css_class("suggested-action")
        self.btn_play.add_css_class("banshee-play-btn")
        self.btn_play.connect("clicked", self._on_play_toggle)
        controls_box.append(self.btn_play)

        self.btn_next = Gtk.Button(icon_name="media-skip-forward-symbolic")
        self.btn_next.add_css_class("flat")
        self.btn_next.connect("clicked", self._on_next)
        controls_box.append(self.btn_next)

        body.append(controls_box)
        root_box.append(body)
        self.set_content(root_box)

        # Initialize Popovers
        self.search_popover = SearchPopover(self.source, self._on_track_selected_from_search)
        self.search_popover.set_parent(self.btn_search)
        self.btn_search.connect("clicked", lambda b: self.search_popover.popup())

        self.queue_popover = QueuePopover(self.queue, self._on_skip_to_queue_index, self._on_remove_queue_index)
        self.queue_popover.set_parent(self.btn_queue)
        self.btn_queue.connect("clicked", lambda b: self.queue_popover.popup())

        self.volume_popover = VolumePopover(self.player)
        self.volume_popover.set_parent(self.btn_volume)
        self.btn_volume.connect("clicked", lambda b: self.volume_popover.popup())

        # Connect GObject signals from player
        self.player.connect("state-changed", lambda p, s: self._on_player_state_changed(s))
        self.player.connect("position-changed", lambda p, pos, dur: self._on_player_position_changed(pos, dur))
        self.player.connect("track-finished", lambda p: self._on_next())
        self.player.connect("playback-error", lambda p, msg: self._on_playback_error(msg))
    def _load_css(self):
        provider = Gtk.CssProvider()
        provider.load_from_data(CSS_STYLING.encode('utf-8'))
        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(),
            provider,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
        )


    def _on_playback_error(self, err_msg: str):
        def update():
            self.lbl_artist.set_text(f"Error: {err_msg}")
            self.btn_play.set_icon_name("media-playback-start-symbolic")
        GLib.idle_add(update)

    def _open_auth_dialog(self, btn):
        dialog = AuthDialog(self, self.source, self._on_auth_changed)
        dialog.present()

    def _on_auth_changed(self, is_authed: bool):
        if is_authed:
            self.btn_auth.add_css_class("accent")
        else:
            self.btn_auth.remove_css_class("accent")

    def _on_play_toggle(self, btn):
        if not self.queue.current_track() and self.queue.tracks:
            self._play_track(self.queue.tracks[0], 0)
        else:
            self.player.toggle_play()

    def _on_prev(self, btn):
        track = self.queue.prev()
        if track:
            self._play_track(track, self.queue.current_index)

    def _on_next(self, btn=None):
        track = self.queue.next()
        if track:
            self._play_track(track, self.queue.current_index)
        else:
            self.player.stop()

    def _on_track_selected_from_search(self, track: Track, play_now: bool):
        if play_now:
            self.queue.add(track, play_next=True)
            self._play_track(track, self.queue.current_index + 1)
        else:
            self.queue.add(track)

    def _on_skip_to_queue_index(self, index: int):
        if 0 <= index < len(self.queue.tracks):
            track = self.queue.tracks[index]
            self._play_track(track, index)

    def _on_remove_queue_index(self, index: int):
        self.queue.remove(index)

    def _play_track(self, track: Track, index: int):
        self.queue.current_index = index
        self.lbl_title.set_text(track.title)
        self.lbl_artist.set_text(track.artist)
        self.lbl_time_cur.set_text("00:00")
        self.lbl_time_dur.set_text(track.duration_str())

        # Async resolve stream URL and thumbnail
        def worker():
            try:
                if not track.stream_url:
                    track.stream_url = self.source.get_stream_url(track)
                GLib.idle_add(lambda: self.player.load_track(track, autoplay=True))
            except Exception as e:
                print(f"[Stream Resolve Error] {e}")

            if track.thumbnail_url:
                self._load_thumbnail(track.thumbnail_url)

        threading.Thread(target=worker, daemon=True).start()

    def _load_thumbnail(self, url: str):
        try:
            req = urllib.request.Request(url, headers={'User-Agent': 'Mozilla/5.0'})
            with urllib.request.urlopen(req, timeout=5) as resp:
                data = resp.read()
            loader = GdkPixbuf.PixbufLoader()
            loader.write(data)
            loader.close()
            pixbuf = loader.get_pixbuf()
            if pixbuf:
                scaled = pixbuf.scale_simple(56, 56, GdkPixbuf.InterpType.BILINEAR)
                GLib.idle_add(lambda: self.cover_image.set_from_pixbuf(scaled))
        except Exception:
            pass

    def _on_player_state_changed(self, state: PlaybackState):
        GLib.idle_add(self._update_playback_ui, state)

    def _update_playback_ui(self, state: PlaybackState):
        if state == PlaybackState.PLAYING:
            self.btn_play.set_icon_name("media-playback-pause-symbolic")
        else:
            self.btn_play.set_icon_name("media-playback-start-symbolic")

    def _on_player_position_changed(self, pos: float, dur: float):
        if not self._user_seeking:
            GLib.idle_add(self._update_progress_ui, pos, dur)

    def _update_progress_ui(self, pos: float, dur: float):
        if dur > 0:
            self.scale_progress.set_range(0.0, dur)
            self.scale_progress.set_value(pos)
            mins_c, secs_c = int(pos) // 60, int(pos) % 60
            mins_d, secs_d = int(dur) // 60, int(dur) % 60
            self.lbl_time_cur.set_text(f"{mins_c:02d}:{secs_c:02d}")
            self.lbl_time_dur.set_text(f"{mins_d:02d}:{secs_d:02d}")

    def _on_user_seek(self, scale, scroll_type, value):
        self._user_seeking = True
        self.player.seek(value)
        GLib.timeout_add(300, lambda: setattr(self, '_user_seeking', False) or False)
        return False
