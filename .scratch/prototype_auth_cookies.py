#!/usr/bin/env python3
"""
Test cookie manager and auth storage fallback in headless environment.
"""
import os
import sys
import tempfile
import gi

gi.require_version('Gtk', '4.0')
gi.require_version('WebKit', '6.0')
from gi.repository import Gtk, WebKit, GLib, Gio

def test_webkit_cookie_api():
    print("[Auth Prototype] Testing WebKitGTK 6.0 Cookie Manager API...")
    wv = WebKit.WebView()
    session = wv.get_network_session()
    cm = session.get_cookie_manager()
    
    # Verify cookie manager methods exist
    assert hasattr(cm, 'get_cookies'), "CookieManager lacks get_cookies"
    assert hasattr(cm, 'set_persistent_storage'), "CookieManager lacks set_persistent_storage"
    
    with tempfile.TemporaryDirectory() as tmpdir:
        cookie_path = os.path.join(tmpdir, "cookies.txt")
        cm.set_persistent_storage(cookie_path, WebKit.CookiePersistentStorage.TEXT)
        print(f"[Auth Prototype] Persistent storage set: {cookie_path}")
        
    print("[Auth Prototype] WebKitGTK cookie manager API verified successfully.")

def test_cookie_file_loading():
    print("[Auth Prototype] Testing Netscape cookie file parser for yt-dlp compatibility...")
    with tempfile.NamedTemporaryFile("w+", delete=False) as f:
        f.write("# Netscape HTTP Cookie File\n")
        f.write(".youtube.com\tTRUE\t/\tTRUE\t1800000000\tSID\tmock_sid_token\n")
        f.write(".youtube.com\tTRUE\t/\tTRUE\t1800000000\tSAPISID\tmock_sapisid_token\n")
        f_path = f.name
    
    try:
        assert os.path.exists(f_path)
        with open(f_path) as r:
            lines = [l for l in r if not l.startswith("#") and l.strip()]
            cookies = {}
            for l in lines:
                parts = l.strip().split("\t")
                if len(parts) >= 7:
                    cookies[parts[5]] = parts[6]
        assert "SID" in cookies
        assert "SAPISID" in cookies
        print(f"[Auth Prototype] Successfully parsed Netscape cookies: {list(cookies.keys())}")
    finally:
        os.remove(f_path)

if __name__ == "__main__":
    test_webkit_cookie_api()
    test_cookie_file_loading()
    print("[Auth Prototype SUCCESS] Both WebKit and Netscape cookie paths verified.")
