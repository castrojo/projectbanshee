#!/usr/bin/env python3
"""
Prototype testing GStreamer audio playback from a YouTube Music stream resolved via yt-dlp.
"""
import sys
import subprocess
import time
import gi

gi.require_version('Gst', '1.0')
gi.require_version('GLib', '2.0')
from gi.repository import Gst, GLib

def resolve_stream(query):
    print(f"[Prototype] Resolving stream for '{query}'...")
    cmd = [
        "/home/linuxbrew/.linuxbrew/bin/yt-dlp",
        "--default-search", "ytsearch",
        "-f", "bestaudio/best",
        "-g",
        "--no-playlist",
        query
    ]
    res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=True)
    url = res.stdout.strip().splitlines()[-1]
    print(f"[Prototype] Got stream URL: {url[:80]}...")
    return url

def main():
    Gst.init(None)
    loop = GLib.MainLoop()

    query = sys.argv[1] if len(sys.argv) > 1 else "ytsearch1:daft punk get lucky official audio"
    try:
        stream_url = resolve_stream(query)
    except Exception as e:
        print(f"Error resolving: {e}")
        return 1

    player = Gst.ElementFactory.make("playbin3", "prototype-player")
    if not player:
        player = Gst.ElementFactory.make("playbin", "prototype-player")
    
    player.set_property("uri", stream_url)

    bus = player.get_bus()
    bus.add_signal_watch()

    start_time = time.time()

    def on_message(bus, msg):
        t = msg.type
        if t == Gst.MessageType.ERROR:
            err, dbg = msg.parse_error()
            print(f"[Gst ERROR] {err}: {dbg}")
            loop.quit()
        elif t == Gst.MessageType.EOS:
            print("[Gst EOS] Reached end of stream")
            loop.quit()
        elif t == Gst.MessageType.STATE_CHANGED:
            if msg.src == player:
                old, new, pending = msg.parse_state_changed()
                print(f"[Gst State] {old.value_name} -> {new.value_name}")
        return True

    bus.connect("message", on_message)

    print("[Prototype] Starting playback...")
    player.set_state(Gst.State.PLAYING)

    def check_progress():
        success, pos = player.query_position(Gst.Format.TIME)
        success_dur, dur = player.query_duration(Gst.Format.TIME)
        if success and pos > 0:
            p_sec = pos / Gst.SECOND
            d_sec = dur / Gst.SECOND if success_dur else 0
            print(f"[Playback] Position: {p_sec:.1f}s / {d_sec:.1f}s")
            if p_sec >= 2.0:
                print("[Prototype SUCCESS] Successfully streamed and decoded 2 seconds of audio!")
                player.set_state(Gst.State.NULL)
                loop.quit()
                return False
        return True

    GLib.timeout_add(300, check_progress)

    # Safety timeout
    GLib.timeout_add_seconds(15, lambda: (print("[Timeout] Exiting"), loop.quit(), False)[-1])

    loop.run()
    return 0

if __name__ == "__main__":
    sys.exit(main())
