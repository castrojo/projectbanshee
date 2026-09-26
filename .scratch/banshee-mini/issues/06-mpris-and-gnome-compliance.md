# Issue 06: MPRIS v2 D-Bus Interface & GNOME 4 Compliance

Status: ready-for-agent
Blocked by: 02, 04

## Context
Full GNOME 4 integration requires exporting `org.mpris.MediaPlayer2` and `org.mpris.MediaPlayer2.Player` on the session bus, desktop entry, AppStream metainfo, and icons.

## Acceptance Criteria
1. Export `org.mpris.MediaPlayer2` and `org.mpris.MediaPlayer2.Player` on `org.mpris.MediaPlayer2.banshee`.
2. Support MPRIS properties: `PlaybackStatus`, `Metadata` (`mpris:trackId`, `xesam:title`, `xesam:artist`, `xesam:album`, `mpris:length`, `mpris:artUrl`), `Volume`, `Position`, `CanPlay`, `CanPause`, `CanGoNext`, `CanGoPrevious`, `CanSeek`.
3. Support MPRIS methods: `Play`, `Pause`, `PlayPause`, `Next`, `Previous`, `Stop`, `Seek`, `SetPosition`, `OpenUri`.
4. Install valid `org.projectbluefin.Banshee.desktop` file verified with `desktop-file-validate`.
5. Install valid `org.projectbluefin.Banshee.metainfo.xml` verified with `appstreamcli validate`.
6. Install SVG application icon in hicolor theme directory.
