# 16: Videos play at 360p

**What to build:** YouTube no longer serves muxed formats above 360p without a PO token; HLS variants fail in playbin3 (hlsdemux2 cannot switch between VP9 and H.264 variants). Options: dual-URI playback (separate audio/video) via a custom pipeline, or PO-token support in yt-dlp.

**Blocked by:** None (can start immediately)

**Status:** needs-info

- [ ] Decide whether higher-resolution video is worth a custom two-stream pipeline
