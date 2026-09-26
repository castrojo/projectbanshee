# Issue 05: Search, Queue, and Volume Popovers

Status: ready-for-agent
Blocked by: 04

## Context
In Mini Mode, library interactions (searching tracks, inspecting/reordering queue, adjusting volume) happen via compact popovers attached to the mini mode window toolbar, maintaining a zero-footprint main window.

## Acceptance Criteria
1. `SearchPopover`: Gtk.SearchEntry with debounced search query triggering `source.search()`. Results displayed in an `Adw.PreferencesGroup` or `Gtk.ListView` with "+ Queue" and "Play Now" action rows.
2. `QueuePopover`: Displays active queue, highlights currently playing track, allows removing tracks and clearing queue.
3. `VolumePopover`: Compact vertical/horizontal volume slider with mute/unmute button.
4. `AuthDialog`: Dialog with WebKitGTK login option (marked experimental) plus one-click "Import from Browser" (Firefox, Chrome, Brave) and "Choose cookies.txt file" buttons.
