//! Share to Discord (ADR 0009): clipboard message + open the Discord Flatpak via `discord://`.

use crate::model::Track;
use gtk::prelude::*;
use gtk::{gdk, gio};

/// Discord's message limit.
const DISCORD_LIMIT: usize = 2000;
pub const DISCORD_URI: &str = "discord://-/channels/@me";

/// One line per track: `title — artist <url>`. Truncated to Discord's 2000-char limit on a
/// line boundary, with a trailing count of omitted items.
pub fn share_message<'a>(tracks: impl IntoIterator<Item = &'a Track>) -> String {
    let lines: Vec<String> = tracks
        .into_iter()
        .map(|t| format!("{} — {} {}", t.title, t.artist, t.web_url()))
        .collect();
    let total = lines.len();
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        let remaining = total - i - 1;
        let tail = if remaining > 0 {
            format!("\n…and {remaining} more")
        } else {
            String::new()
        };
        let sep = if out.is_empty() { 0 } else { 1 };
        if out.len() + sep + line.len() + tail.len() > DISCORD_LIMIT {
            let omitted = total - i;
            out.push_str(&format!("\n…and {omitted} more"));
            return out;
        }
        if sep == 1 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// Put `message` on the clipboard, then ask the desktop to open Discord.
/// The clipboard is set before launching so a missing handler still leaves it usable.
pub async fn share(
    display: &gdk::Display,
    parent: Option<&gtk::Window>,
    message: &str,
) -> Result<(), String> {
    display.clipboard().set_text(message);
    gtk::UriLauncher::new(DISCORD_URI)
        .launch_future(parent)
        .await
        .map_err(|e| {
            if e.matches(gio::IOErrorEnum::NotSupported) || e.matches(gio::IOErrorEnum::NotFound) {
                "Discord is not installed; the links are on the clipboard".to_string()
            } else {
                format!("Could not open Discord ({e}); the links are on the clipboard")
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MediaKind, SourceKind};

    fn t(i: usize) -> Track {
        Track {
            id: format!("id{i:05}"),
            kind: MediaKind::Music,
            source: SourceKind::YouTubeMusic,
            title: format!("Song {i}"),
            artist: "Artist".into(),
            artist_id: None,
            album: None,
            duration_secs: None,
            thumbnail_url: None,
        }
    }

    #[test]
    fn message_lists_links_and_respects_discord_limit() {
        let one = share_message([&t(1)]);
        assert_eq!(
            one,
            "Song 1 — Artist https://music.youtube.com/watch?v=id00001"
        );
        let many: Vec<Track> = (0..200).map(t).collect();
        let msg = share_message(&many);
        assert!(msg.len() <= DISCORD_LIMIT, "{}", msg.len());
        assert!(msg.ends_with("more"));
        let shown = msg.lines().filter(|l| l.contains("https://")).count();
        let omitted: usize = msg.rsplit(' ').nth(1).unwrap().parse().unwrap();
        assert_eq!(shown + omitted, 200);
    }
}
