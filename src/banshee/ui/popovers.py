"""
Popovers for Search, Queue, Volume, and Settings/Auth Dialog.
"""
import os
import threading
import gi
gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
from gi.repository import Gtk, Adw, GLib
from banshee.models import Track

class SearchPopover(Gtk.Popover):
    def __init__(self, source, on_track_selected):
        super().__init__()
        self.source = source
        self.on_track_selected = on_track_selected

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        box.set_size_request(320, 360)
        box.set_margin_top(8)
        box.set_margin_bottom(8)
        box.set_margin_start(8)
        box.set_margin_end(8)

        # Search Entry
        self.entry = Gtk.SearchEntry()
        self.entry.set_placeholder_text("Search YouTube Music...")
        self.entry.connect("activate", self._on_search)
        box.append(self.entry)

        # Spinner
        self.spinner = Gtk.Spinner()
        self.spinner.set_visible(False)
        box.append(self.spinner)

        # Scrolled Results List
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_vexpand(True)
        self.list_box = Gtk.ListBox()
        self.list_box.set_selection_mode(Gtk.SelectionMode.NONE)
        self.list_box.add_css_class("boxed-list")
        scrolled.set_child(self.list_box)
        box.append(scrolled)

        self.set_child(box)

    def _on_search(self, entry):
        query = entry.get_text().strip()
        if not query:
            return

        self.spinner.set_visible(True)
        self.spinner.start()

        # Clear existing
        while True:
            row = self.list_box.get_row_at_index(0)
            if not row:
                break
            self.list_box.remove(row)

        def worker():
            tracks = self.source.search(query, limit=12)
            GLib.idle_add(self._display_results, tracks)

        threading.Thread(target=worker, daemon=True).start()

    def _display_results(self, tracks):
        self.spinner.stop()
        self.spinner.set_visible(False)

        if not tracks:
            row = Adw.ActionRow(title="No tracks found")
            self.list_box.append(row)
            return

        for track in tracks:
            row = Adw.ActionRow(title=track.title, subtitle=f"{track.artist} • {track.duration_str()}")
            
            btn_play = Gtk.Button(icon_name="media-playback-start-symbolic")
            btn_play.set_valign(Gtk.Align.CENTER)
            btn_play.add_css_class("flat")
            btn_play.set_tooltip_text("Play Now")
            btn_play.connect("clicked", lambda b, t=track: self._select(t, play_now=True))
            row.add_suffix(btn_play)

            btn_queue = Gtk.Button(icon_name="list-add-symbolic")
            btn_queue.set_valign(Gtk.Align.CENTER)
            btn_queue.add_css_class("flat")
            btn_queue.set_tooltip_text("Add to Queue")
            btn_queue.connect("clicked", lambda b, t=track: self._select(t, play_now=False))
            row.add_suffix(btn_queue)

            self.list_box.append(row)

    def _select(self, track: Track, play_now: bool):
        self.popdown()
        self.on_track_selected(track, play_now)


class QueuePopover(Gtk.Popover):
    def __init__(self, queue, on_skip_to_index, on_remove_index):
        super().__init__()
        self.queue = queue
        self.on_skip_to_index = on_skip_to_index
        self.on_remove_index = on_remove_index

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        box.set_size_request(300, 320)
        box.set_margin_top(8)
        box.set_margin_bottom(8)
        box.set_margin_start(8)
        box.set_margin_end(8)

        header = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        title = Gtk.Label(label="Playback Queue")
        title.add_css_class("heading")
        title.set_hexpand(True)
        title.set_halign(Gtk.Align.START)
        header.append(title)

        btn_clear = Gtk.Button(label="Clear")
        btn_clear.add_css_class("flat")
        btn_clear.connect("clicked", self._on_clear)
        header.append(btn_clear)
        box.append(header)

        scrolled = Gtk.ScrolledWindow()
        scrolled.set_vexpand(True)
        self.list_box = Gtk.ListBox()
        self.list_box.set_selection_mode(Gtk.SelectionMode.NONE)
        self.list_box.add_css_class("boxed-list")
        scrolled.set_child(self.list_box)
        box.append(scrolled)

        self.set_child(box)
        self.connect("show", lambda p: self.refresh())

    def refresh(self):
        while True:
            row = self.list_box.get_row_at_index(0)
            if not row:
                break
            self.list_box.remove(row)

        if not self.queue.tracks:
            row = Adw.ActionRow(title="Queue is empty")
            self.list_box.append(row)
            return

        for idx, track in enumerate(self.queue.tracks):
            is_current = (idx == self.queue.current_index)
            prefix = "▶ " if is_current else f"{idx+1}. "
            row = Adw.ActionRow(title=f"{prefix}{track.title}", subtitle=track.artist)
            if is_current:
                row.add_css_class("accent")

            btn_del = Gtk.Button(icon_name="user-trash-symbolic")
            btn_del.add_css_class("flat")
            btn_del.connect("clicked", lambda b, i=idx: (self.on_remove_index(i), self.refresh()))
            row.add_suffix(btn_del)

            row.set_activatable(True)
            row.connect("activated", lambda r, i=idx: (self.on_skip_to_index(i), self.popdown()))
            self.list_box.append(row)

    def _on_clear(self, btn):
        self.queue.clear()
        self.refresh()


class VolumePopover(Gtk.Popover):
    def __init__(self, player):
        super().__init__()
        self.player = player

        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        box.set_margin_top(8)
        box.set_margin_bottom(8)
        box.set_margin_start(8)
        box.set_margin_end(8)

        self.btn_mute = Gtk.Button(icon_name="audio-volume-high-symbolic")
        self.btn_mute.add_css_class("flat")
        self.btn_mute.connect("clicked", self._on_toggle_mute)
        box.append(self.btn_mute)

        self.scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0.0, 1.0, 0.05)
        self.scale.set_size_request(120, -1)
        self.scale.set_value(player.volume)
        self.scale.connect("value-changed", self._on_volume_changed)
        box.append(self.scale)

        self.set_child(box)

    def _on_toggle_mute(self, btn):
        muted = self.player.toggle_mute()
        icon = "audio-volume-muted-symbolic" if muted else "audio-volume-high-symbolic"
        self.btn_mute.set_icon_name(icon)

    def _on_volume_changed(self, scale):
        val = scale.get_value()
        self.player.set_volume(val)
        if self.player.muted:
            self.btn_mute.set_icon_name("audio-volume-muted-symbolic")
        elif val == 0:
            self.btn_mute.set_icon_name("audio-volume-muted-symbolic")
        elif val < 0.33:
            self.btn_mute.set_icon_name("audio-volume-low-symbolic")
        elif val < 0.66:
            self.btn_mute.set_icon_name("audio-volume-medium-symbolic")
        else:
            self.btn_mute.set_icon_name("audio-volume-high-symbolic")


class AuthDialog(Adw.Window):
    def __init__(self, parent_window, source, on_auth_changed):
        super().__init__(transient_for=parent_window, modal=True)
        self.source = source
        self.on_auth_changed = on_auth_changed
        self.set_title("YouTube Music Login")
        self.set_default_size(440, 480)

        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        header = Adw.HeaderBar()
        header.set_show_title(True)
        content.append(header)

        scrolled = Gtk.ScrolledWindow()
        scrolled.set_vexpand(True)

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=16)
        box.set_margin_top(20)
        box.set_margin_bottom(20)
        box.set_margin_start(20)
        box.set_margin_end(20)

        # Step 1: Open in default browser
        group_open = Adw.PreferencesGroup(title="1. Log In via Your Browser")
        group_open.set_description("Open YouTube Music in your desktop browser to log into your Google account:")
        btn_open_browser = Gtk.Button(label="Open music.youtube.com in Browser")
        btn_open_browser.add_css_class("suggested-action")
        btn_open_browser.connect("clicked", self._on_open_browser)
        group_open.add(btn_open_browser)
        box.append(group_open)

        # Step 2: Import session
        self.group_import = Adw.PreferencesGroup(title="2. Import Browser Session")
        self.group_import.set_description("Import your logged-in YouTube Music session:")

        detected = self.source.get_detected_browsers()
        for label, spec in detected.items():
            btn = Gtk.Button(label=f"Import from {label}")
            btn.connect("clicked", lambda b, s=spec: self._import_browser(s))
            self.group_import.add(btn)

        box.append(self.group_import)

        # Step 3: Manual cookie file
        group_file = Adw.PreferencesGroup(title="3. Alternative / Manual Import")
        btn_file = Gtk.Button(label="Load Netscape cookies.txt...")
        btn_file.connect("clicked", self._open_file_dialog)
        group_file.add(btn_file)
        box.append(group_file)

        # Status Label
        self.lbl_status = Gtk.Label()
        self.lbl_status.set_wrap(True)
        box.append(self.lbl_status)

        scrolled.set_child(box)
        content.append(scrolled)
        self.set_content(content)

    def _on_open_browser(self, btn):
        import subprocess
        subprocess.Popen(["xdg-open", "https://music.youtube.com"])

    def _import_browser(self, browser_spec: str):
        self.lbl_status.set_text("Importing session...")
        def worker():
            ok = self.source.import_browser_cookies(browser_spec)
            GLib.idle_add(self._on_import_done, ok)
        threading.Thread(target=worker, daemon=True).start()

    def _on_import_done(self, success: bool):
        if success:
            self.lbl_status.set_text("Session imported successfully!")
            if self.on_auth_changed:
                self.on_auth_changed(True)
            GLib.timeout_add(1000, self.close)
        else:
            self.lbl_status.set_text("Import failed. Make sure you are logged into YouTube Music in your browser, or close the browser to unlock the cookie database.")

    def _open_file_dialog(self, btn):
        dialog = Gtk.FileDialog()
        dialog.open(self, None, self._on_file_selected)

    def _on_file_selected(self, dialog, result):
        try:
            file = dialog.open_finish(result)
            if file and self.source.load_cookie_file(file.get_path()):
                if self.on_auth_changed:
                    self.on_auth_changed(True)
                self.close()
        except Exception:
            pass
