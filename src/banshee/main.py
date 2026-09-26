"""
Application entry point and GApplication lifecycle.
"""
import sys
import gi

gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
from gi.repository import Gtk, Adw, Gio, GLib

from banshee import __app_id__
from banshee.models import Queue
from banshee.player import Player
from banshee.sources.ytm import YouTubeMusicSource
from banshee.ui.window import MiniModeWindow
from banshee.mpris import MPRISService

class BansheeApplication(Adw.Application):
    def __init__(self):
        super().__init__(
            application_id=__app_id__,
            flags=Gio.ApplicationFlags.FLAGS_NONE
        )
        self.player = None
        self.queue = None
        self.source = None
        self.mpris = None

    def do_startup(self):
        Adw.Application.do_startup(self)
        self.queue = Queue()
        self.player = Player()
        self.source = YouTubeMusicSource()
        self.mpris = MPRISService(self, self.player, self.queue)

    def do_activate(self):
        win = self.get_active_window()
        if not win:
            win = MiniModeWindow(self, self.player, self.queue, self.source)
        win.present()

def main():
    app = BansheeApplication()
    return app.run(sys.argv)

if __name__ == "__main__":
    sys.exit(main())
