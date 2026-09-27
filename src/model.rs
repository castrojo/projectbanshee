//! Core domain types shared by every layer (see CONTEXT.md).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Which kind of media a Track is. The Queue mixes all kinds freely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MediaKind {
    Music,
    Video,
    Episode,
}

impl MediaKind {
    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Music => "Song",
            MediaKind::Video => "Video",
            MediaKind::Episode => "Episode",
        }
    }
    pub fn icon_name(self) -> &'static str {
        match self {
            MediaKind::Music => "audio-x-generic-symbolic",
            MediaKind::Video => "video-x-generic-symbolic",
            MediaKind::Episode => "audio-input-microphone-symbolic",
        }
    }
}

/// The Audio Source a Track came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum SourceKind {
    YouTubeMusic,
    Spotify,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::YouTubeMusic => "YouTube",
            SourceKind::Spotify => "Spotify",
        }
    }
    pub fn slug(self) -> &'static str {
        match self {
            SourceKind::YouTubeMusic => "youtube",
            SourceKind::Spotify => "spotify",
        }
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Normalized metadata for one playable item. Stream URLs are resolved at play time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Track {
    /// Source-native id (YouTube video id, Spotify base62 id).
    pub id: String,
    pub kind: MediaKind,
    pub source: SourceKind,
    pub title: String,
    pub artist: String,
    /// YouTube Music channel id (`UC…`) or Spotify artist id, if known.
    pub artist_id: Option<String>,
    pub album: Option<String>,
    /// Duration in whole seconds, if known.
    pub duration_secs: Option<u32>,
    pub thumbnail_url: Option<String>,
}

impl Track {
    /// Globally unique key across sources.
    pub fn key(&self) -> String {
        format!("{}:{}", self.source.slug(), self.id)
    }

    /// Canonical web link used for sharing and "open in browser".
    pub fn web_url(&self) -> String {
        match (self.source, self.kind) {
            (SourceKind::YouTubeMusic, MediaKind::Video) => {
                format!("https://www.youtube.com/watch?v={}", self.id)
            }
            (SourceKind::YouTubeMusic, _) => {
                format!("https://music.youtube.com/watch?v={}", self.id)
            }
            (SourceKind::Spotify, MediaKind::Episode) => {
                format!("https://open.spotify.com/episode/{}", self.id)
            }
            (SourceKind::Spotify, _) => format!("https://open.spotify.com/track/{}", self.id),
        }
    }

    /// The artist's page on YouTube Music (for artwork clicks). Spotify tracks fall back to
    /// a YouTube Music search for the artist name.
    pub fn artist_page_url(&self) -> String {
        match (&self.artist_id, self.source) {
            (Some(id), SourceKind::YouTubeMusic) if id.starts_with("UC") => {
                format!("https://music.youtube.com/channel/{id}")
            }
            _ => {
                let q: String =
                    url::form_urlencoded::byte_serialize(self.artist.as_bytes()).collect();
                format!("https://music.youtube.com/search?q={q}")
            }
        }
    }

    pub fn duration_label(&self) -> String {
        match self.duration_secs {
            Some(s) => format_duration(u64::from(s)),
            None => String::new(),
        }
    }

    /// Text used by the fuzzy matcher.
    pub fn haystack(&self) -> String {
        match &self.album {
            Some(a) if !a.is_empty() => format!("{} {} {}", self.title, self.artist, a),
            _ => format!("{} {}", self.title, self.artist),
        }
    }
}

/// `m:ss` or `h:mm:ss`.
pub fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Human total length for lists: `3 h 12 min`, `47 min`.
pub fn format_total(secs: u64) -> String {
    let (h, m) = (secs / 3600, (secs / 60) % 60);
    match (h, m) {
        (0, 0) => "under a minute".into(),
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// Parse `m:ss` / `h:mm:ss` as returned by YouTube Music.
pub fn parse_duration(text: &str) -> Option<u32> {
    let mut total: u32 = 0;
    let mut parts = 0;
    for p in text.trim().split(':') {
        let v: u32 = p.trim().parse().ok()?;
        total = total.checked_mul(60)?.checked_add(v)?;
        parts += 1;
    }
    (1..=3).contains(&parts).then_some(total)
}

/// What kind of Collection a library item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CollectionKind {
    Playlist,
    Album,
    Artist,
    Podcast,
    LikedSongs,
}

impl CollectionKind {
    pub fn icon_name(self) -> &'static str {
        match self {
            CollectionKind::Playlist | CollectionKind::LikedSongs => "view-list-symbolic",
            CollectionKind::Album => "media-optical-symbolic",
            CollectionKind::Artist => "avatar-default-symbolic",
            CollectionKind::Podcast => "audio-input-microphone-symbolic",
        }
    }
}

/// A browsable, queueable group (playlist, album, artist top songs, podcast).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Collection {
    pub id: String,
    pub source: SourceKind,
    pub kind: CollectionKind,
    pub title: String,
    pub subtitle: String,
    pub thumbnail_url: Option<String>,
}

impl Collection {
    pub fn key(&self) -> String {
        format!("{}:{:?}:{}", self.source.slug(), self.kind, self.id)
    }
}

/// One Library surface (e.g. "Playlists") for one Audio Source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySection {
    pub title: String,
    pub collections: Vec<Collection>,
}

/// Something the search page can show: a Track to queue or a Collection to open/queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchItem {
    Track(Track),
    Collection(Collection),
}

/// Search filter chosen in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SearchFilter {
    #[default]
    All,
    Music,
    Videos,
    Podcasts,
}

/// What Player Core needs to start an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Playable {
    /// A URI playbin3 can open directly. `video` selects the video-capable pipeline;
    /// `headers` are HTTP request headers the stream host expects (e.g. User-Agent).
    Uri {
        uri: String,
        video: bool,
        headers: Vec<(String, String)>,
    },
    /// A Spotify URI (`spotify:track:…` / `spotify:episode:…`) for the librespot pipeline.
    Spotify { uri: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_round_trip_known_values() {
        assert_eq!(parse_duration("3:45"), Some(225));
        assert_eq!(parse_duration("1:02:03"), Some(3723));
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(format_duration(3723), "1:02:03");
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_total(71_895), "19 h 58 min");
        assert_eq!(format_total(2_820), "47 min");
    }
}
