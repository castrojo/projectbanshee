//! Spotify Web API response shapes and their mapping onto Banshee's model.
//!
//! Shapes follow the February 2026 Web API (playlist `tracks` → `items`, playlist entry
//! `track` → `item`) and still accept the older field names so either response parses.

use crate::model::{Collection, CollectionKind, MediaKind, SearchItem, SourceKind, Track};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// Smallest artwork edge the UI renders; smaller images look blurry.
const MIN_THUMBNAIL_PX: u32 = 120;

/// Collection id used for the signed-in user's saved tracks.
pub(crate) const LIKED_SONGS_ID: &str = "liked";

fn nullable_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<Vec<T>>::deserialize(d).map(Option::unwrap_or_default)
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Image {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Artist {
    pub id: Option<String>,
    pub name: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub images: Vec<Image>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Album {
    pub id: Option<String>,
    pub name: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub images: Vec<Image>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub artists: Vec<Artist>,
    pub release_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ApiTrack {
    pub id: Option<String>,
    pub name: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub artists: Vec<Artist>,
    pub album: Option<Album>,
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub is_local: bool,
    pub is_playable: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Show {
    pub id: Option<String>,
    pub name: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub images: Vec<Image>,
    pub publisher: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Episode {
    pub id: Option<String>,
    pub name: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub images: Vec<Image>,
    pub duration_ms: Option<u64>,
    pub show: Option<Show>,
    pub is_playable: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Owner {
    pub display_name: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Playlist {
    pub id: Option<String>,
    pub name: String,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub images: Vec<Image>,
    pub owner: Option<Owner>,
    /// `{ href, total }` (2026 name).
    pub items: Option<Value>,
    /// `{ href, total }` (pre-2026 name).
    pub tracks: Option<Value>,
}

/// One page of a paging object. Items stay raw so a single malformed or `null` entry
/// (Spotify returns `null` for removed playlists/episodes) is skipped instead of failing
/// the whole page.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Page {
    #[serde(default, deserialize_with = "nullable_vec")]
    pub items: Vec<Value>,
    pub next: Option<String>,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct SearchResponse {
    pub tracks: Option<Page>,
    pub episodes: Option<Page>,
    pub artists: Option<Page>,
    pub albums: Option<Page>,
    pub playlists: Option<Page>,
    pub shows: Option<Page>,
}

/// `GET /v1/me`. `product` is absent for apps affected by the 2026 field removals.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Me {
    pub id: String,
    pub display_name: Option<String>,
    pub product: Option<String>,
}

impl Me {
    pub fn name(&self) -> String {
        match self.display_name.as_deref().map(str::trim) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => self.id.clone(),
        }
    }
}

/// `GET /v1/artists/{id}/top-tracks`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TopTracks {
    #[serde(default, deserialize_with = "nullable_vec")]
    pub tracks: Vec<Value>,
}

/// Album/show metadata for simplified tracks/episodes that omit it (album tracks, show
/// episodes).
#[derive(Debug, Clone, Default)]
pub(crate) struct Parent {
    pub name: String,
    pub thumbnail_url: Option<String>,
}

impl Parent {
    pub fn of(collection: &Collection) -> Self {
        Self {
            name: collection.title.clone(),
            thumbnail_url: collection.thumbnail_url.clone(),
        }
    }
}

/// Deserialize every non-null entry, skipping (and logging) entries that do not parse.
pub(crate) fn parse_each<T: DeserializeOwned>(items: Vec<Value>) -> Vec<T> {
    items
        .into_iter()
        .filter(|v| !v.is_null())
        .filter_map(|v| match serde_json::from_value(v) {
            Ok(t) => Some(t),
            Err(e) => {
                log::debug!("skipping unparseable Spotify item: {e}");
                None
            }
        })
        .collect()
}

/// Smallest image whose edge is at least 120 px; otherwise the largest known one.
pub(crate) fn pick_image(images: &[Image]) -> Option<String> {
    let edge = |i: &Image| i.width.or(i.height);
    images
        .iter()
        .filter(|i| edge(i).is_some_and(|e| e >= MIN_THUMBNAIL_PX))
        .min_by_key(|i| edge(i))
        .or_else(|| {
            images
                .iter()
                .filter(|i| edge(i).is_some())
                .max_by_key(|i| edge(i))
        })
        .or_else(|| images.first())
        .map(|i| i.url.clone())
}

fn duration_secs(ms: Option<u64>) -> Option<u32> {
    ms.map(|ms| u32::try_from(ms.saturating_add(500) / 1000).unwrap_or(u32::MAX))
}

fn join_artists(artists: &[Artist]) -> String {
    artists
        .iter()
        .map(|a| a.name.trim())
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn non_empty(id: Option<String>) -> Option<String> {
    id.filter(|s| !s.is_empty())
}

/// A playable catalog track. `None` for local files, relinked-away or unplayable tracks.
pub(crate) fn track(t: ApiTrack, parent: Option<&Parent>) -> Option<Track> {
    if t.is_local || t.is_playable == Some(false) {
        return None;
    }
    let id = non_empty(t.id).filter(|id| is_spotify_id(id))?;
    let (album, thumbnail_url) = match (t.album, parent) {
        (Some(a), _) => (Some(a.name), pick_image(&a.images)),
        (None, Some(p)) => (Some(p.name.clone()), p.thumbnail_url.clone()),
        (None, None) => (None, None),
    };
    Some(Track {
        id,
        kind: MediaKind::Music,
        source: SourceKind::Spotify,
        title: t.name,
        artist: join_artists(&t.artists),
        artist_id: t.artists.iter().find_map(|a| non_empty(a.id.clone())),
        album: album.filter(|a| !a.is_empty()),
        duration_secs: duration_secs(t.duration_ms),
        thumbnail_url,
    })
}

/// A podcast episode; `artist` is the show name (from the episode or from `parent`).
pub(crate) fn episode(e: Episode, parent: Option<&Parent>) -> Option<Track> {
    if e.is_playable == Some(false) {
        return None;
    }
    let id = non_empty(e.id).filter(|id| is_spotify_id(id))?;
    let show_name = e
        .show
        .as_ref()
        .map(|s| s.name.clone())
        .or_else(|| parent.map(|p| p.name.clone()));
    let thumbnail_url = pick_image(&e.images)
        .or_else(|| e.show.as_ref().and_then(|s| pick_image(&s.images)))
        .or_else(|| parent.and_then(|p| p.thumbnail_url.clone()));
    Some(Track {
        id,
        kind: MediaKind::Episode,
        source: SourceKind::Spotify,
        title: e.name,
        artist: show_name.unwrap_or_default(),
        artist_id: None,
        album: None,
        duration_secs: duration_secs(e.duration_ms),
        thumbnail_url,
    })
}

/// A track or episode object, told apart by its `type` field.
pub(crate) fn playable(v: Value, parent: Option<&Parent>) -> Option<Track> {
    let is_episode = v.get("type").and_then(Value::as_str) == Some("episode");
    let parsed = if is_episode {
        serde_json::from_value::<Episode>(v).map(|e| episode(e, parent))
    } else {
        serde_json::from_value::<ApiTrack>(v).map(|t| track(t, parent))
    };
    parsed.unwrap_or_else(|e| {
        log::debug!("skipping unparseable Spotify track: {e}");
        None
    })
}

/// Entries of saved-track lists and playlists: `{ added_at, item | track: {…} }`.
pub(crate) fn saved_tracks(items: Vec<Value>) -> Vec<Track> {
    items
        .into_iter()
        .filter_map(|mut entry| {
            let inner = ["item", "track"]
                .iter()
                .filter_map(|k| entry.get_mut(*k).map(Value::take))
                .find(|v| v.is_object())?;
            playable(inner, None)
        })
        .collect()
}

/// Unwrap `{ added_at, <key>: {…} }` saved-item entries (saved albums/shows).
pub(crate) fn unwrap_saved(items: Vec<Value>, key: &str) -> Vec<Value> {
    items
        .into_iter()
        .filter_map(|mut v| v.get_mut(key).map(Value::take))
        .collect()
}

fn count_label(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

pub(crate) fn playlist_collection(p: Playlist) -> Option<Collection> {
    let id = non_empty(p.id).filter(|id| is_spotify_id(id))?;
    let total = p
        .items
        .as_ref()
        .or(p.tracks.as_ref())
        .and_then(|v| v.get("total"))
        .and_then(Value::as_u64);
    let owner = p
        .owner
        .and_then(|o| non_empty(o.display_name).or(non_empty(o.id)));
    let subtitle = match (owner, total) {
        (Some(o), Some(n)) => format!("{o} · {}", count_label(n, "item", "items")),
        (Some(o), None) => o,
        (None, Some(n)) => count_label(n, "item", "items"),
        (None, None) => "Playlist".to_string(),
    };
    Some(Collection {
        id,
        source: SourceKind::Spotify,
        kind: CollectionKind::Playlist,
        title: p.name,
        subtitle,
        thumbnail_url: pick_image(&p.images),
    })
}

pub(crate) fn album_collection(a: Album) -> Option<Collection> {
    let id = non_empty(a.id).filter(|id| is_spotify_id(id))?;
    let artists = join_artists(&a.artists);
    let year = a
        .release_date
        .as_deref()
        .and_then(|d| d.get(..4))
        .filter(|y| y.bytes().all(|b| b.is_ascii_digit()));
    let subtitle = match year {
        Some(y) if !artists.is_empty() => format!("{artists} · {y}"),
        Some(y) => y.to_string(),
        None if !artists.is_empty() => artists,
        None => "Album".to_string(),
    };
    Some(Collection {
        id,
        source: SourceKind::Spotify,
        kind: CollectionKind::Album,
        title: a.name,
        subtitle,
        thumbnail_url: pick_image(&a.images),
    })
}

pub(crate) fn artist_collection(a: Artist) -> Option<Collection> {
    let id = non_empty(a.id).filter(|id| is_spotify_id(id))?;
    Some(Collection {
        id,
        source: SourceKind::Spotify,
        kind: CollectionKind::Artist,
        title: a.name,
        subtitle: "Artist".to_string(),
        thumbnail_url: pick_image(&a.images),
    })
}

pub(crate) fn show_collection(s: Show) -> Option<Collection> {
    let id = non_empty(s.id).filter(|id| is_spotify_id(id))?;
    Some(Collection {
        id,
        source: SourceKind::Spotify,
        kind: CollectionKind::Podcast,
        title: s.name,
        subtitle: non_empty(s.publisher).unwrap_or_else(|| "Podcast".to_string()),
        thumbnail_url: pick_image(&s.images),
    })
}

pub(crate) fn liked_songs_collection(total: Option<u64>) -> Collection {
    Collection {
        id: LIKED_SONGS_ID.to_string(),
        source: SourceKind::Spotify,
        kind: CollectionKind::LikedSongs,
        title: "Liked Songs".to_string(),
        subtitle: total.map_or_else(
            || "Your saved songs".to_string(),
            |n| count_label(n, "song", "songs"),
        ),
        thumbnail_url: None,
    }
}

fn page_items(page: Option<Page>) -> Vec<Value> {
    page.map(|p| p.items).unwrap_or_default()
}

/// Flatten a search response: tracks and episodes first, then artists, albums, playlists
/// and shows, each in Spotify's relevance order.
pub(crate) fn search_items(r: SearchResponse) -> Vec<SearchItem> {
    let mut out = Vec::new();
    out.extend(
        parse_each::<ApiTrack>(page_items(r.tracks))
            .into_iter()
            .filter_map(|t| track(t, None))
            .map(SearchItem::Track),
    );
    out.extend(
        parse_each::<Episode>(page_items(r.episodes))
            .into_iter()
            .filter_map(|e| episode(e, None))
            .map(SearchItem::Track),
    );
    out.extend(
        parse_each::<Artist>(page_items(r.artists))
            .into_iter()
            .filter_map(artist_collection)
            .map(SearchItem::Collection),
    );
    out.extend(
        parse_each::<Album>(page_items(r.albums))
            .into_iter()
            .filter_map(album_collection)
            .map(SearchItem::Collection),
    );
    out.extend(
        parse_each::<Playlist>(page_items(r.playlists))
            .into_iter()
            .filter_map(playlist_collection)
            .map(SearchItem::Collection),
    );
    out.extend(
        parse_each::<Show>(page_items(r.shows))
            .into_iter()
            .filter_map(show_collection)
            .map(SearchItem::Collection),
    );
    out
}

/// Spotify ids are 22 base62 characters.
pub(crate) fn is_spotify_id(id: &str) -> bool {
    id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Human-readable message from a Web API error body
/// (`{"error":{"status":403,"message":"…"}}` or OAuth `{"error":"…","error_description":"…"}`).
pub(crate) fn error_message(body: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let err = v.get("error")?;
    let msg = match err {
        Value::String(code) => v
            .get("error_description")
            .and_then(Value::as_str)
            .unwrap_or(code),
        other => other.get("message").and_then(Value::as_str)?,
    };
    let msg = msg.trim();
    (!msg.is_empty()).then(|| msg.to_string())
}

/// `(kind, id)` for `spotify:track:<id>` / `spotify:episode:<id>` URIs and
/// `https://open.spotify.com/[intl-xx/][embed/]track|episode/<id>` links.
pub fn track_from_url(url: &str) -> Option<(MediaKind, String)> {
    let url = url.trim();
    let kind_of = |segment: &str| match segment {
        "track" => Some(MediaKind::Music),
        "episode" => Some(MediaKind::Episode),
        _ => None,
    };
    if let Some(rest) = url.strip_prefix("spotify:") {
        let mut parts = rest.split(':');
        let kind = kind_of(parts.next()?)?;
        let id = parts.next()?;
        return (parts.next().is_none() && is_spotify_id(id)).then(|| (kind, id.to_string()));
    }
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "https" | "http") || parsed.host_str() != Some("open.spotify.com")
    {
        return None;
    }
    let mut segments = parsed.path_segments()?.filter(|s| !s.is_empty()).peekable();
    if segments.peek().is_some_and(|s| s.starts_with("intl-")) {
        segments.next();
    }
    if segments.peek() == Some(&"embed") {
        segments.next();
    }
    let kind = kind_of(segments.next()?)?;
    let id = segments.next()?;
    (segments.next().is_none() && is_spotify_id(id)).then(|| (kind, id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn img(url: &str, w: u32) -> Value {
        json!({ "url": url, "width": w, "height": w })
    }

    fn track_json() -> Value {
        json!({
            "type": "track",
            "id": "4uLU6hMCjMI75M1A2tKUQC",
            "name": "Never Gonna Give You Up",
            "duration_ms": 213_573,
            "is_local": false,
            "is_playable": true,
            "artists": [
                { "id": "0gxyHStUsqpMadRV0Di1Qt", "name": "Rick Astley", "type": "artist" },
                { "id": "1234567890123456789012", "name": "Guest", "type": "artist" }
            ],
            "album": {
                "id": "6N9PS4QXF1D0OWPk0Sxtb4",
                "name": "Whenever You Need Somebody",
                "album_type": "album",
                "release_date": "1987-11-12",
                "artists": [{ "id": "0gxyHStUsqpMadRV0Di1Qt", "name": "Rick Astley" }],
                "images": [img("https://i.scdn.co/640", 640), img("https://i.scdn.co/300", 300), img("https://i.scdn.co/64", 64)]
            }
        })
    }

    #[test]
    fn track_maps_every_field() {
        let t = playable(track_json(), None).expect("track maps");
        assert_eq!(t.id, "4uLU6hMCjMI75M1A2tKUQC");
        assert_eq!(t.kind, MediaKind::Music);
        assert_eq!(t.source, SourceKind::Spotify);
        assert_eq!(t.artist, "Rick Astley, Guest");
        assert_eq!(t.artist_id.as_deref(), Some("0gxyHStUsqpMadRV0Di1Qt"));
        assert_eq!(t.album.as_deref(), Some("Whenever You Need Somebody"));
        assert_eq!(t.duration_secs, Some(214));
        assert_eq!(t.thumbnail_url.as_deref(), Some("https://i.scdn.co/300"));
    }

    #[test]
    fn local_and_unplayable_tracks_are_skipped() {
        let mut local = track_json();
        local["is_local"] = json!(true);
        assert!(playable(local, None).is_none());
        let mut blocked = track_json();
        blocked["is_playable"] = json!(false);
        assert!(playable(blocked, None).is_none());
        let mut no_id = track_json();
        no_id["id"] = Value::Null;
        assert!(playable(no_id, None).is_none());
    }

    #[test]
    fn simplified_album_track_uses_parent_album() {
        let v = json!({
            "id": "4uLU6hMCjMI75M1A2tKUQC", "name": "Song", "duration_ms": 1000,
            "artists": [{ "id": "0gxyHStUsqpMadRV0Di1Qt", "name": "A" }], "type": "track"
        });
        let parent = Parent {
            name: "The Album".into(),
            thumbnail_url: Some("https://t".into()),
        };
        let t = playable(v, Some(&parent)).expect("maps");
        assert_eq!(t.album.as_deref(), Some("The Album"));
        assert_eq!(t.thumbnail_url.as_deref(), Some("https://t"));
    }

    #[test]
    fn episode_uses_show_name_as_artist() {
        let v = json!({
            "type": "episode", "id": "512ojhOuo1ktJprKbVcKyQ", "name": "Ep 1",
            "duration_ms": 3_600_000, "images": [img("https://e/64", 64)],
            "show": { "id": "38bS44xjbVVZ3No3ByF1dJ", "name": "The Show", "images": [img("https://s/300", 300)] }
        });
        let t = playable(v, None).expect("episode maps");
        assert_eq!(t.kind, MediaKind::Episode);
        assert_eq!(t.artist, "The Show");
        assert_eq!(t.duration_secs, Some(3600));
        // Only a 64 px episode image: the largest known one wins over nothing.
        assert_eq!(t.thumbnail_url.as_deref(), Some("https://e/64"));
        assert_eq!(
            t.web_url(),
            "https://open.spotify.com/episode/512ojhOuo1ktJprKbVcKyQ"
        );
    }

    #[test]
    fn show_episodes_fall_back_to_parent_show() {
        let items = json!([
            { "type": "episode", "id": "512ojhOuo1ktJprKbVcKyQ", "name": "Ep", "duration_ms": 60_000, "images": null },
            null
        ]);
        let parent = Parent {
            name: "Pod".into(),
            thumbnail_url: Some("https://p".into()),
        };
        let eps: Vec<Track> = parse_each::<Episode>(serde_json::from_value(items).expect("array"))
            .into_iter()
            .filter_map(|e| episode(e, Some(&parent)))
            .collect();
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].artist, "Pod");
        assert_eq!(eps[0].thumbnail_url.as_deref(), Some("https://p"));
    }

    #[test]
    fn playlist_entries_accept_item_and_legacy_track_keys() {
        let items = vec![
            json!({ "added_at": "2026-01-01T00:00:00Z", "item": track_json() }),
            json!({ "added_at": "2020-01-01T00:00:00Z", "track": track_json() }),
            json!({ "added_at": "2020-01-01T00:00:00Z", "item": null, "track": track_json() }),
            json!({ "added_at": "2020-01-01T00:00:00Z", "track": null }),
        ];
        assert_eq!(saved_tracks(items).len(), 3);
    }

    #[test]
    fn playlist_collection_counts_new_and_old_shapes() {
        let new: Playlist = serde_json::from_value(json!({
            "id": "37i9dQZF1DXcBWIGoYBM5M", "name": "Today's Top Hits", "images": null,
            "owner": { "id": "spotify", "display_name": "Spotify" }, "items": { "href": "x", "total": 50 }
        }))
        .expect("parses");
        let c = playlist_collection(new).expect("maps");
        assert_eq!(c.kind, CollectionKind::Playlist);
        assert_eq!(c.subtitle, "Spotify · 50 items");
        assert_eq!(c.thumbnail_url, None);
        let old: Playlist = serde_json::from_value(json!({
            "id": "37i9dQZF1DXcBWIGoYBM5M", "name": "Mine", "images": [{ "url": "https://m", "width": null, "height": null }],
            "owner": { "id": "jorge", "display_name": null }, "tracks": { "href": "x", "total": 1 }
        }))
        .expect("parses");
        let c = playlist_collection(old).expect("maps");
        assert_eq!(c.subtitle, "jorge · 1 item");
        assert_eq!(c.thumbnail_url.as_deref(), Some("https://m"));
    }

    #[test]
    fn saved_albums_and_shows_map_to_collections() {
        let albums = unwrap_saved(
            vec![json!({ "added_at": "x", "album": track_json()["album"].clone() })],
            "album",
        );
        let c = album_collection(parse_each::<Album>(albums).remove(0)).expect("album");
        assert_eq!(c.subtitle, "Rick Astley · 1987");
        assert_eq!(c.kind, CollectionKind::Album);
        let shows = unwrap_saved(
            vec![
                json!({ "added_at": "x", "show": { "id": "38bS44xjbVVZ3No3ByF1dJ", "name": "Pod", "images": [] } }),
            ],
            "show",
        );
        let s = show_collection(parse_each::<Show>(shows).remove(0)).expect("show");
        assert_eq!(
            (s.kind, s.subtitle.as_str()),
            (CollectionKind::Podcast, "Podcast")
        );
    }

    #[test]
    fn search_response_orders_tracks_before_collections_and_skips_nulls() {
        let r: SearchResponse = serde_json::from_value(json!({
            "tracks": { "items": [track_json()], "next": null, "total": 1 },
            "artists": { "items": [{ "id": "0gxyHStUsqpMadRV0Di1Qt", "name": "Rick Astley", "images": [img("https://a/160", 160)] }] },
            "playlists": { "items": [null, { "id": "37i9dQZF1DXcBWIGoYBM5M", "name": "P", "images": [], "owner": null, "items": null }] },
            "episodes": { "items": [null] }
        }))
        .expect("parses");
        let items = search_items(r);
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[0], SearchItem::Track(t) if t.id == "4uLU6hMCjMI75M1A2tKUQC"));
        assert!(
            matches!(&items[1], SearchItem::Collection(c) if c.kind == CollectionKind::Artist
            && c.thumbnail_url.as_deref() == Some("https://a/160"))
        );
        assert!(
            matches!(&items[2], SearchItem::Collection(c) if c.kind == CollectionKind::Playlist && c.subtitle == "Playlist")
        );
    }

    #[test]
    fn image_choice_prefers_smallest_adequate() {
        let images: Vec<Image> =
            serde_json::from_value(json!([img("a", 640), img("b", 160), img("c", 64)]))
                .expect("parses");
        assert_eq!(pick_image(&images).as_deref(), Some("b"));
        let small: Vec<Image> =
            serde_json::from_value(json!([img("c", 64), img("d", 100)])).expect("parses");
        assert_eq!(pick_image(&small).as_deref(), Some("d"));
        assert_eq!(pick_image(&[]), None);
    }

    #[test]
    fn error_bodies_yield_messages() {
        assert_eq!(
            error_message(br#"{"error":{"status":403,"message":"Premium required"}}"#).as_deref(),
            Some("Premium required")
        );
        assert_eq!(
            error_message(
                br#"{"error":"invalid_grant","error_description":"Refresh token revoked"}"#
            )
            .as_deref(),
            Some("Refresh token revoked")
        );
        assert_eq!(error_message(b"<html>"), None);
    }

    #[test]
    fn urls_and_uris_parse() {
        let id = "4uLU6hMCjMI75M1A2tKUQC".to_string();
        assert_eq!(
            track_from_url("spotify:track:4uLU6hMCjMI75M1A2tKUQC"),
            Some((MediaKind::Music, id.clone()))
        );
        assert_eq!(
            track_from_url("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=abc"),
            Some((MediaKind::Music, id.clone()))
        );
        assert_eq!(
            track_from_url("https://open.spotify.com/intl-de/track/4uLU6hMCjMI75M1A2tKUQC"),
            Some((MediaKind::Music, id.clone()))
        );
        assert_eq!(
            track_from_url(" https://open.spotify.com/embed/episode/4uLU6hMCjMI75M1A2tKUQC "),
            Some((MediaKind::Episode, id.clone()))
        );
        assert_eq!(
            track_from_url("spotify:episode:4uLU6hMCjMI75M1A2tKUQC"),
            Some((MediaKind::Episode, id))
        );
        assert_eq!(
            track_from_url("https://open.spotify.com/album/4uLU6hMCjMI75M1A2tKUQC"),
            None
        );
        assert_eq!(
            track_from_url("https://evil.example/track/4uLU6hMCjMI75M1A2tKUQC"),
            None
        );
        assert_eq!(track_from_url("spotify:track:short"), None);
        assert_eq!(
            track_from_url("spotify:track:4uLU6hMCjMI75M1A2tKUQC:extra"),
            None
        );
        assert_eq!(
            track_from_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            None
        );
    }
}
