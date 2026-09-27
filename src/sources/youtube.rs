//! `YouTubeMusicSource` (CONTEXT.md, ADR 0006): structured search, library and collections via
//! the YouTube Music InnerTube API (`ytmapi-rs`), stream extraction via `yt-dlp` at play time.
//!
//! One unauthenticated client serves search and public pages; a cookie-authenticated client
//! (Flatpak Browser Session Bridge jar) serves the library and private playlists. Both are
//! built lazily once and reused, so a warm search is a single HTTPS round trip.

use crate::model::{
    Collection, CollectionKind, LibrarySection, MediaKind, Playable, SearchFilter, SearchItem,
    SourceKind, Track, parse_duration,
};
use crate::sources::cookies::{self, ErrorLinePick, PrivateTemp};
use crate::sources::{AudioSource, Resolved, SourceError, SourceResult};
use futures::future::BoxFuture;
use futures::{FutureExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{OnceCell, RwLock};
use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::auth::{AuthToken, BrowserToken};
use ytmapi_rs::common::{
    AlbumID, ArtistChannelID, PlaylistID, PodcastID, Thumbnail, VideoID, YoutubeID,
};
use ytmapi_rs::continuations::ParseFromContinuable;
use ytmapi_rs::error::ErrorKind;
use ytmapi_rs::query::search::{
    BasicSearch, EpisodesFilter, FilteredSearch, PodcastsFilter, SongsFilter, VideosFilter,
};
use ytmapi_rs::query::{
    GetLibraryAlbumsQuery, GetLibraryArtistSubscriptionsQuery, GetLibraryArtistsQuery,
    GetLibraryPlaylistsQuery, GetLibraryPodcastsQuery, GetNewEpisodesQuery, GetPlaylistTracksQuery,
    GetPodcastQuery, PostQuery, Query, SearchQuery,
};
use ytmapi_rs::{YtMusic, YtMusicBuilder};

/// Upper bound for one InnerTube operation (reqwest has no default timeout).
const API_TIMEOUT: Duration = Duration::from_secs(20);
/// Upper bound for multi-page operations (whole playlists, library).
const PAGED_TIMEOUT: Duration = Duration::from_secs(90);
/// Continuation pages fetched at most per list (~100 items each), bounding memory.
const MAX_PAGES: usize = 100;
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(45);
/// Audio for songs and episodes: Opus when available (playbin3 decodes it natively).
const AUDIO_FORMAT: &str = "bestaudio[acodec=opus]/bestaudio/best";
/// Video must be ONE muxed URI for playbin3. YouTube's default clients now only offer split
/// DASH/HLS (the HLS master mixes VP9 and H.264 variants, which hlsdemux2 cannot switch
/// between), so the `tv_simply` client is added for its progressive muxed MP4 (360p). `mweb`,
/// `web_embedded` and `android_vr` also list it, but their URLs answer 403 without a PO token.
const VIDEO_FORMAT: &str =
    "best[height<=720][vcodec!=none][acodec!=none]/best[vcodec!=none][acodec!=none]";
const VIDEO_EXTRACTOR_ARGS: &str = "youtube:player_client=default,tv_simply";
/// Liked Music playlist id.
const LIKED_SONGS_ID: &str = "LM";
/// "New Episodes" auto playlist id.
const NEW_EPISODES_ID: &str = "RDPN";
const MIN_THUMB_WIDTH: u64 = 120;

/// A YouTube (Music) link parsed from a URL (MPRIS `OpenUri`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoLink {
    pub id: String,
    /// `Music` for music.youtube.com links, `Video` for youtube.com / youtu.be links.
    pub kind: MediaKind,
}

/// Parse music.youtube.com / youtube.com / youtu.be watch, shorts, live and embed links.
pub fn parse_video_url(url: &str) -> Option<VideoLink> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let kind = match host {
        "music.youtube.com" => MediaKind::Music,
        "youtube.com" | "m.youtube.com" | "youtube-nocookie.com" | "youtu.be" => MediaKind::Video,
        _ => return None,
    };
    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let id = if host == "youtu.be" {
        segments.first().map(|s| s.to_string())
    } else {
        match segments.as_slice() {
            ["watch"] => parsed
                .query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.into_owned()),
            ["shorts" | "live" | "embed" | "v", id, ..] => Some(id.to_string()),
            _ => None,
        }
    }?;
    is_video_id(&id).then_some(VideoLink { id, kind })
}

fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

enum AuthSlot {
    /// Not built yet (or last build failed): build on next use.
    Unloaded,
    /// No usable jar.
    Missing,
    Ready(Arc<YtMusic<BrowserToken>>),
}

struct Inner {
    anon: OnceCell<YtMusic<NoAuthToken>>,
    auth: RwLock<AuthSlot>,
    signed_in: AtomicBool,
    ytdlp: OsString,
    /// At most one automatic session re-import per run.
    refresh_tried: AtomicBool,
    /// yt-dlp reported that the imported cookies were rotated away.
    cookies_rotated: AtomicBool,
}

/// YouTube Music / YouTube / YouTube podcasts Audio Source.
pub struct YouTubeMusicSource {
    inner: Arc<Inner>,
}

impl Default for YouTubeMusicSource {
    fn default() -> Self {
        Self::new()
    }
}

impl YouTubeMusicSource {
    /// Cheap: no network. Clients are built on first use.
    pub fn new() -> Self {
        YouTubeMusicSource {
            inner: Arc::new(Inner {
                anon: OnceCell::new(),
                auth: RwLock::new(AuthSlot::Unloaded),
                signed_in: AtomicBool::new(cookies::has_jar()),
                refresh_tried: AtomicBool::new(false),
                cookies_rotated: AtomicBool::new(false),
                ytdlp: cookies::ytdlp_program(),
            }),
        }
    }

    /// Build the unauthenticated client now so the first search is a single round trip.
    pub fn prewarm(&self) -> BoxFuture<'static, SourceResult<()>> {
        let inner = self.inner.clone();
        async move { inner.anon().await.map(|_| ()) }.boxed()
    }

    /// Rebuild the authenticated client from the jar (after an import or sign-out).
    /// `Ok(true)`: signed in; `Ok(false)`: no jar.
    pub fn reload_auth(&self) -> BoxFuture<'static, SourceResult<bool>> {
        let inner = self.inner.clone();
        async move {
            let mut slot = inner.auth.write().await;
            *slot = AuthSlot::Unloaded;
            match inner.load_auth(&mut slot).await {
                Ok(_) => Ok(true),
                Err(_) if matches!(*slot, AuthSlot::Missing) => Ok(false),
                Err(e) => Err(e),
            }
        }
        .boxed()
    }

    /// The browser rotated the imported session: re-import it once from the browser it came
    /// from (or the only Flatpak browser found) and rebuild the client. `Ok(true)` when a
    /// fresh session is in place; `Ok(false)` when no automatic refresh is possible.
    pub fn refresh_session(&self) -> BoxFuture<'static, SourceResult<bool>> {
        let this = Self {
            inner: self.inner.clone(),
        };
        async move {
            if this.inner.refresh_tried.swap(true, Ordering::SeqCst) {
                return Ok(false);
            }
            let spec = cookies::last_browser_spec().or_else(|| {
                let found = cookies::detect_browsers();
                (found.len() == 1).then(|| found[0].spec.clone())
            });
            let Some(spec) = spec else { return Ok(false) };
            log::info!("YouTube Music session rejected; re-importing from {spec}");
            cookies::import_from_browser(&spec).await?;
            this.reload_auth().await
        }
        .boxed()
    }

    /// Delete the jar and drop the authenticated client.
    pub fn sign_out(&self) -> BoxFuture<'static, SourceResult<()>> {
        let inner = self.inner.clone();
        async move {
            cookies::sign_out()?;
            *inner.auth.write().await = AuthSlot::Missing;
            inner.signed_in.store(false, Ordering::SeqCst);
            Ok(())
        }
        .boxed()
    }

    /// Metadata for a video id (e.g. from `parse_video_url`), from the YouTube Music watch
    /// playlist whose first entry is the video itself.
    pub fn lookup_video(
        &self,
        id: String,
        kind: MediaKind,
    ) -> BoxFuture<'static, SourceResult<Track>> {
        let inner = self.inner.clone();
        async move {
            if !is_video_id(&id) {
                return Err(SourceError::NotFound);
            }
            let yt = inner.anon().await?;
            let found = timed(
                API_TIMEOUT,
                yt.get_watch_playlist_from_video_id(VideoID::from_raw(id.as_str())),
            )
            .await
            .and_then(|tracks| {
                tracks
                    .into_iter()
                    .find(|t| t.video_id.get_raw() == id)
                    .ok_or(SourceError::NotFound)
            });
            let track = match found {
                Ok(t) => t,
                Err(e) => {
                    // Plain youtube.com videos often have no music queue: ask yt-dlp instead.
                    log::info!(
                        "watch playlist lookup for {id} failed ({e}); falling back to yt-dlp"
                    );
                    return inner.ytdlp_metadata(id, kind).await;
                }
            };
            Ok(Track {
                id,
                kind,
                source: SourceKind::YouTubeMusic,
                title: track.title,
                artist: track.author,
                artist_id: None,
                album: None,
                duration_secs: parse_text_duration(&track.duration),
                thumbnail_url: pick_thumbnail(&track.thumbnails),
            })
        }
        .boxed()
    }
}

impl Inner {
    async fn anon(&self) -> SourceResult<&YtMusic<NoAuthToken>> {
        self.anon
            .get_or_try_init(|| async { timed(API_TIMEOUT, YtMusicBuilder::new().build()).await })
            .await
    }

    async fn auth(&self) -> SourceResult<Arc<YtMusic<BrowserToken>>> {
        {
            let slot = self.auth.read().await;
            match &*slot {
                AuthSlot::Ready(yt) => return Ok(yt.clone()),
                AuthSlot::Missing => return Err(not_signed_in()),
                AuthSlot::Unloaded => {}
            }
        }
        let mut slot = self.auth.write().await;
        match &*slot {
            AuthSlot::Ready(yt) => Ok(yt.clone()),
            AuthSlot::Missing => Err(not_signed_in()),
            AuthSlot::Unloaded => self.load_auth(&mut slot).await,
        }
    }

    /// Build the authenticated client into `slot` (held under the write lock).
    async fn load_auth(&self, slot: &mut AuthSlot) -> SourceResult<Arc<YtMusic<BrowserToken>>> {
        let Some(header) = cookies::read_jar()
            .as_deref()
            .and_then(cookies::cookie_header)
        else {
            *slot = AuthSlot::Missing;
            self.signed_in.store(false, Ordering::SeqCst);
            return Err(not_signed_in());
        };
        match timed(
            API_TIMEOUT,
            YtMusicBuilder::new()
                .with_browser_token_cookie(header)
                .build(),
        )
        .await
        {
            Ok(yt) => {
                let yt = Arc::new(yt);
                *slot = AuthSlot::Ready(yt.clone());
                self.signed_in.store(true, Ordering::SeqCst);
                Ok(yt)
            }
            Err(e) => {
                log::warn!("cannot build the signed-in YouTube Music client: {e}");
                if matches!(e, SourceError::AuthRequired(_)) {
                    self.signed_in.store(false, Ordering::SeqCst);
                }
                Err(e)
            }
        }
    }

    async fn search(&self, query: String, filter: SearchFilter) -> SourceResult<Vec<SearchItem>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let yt = self.anon().await?;
        match filter {
            SearchFilter::All => Ok(search_page(yt, SearchQuery::<BasicSearch>::from(query))
                .await?
                .into_items(&[])),
            SearchFilter::Music => Ok(search_page(
                yt,
                SearchQuery::<FilteredSearch<SongsFilter>>::from(query),
            )
            .await?
            .into_items(&[Group::Songs])),
            SearchFilter::Videos => Ok(search_page(
                yt,
                SearchQuery::<FilteredSearch<VideosFilter>>::from(query),
            )
            .await?
            .into_items(&[Group::Videos, Group::Episodes])),
            SearchFilter::Podcasts => {
                let (episodes, podcasts) = futures::join!(
                    search_page(
                        yt,
                        SearchQuery::<FilteredSearch<EpisodesFilter>>::from(query)
                    ),
                    search_page(
                        yt,
                        SearchQuery::<FilteredSearch<PodcastsFilter>>::from(query)
                    ),
                );
                let mut items = Vec::new();
                let mut first_err = None;
                match episodes {
                    Ok(page) => items.extend(page.into_items(&[Group::Episodes])),
                    Err(e) => {
                        log::warn!("YouTube episode search failed: {e}");
                        first_err = Some(e);
                    }
                }
                match podcasts {
                    Ok(page) => items.extend(page.into_items(&[Group::Podcasts])),
                    Err(e) => {
                        log::warn!("YouTube podcast search failed: {e}");
                        if let Some(first) = first_err {
                            return Err(first);
                        }
                    }
                }
                Ok(items)
            }
        }
    }

    async fn library(&self) -> SourceResult<Vec<LibrarySection>> {
        if !self.signed_in.load(Ordering::SeqCst) {
            return Err(not_signed_in());
        }
        let yt = self.auth().await?;
        let yt = &*yt;
        let albums_query = GetLibraryAlbumsQuery::default();
        let subscriptions_query = GetLibraryArtistSubscriptionsQuery::default();
        let artists_query = GetLibraryArtistsQuery::default();
        let podcasts_query = GetLibraryPodcastsQuery::default();
        let (playlists, albums, subscriptions, artists, podcasts) =
            timed_result(PAGED_TIMEOUT, async {
                Ok(futures::join!(
                    library_collections(yt, &GetLibraryPlaylistsQuery, CollectionKind::Playlist),
                    library_collections(yt, &albums_query, CollectionKind::Album),
                    library_collections(yt, &subscriptions_query, CollectionKind::Artist),
                    library_collections(yt, &artists_query, CollectionKind::Artist),
                    library_collections(yt, &podcasts_query, CollectionKind::Podcast),
                ))
            })
            .await?;

        let mut first_err: Option<SourceError> = None;
        let mut ok = |name: &str, r: SourceResult<Vec<Collection>>| -> Option<Vec<Collection>> {
            match r {
                Ok(v) => Some(v),
                Err(e) => {
                    log::warn!("YouTube library section {name} failed: {e}");
                    first_err.get_or_insert(e);
                    None
                }
            }
        };
        let playlists = ok(
            "Playlists",
            playlists.map(|v| v.into_iter().filter(|p| p.id != LIKED_SONGS_ID).collect()),
        );
        let albums = ok("Albums", albums);
        let subscriptions = ok("Artists", subscriptions);
        let artists = ok("Artists", artists);
        let podcasts = ok("Podcasts", podcasts);

        if playlists.is_none()
            && albums.is_none()
            && subscriptions.is_none()
            && artists.is_none()
            && podcasts.is_none()
        {
            return Err(first_err.unwrap_or_else(not_signed_in));
        }
        let mut sections = Vec::with_capacity(5);
        if let Some(collections) = playlists {
            sections.push(LibrarySection {
                title: "Playlists".to_string(),
                collections,
            });
        }
        sections.push(LibrarySection {
            title: "Liked Songs".to_string(),
            collections: vec![Collection {
                id: LIKED_SONGS_ID.to_string(),
                source: SourceKind::YouTubeMusic,
                kind: CollectionKind::LikedSongs,
                title: "Liked Songs".to_string(),
                subtitle: "YouTube Music".to_string(),
                thumbnail_url: None,
            }],
        });
        if let Some(collections) = albums {
            sections.push(LibrarySection {
                title: "Albums".to_string(),
                collections,
            });
        }
        if subscriptions.is_some() || artists.is_some() {
            let mut seen = HashSet::new();
            let collections = subscriptions
                .into_iter()
                .flatten()
                .chain(artists.into_iter().flatten())
                .filter(|c| seen.insert(c.id.clone()))
                .collect();
            sections.push(LibrarySection {
                title: "Artists".to_string(),
                collections,
            });
        }
        if let Some(collections) = podcasts {
            sections.push(LibrarySection {
                title: "Podcasts".to_string(),
                collections,
            });
        }
        Ok(sections)
    }

    async fn collection(&self, collection: Collection) -> SourceResult<Vec<Track>> {
        match collection.kind {
            CollectionKind::LikedSongs => {
                let yt = self.auth().await?;
                playlist_tracks(&*yt, LIKED_SONGS_ID).await
            }
            CollectionKind::Playlist if collection.id == NEW_EPISODES_ID => {
                // The "New Episodes" auto playlist is an episode feed, not a playlist page.
                let yt = self.auth().await?;
                let json = raw_page(&*yt, GetNewEpisodesQuery).await?;
                Ok(episode_rows(
                    &json,
                    &collection.title,
                    &collection.title,
                    collection.thumbnail_url.as_deref(),
                ))
            }
            CollectionKind::Playlist => {
                if self.signed_in.load(Ordering::SeqCst) {
                    match self.auth().await {
                        Ok(yt) => return playlist_tracks(&*yt, &collection.id).await,
                        Err(e) => log::warn!("opening playlist without sign-in: {e}"),
                    }
                }
                playlist_tracks(self.anon().await?, &collection.id).await
            }
            CollectionKind::Album => {
                let yt = self.anon().await?;
                let album = timed(
                    API_TIMEOUT,
                    yt.get_album(AlbumID::from_raw(collection.id.as_str())),
                )
                .await?;
                let artist = join_names(album.artists.iter().map(|a| a.name.as_str()));
                let artist_id = album
                    .artists
                    .iter()
                    .find_map(|a| a.id.as_ref().map(|id| id.get_raw().to_string()));
                let thumbnail_url = pick_thumbnail(&album.thumbnails);
                let title = album.title;
                Ok(album
                    .tracks
                    .into_iter()
                    .map(|s| Track {
                        id: s.video_id.get_raw().to_string(),
                        kind: MediaKind::Music,
                        source: SourceKind::YouTubeMusic,
                        title: s.title,
                        artist: artist.clone(),
                        artist_id: artist_id.clone(),
                        album: Some(title.clone()),
                        duration_secs: parse_text_duration(&s.duration),
                        thumbnail_url: thumbnail_url.clone(),
                    })
                    .collect())
            }
            CollectionKind::Artist => {
                let yt = self.anon().await?;
                let artist = timed(
                    API_TIMEOUT,
                    yt.get_artist(ArtistChannelID::from_raw(collection.id.as_str())),
                )
                .await?;
                let channel_id = artist.channel_id.get_raw().to_string();
                let Some(songs) = artist.top_releases.songs else {
                    return Ok(Vec::new());
                };
                Ok(songs
                    .results
                    .into_iter()
                    .map(|s| {
                        let id = s.video_id.get_raw().to_string();
                        let thumbnail_url = Some(video_thumbnail(&id));
                        Track {
                            id,
                            kind: MediaKind::Music,
                            source: SourceKind::YouTubeMusic,
                            title: s.title,
                            artist: if s.artists.is_empty() {
                                artist.name.clone()
                            } else {
                                join_names(s.artists.iter().map(|a| a.name.as_str()))
                            },
                            artist_id: Some(channel_id.clone()),
                            album: Some(s.album.name),
                            duration_secs: None,
                            thumbnail_url,
                        }
                    })
                    .collect())
            }
            CollectionKind::Podcast => {
                let yt = self.anon().await?;
                let json = raw_page(
                    yt,
                    GetPodcastQuery::new(PodcastID::from_raw(collection.id.as_str())),
                )
                .await?;
                let header = "/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicResponsiveHeaderRenderer";
                let text_at = |key: &str| {
                    json.pointer(&format!("{header}/{key}"))
                        .map(|t| runs_text(runs_of(Some(t))))
                        .filter(|t| !t.is_empty())
                };
                let podcast = text_at("title").unwrap_or_else(|| collection.title.clone());
                let publisher = text_at("straplineTextOne").unwrap_or_else(|| podcast.clone());
                Ok(episode_rows(
                    &json,
                    &publisher,
                    &podcast,
                    collection.thumbnail_url.as_deref(),
                ))
            }
        }
    }

    /// Track metadata for a video id straight from yt-dlp (no stream selection).
    async fn ytdlp_metadata(&self, id: String, kind: MediaKind) -> SourceResult<Track> {
        let mut cmd = tokio::process::Command::new(&self.ytdlp);
        cmd.args([
            "-J",
            "--skip-download",
            "--ignore-no-formats-error",
            "--no-playlist",
            "--no-warnings",
            "--no-progress",
            "--",
        ])
        .arg(format!("https://www.youtube.com/watch?v={id}"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
        let child = cmd.spawn().map_err(cookies::spawn_error)?;
        let output = tokio::time::timeout(RESOLVE_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| {
                SourceError::Extraction(format!(
                    "yt-dlp timed out after {} s",
                    RESOLVE_TIMEOUT.as_secs()
                ))
            })?
            .map_err(|e| SourceError::Extraction(format!("yt-dlp failed: {e}")))?;
        if !output.status.success() {
            return Err(map_ytdlp_failure(&String::from_utf8_lossy(&output.stderr)));
        }
        parse_ytdlp_metadata(&output.stdout, id, kind)
    }

    async fn resolve(&self, track: Track) -> SourceResult<Resolved> {
        if track.source != SourceKind::YouTubeMusic {
            return Err(SourceError::Unavailable("not a YouTube item".to_string()));
        }
        if !is_video_id(&track.id) {
            return Err(SourceError::NotFound);
        }
        let video = track.kind == MediaKind::Video;
        // yt-dlp rewrites its --cookies file on exit: hand it a private per-call copy.
        let jar = if self.signed_in.load(Ordering::SeqCst) {
            match cookies::read_jar().map(|text| PrivateTemp::file(text.as_bytes())) {
                Some(Ok(temp)) => Some(temp),
                Some(Err(e)) => {
                    log::warn!("cannot prepare cookies for yt-dlp, resolving signed out: {e}");
                    None
                }
                None => None,
            }
        } else {
            None
        };
        let mut cmd = tokio::process::Command::new(&self.ytdlp);
        cmd.args(["-J", "--no-playlist", "--no-progress", "-f"])
            .arg(if video { VIDEO_FORMAT } else { AUDIO_FORMAT });
        if video {
            cmd.args(["--extractor-args", VIDEO_EXTRACTOR_ARGS]);
        }
        if let Some(jar) = &jar {
            cmd.arg("--cookies").arg(jar.path());
        }
        cmd.arg("--")
            .arg(format!("https://www.youtube.com/watch?v={}", track.id))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = cmd.spawn().map_err(cookies::spawn_error)?;
        let output = match tokio::time::timeout(RESOLVE_TIMEOUT, child.wait_with_output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return Err(SourceError::Extraction(format!("yt-dlp failed: {e}"))),
            Err(_) => {
                return Err(SourceError::Extraction(format!(
                    "yt-dlp timed out after {} s",
                    RESOLVE_TIMEOUT.as_secs()
                )));
            }
        };
        drop(jar);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("cookies are no longer valid") {
            self.cookies_rotated.store(true, Ordering::SeqCst);
        }
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::warn!("yt-dlp failed for {}: {}", track.id, stderr.trim());
            return Err(map_ytdlp_failure(&stderr));
        }
        parse_ytdlp_json(&output.stdout, video)
    }
}

impl AudioSource for YouTubeMusicSource {
    fn kind(&self) -> SourceKind {
        SourceKind::YouTubeMusic
    }

    fn is_signed_in(&self) -> bool {
        self.inner.signed_in.load(Ordering::SeqCst)
    }

    fn search(
        &self,
        query: String,
        filter: SearchFilter,
    ) -> BoxFuture<'static, SourceResult<Vec<SearchItem>>> {
        let inner = self.inner.clone();
        async move { inner.search(query, filter).await }.boxed()
    }

    fn library(&self) -> BoxFuture<'static, SourceResult<Vec<LibrarySection>>> {
        let inner = self.inner.clone();
        async move { inner.library().await }.boxed()
    }

    fn collection(&self, collection: Collection) -> BoxFuture<'static, SourceResult<Vec<Track>>> {
        let inner = self.inner.clone();
        async move { inner.collection(collection).await }.boxed()
    }

    fn resolve(&self, track: Track) -> BoxFuture<'static, SourceResult<Resolved>> {
        let this = Self {
            inner: self.inner.clone(),
        };
        async move {
            let r = this.inner.resolve(track).await;
            // Playback still works signed out; refresh the session in the background.
            if this.inner.cookies_rotated.swap(false, Ordering::SeqCst) {
                let refresh = this.refresh_session();
                tokio::spawn(async move {
                    match refresh.await {
                        Ok(true) => log::info!(
                            "YouTube Music session refreshed after yt-dlp reported rotated cookies"
                        ),
                        Ok(false) => log::warn!(
                            "YouTube Music cookies were rotated; import the session again"
                        ),
                        Err(e) => log::warn!("refreshing the YouTube Music session failed: {e}"),
                    }
                });
            }
            r
        }
        .boxed()
    }
}

// ---------------------------------------------------------------------------------------------
// InnerTube helpers

async fn timed<T>(
    limit: Duration,
    fut: impl Future<Output = ytmapi_rs::Result<T>>,
) -> SourceResult<T> {
    match tokio::time::timeout(limit, fut).await {
        Ok(result) => result.map_err(map_api_error),
        Err(_) => Err(SourceError::Network(format!(
            "YouTube Music did not answer within {} s",
            limit.as_secs()
        ))),
    }
}

async fn timed_result<T>(
    limit: Duration,
    fut: impl Future<Output = SourceResult<T>>,
) -> SourceResult<T> {
    tokio::time::timeout(limit, fut).await.unwrap_or_else(|_| {
        Err(SourceError::Network(format!(
            "YouTube Music did not answer within {} s",
            limit.as_secs()
        )))
    })
}

/// Feed every page of a continuable browse query (bounded by `MAX_PAGES`) to `on_page` as raw
/// JSON. Pages are parsed by Banshee's tolerant readers: ytmapi's typed parsers reject a whole
/// page when one row lacks a field. ytmapi still drives the continuation requests, so a page
/// its parser rejects ends the list there (logged). A failing first page is an error.
async fn raw_pages<A, Q>(
    yt: &YtMusic<A>,
    query: &Q,
    mut on_page: impl FnMut(&Value),
) -> SourceResult<()>
where
    A: AuthToken,
    Q: Query<A> + PostQuery,
    Q::Output: ParseFromContinuable<Q>,
{
    let stream = yt.raw_json_stream(query).take(MAX_PAGES);
    futures::pin_mut!(stream);
    let mut first = true;
    while let Some(page) = stream.next().await {
        let page = page.map_err(map_api_error).and_then(|raw| {
            let json = serde_json::from_str::<Value>(&raw).map_err(|e| {
                SourceError::Parse(format!("YouTube Music response is not JSON: {e}"))
            })?;
            check_error(&json)?;
            Ok(json)
        });
        match page {
            Ok(json) => on_page(&json),
            Err(e) if first => return Err(e),
            Err(e) => {
                log::warn!("YouTube Music continuation failed, list truncated: {e}");
                break;
            }
        }
        first = false;
    }
    Ok(())
}

async fn playlist_tracks<A: AuthToken>(
    yt: &YtMusic<A>,
    playlist_id: &str,
) -> SourceResult<Vec<Track>> {
    let query = GetPlaylistTracksQuery::new(PlaylistID::from_raw(playlist_browse_id(playlist_id)));
    let mut out = Vec::new();
    timed_result(
        PAGED_TIMEOUT,
        raw_pages(yt, &query, |json| playlist_rows(json, &mut out)),
    )
    .await?;
    Ok(out)
}

/// Playable rows of a playlist page or continuation, skipping greyed-out (unavailable) ones.
/// Rows outside the playlist shelf (e.g. suggestions) are ignored.
fn playlist_rows(json: &Value, out: &mut Vec<Track>) {
    match json {
        Value::Object(map) => {
            let shelf = map
                .get("musicPlaylistShelfRenderer")
                .or_else(|| map.get("musicPlaylistShelfContinuation"))
                .and_then(|s| s.get("contents"))
                .or_else(|| map.get("continuationItems"));
            match shelf.and_then(Value::as_array) {
                Some(rows) => out.extend(
                    rows.iter()
                        .filter_map(|r| r.get("musicResponsiveListItemRenderer"))
                        .filter_map(playlist_row_track),
                ),
                None => map.values().for_each(|v| playlist_rows(v, out)),
            }
        }
        Value::Array(items) => items.iter().for_each(|v| playlist_rows(v, out)),
        _ => {}
    }
}

/// Playlist row: title / artists / album columns, duration in the fixed column.
fn playlist_row_track(row: &Value) -> Option<Track> {
    if row
        .get("musicItemRendererDisplayPolicy")
        .and_then(Value::as_str)
        == Some("MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT")
    {
        return None;
    }
    let column = |i: usize| {
        row.get("flexColumns")
            .and_then(|c| c.get(i))
            .and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"))
            .map(|c| runs_of(c.get("text")))
            .unwrap_or_default()
    };
    let title_runs = column(0);
    let watch = row
        .pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint")
        .and_then(watch_target)
        .or_else(|| title_runs.first()?.get("navigationEndpoint").and_then(watch_target));
    let id = row
        .pointer("/playlistItemData/videoId")
        .and_then(Value::as_str)
        .or(watch.map(|(id, _)| id))?;
    if !is_video_id(id) {
        return None;
    }
    let title = runs_text(title_runs);
    if title.is_empty() {
        return None;
    }
    let artists = column(1);
    let album_runs = column(2);
    let has_album = album_runs.iter().any(|r| {
        r.get("navigationEndpoint")
            .and_then(browse_target)
            .is_some_and(|(_, p)| p == "MUSIC_PAGE_TYPE_ALBUM")
    });
    let kind = match watch.map(|(_, t)| t).unwrap_or_default() {
        "MUSIC_VIDEO_TYPE_PODCAST_EPISODE" => MediaKind::Episode,
        "MUSIC_VIDEO_TYPE_ATV" => MediaKind::Music,
        _ if has_album => MediaKind::Music,
        _ => MediaKind::Video,
    };
    let artist_id =
        artists.iter().find_map(
            |r| match r.get("navigationEndpoint").and_then(browse_target) {
                Some((id, "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL"))
                    if id.starts_with("UC") =>
                {
                    Some(id.to_string())
                }
                _ => None,
            },
        );
    let duration = row
        .get("fixedColumns")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("musicResponsiveListItemFixedColumnRenderer"))
        .map(|c| runs_text(runs_of(c.get("text"))));
    Some(Track {
        id: id.to_string(),
        kind,
        source: SourceKind::YouTubeMusic,
        title,
        artist: runs_text(artists),
        artist_id,
        album: Some(runs_text(album_runs)).filter(|a| !a.is_empty()),
        duration_secs: duration.as_deref().and_then(parse_text_duration),
        thumbnail_url: row.get("thumbnail").and_then(json_thumbnail),
    })
}

/// Collections of one library page type.
async fn library_collections<Q>(
    yt: &YtMusic<BrowserToken>,
    query: &Q,
    kind: CollectionKind,
) -> SourceResult<Vec<Collection>>
where
    Q: Query<BrowserToken> + PostQuery,
    Q::Output: ParseFromContinuable<Q>,
{
    let mut out = Vec::new();
    let mut logged_out = false;
    raw_pages(yt, query, |json| {
        logged_out |= reports_logged_out(json);
        collect_tiles(json, &mut |item| {
            if let (_, SearchItem::Collection(c)) = item
                && c.kind == kind
            {
                out.push(c);
            }
        })
    })
    .await?;
    if logged_out {
        // YouTube answers with an empty, signed-out page when the imported cookies
        // have been rotated away by the browser: say so instead of showing nothing.
        return Err(SourceError::AuthRequired(SESSION_EXPIRED.to_string()));
    }
    let mut seen = HashSet::new();
    out.retain(|c| seen.insert(c.id.clone()));
    Ok(out)
}

const SESSION_EXPIRED: &str =
    "your YouTube Music session has expired; import it again from Accounts";

/// InnerTube reports the session state in `responseContext.serviceTrackingParams`
/// (`logged_in` = `0` / `1`).
fn reports_logged_out(json: &Value) -> bool {
    json.pointer("/responseContext/serviceTrackingParams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|svc| svc.get("params").and_then(Value::as_array))
        .flatten()
        .any(|p| {
            p.get("key").and_then(Value::as_str) == Some("logged_in")
                && p.get("value").and_then(Value::as_str) == Some("0")
        })
}

/// Every grid tile / list row anywhere in a browse response, in document order.
fn collect_tiles(json: &Value, sink: &mut dyn FnMut((Group, SearchItem))) {
    match json {
        Value::Object(map) => {
            if let Some(tile) = map.get("musicTwoRowItemRenderer") {
                let title = runs_text(runs_of(tile.get("title")));
                let thumb = tile.get("thumbnailRenderer").and_then(json_thumbnail);
                if let Some(endpoint) = tile.get("navigationEndpoint")
                    && let Some(item) =
                        classify(endpoint, title, runs_of(tile.get("subtitle")), thumb)
                {
                    sink(item);
                }
            } else if let Some(row) = map.get("musicResponsiveListItemRenderer") {
                if let Some(item) = parse_list_item(row) {
                    sink(item);
                }
            } else {
                map.values().for_each(|v| collect_tiles(v, sink));
            }
        }
        Value::Array(items) => items.iter().for_each(|v| collect_tiles(v, sink)),
        _ => {}
    }
}

fn not_signed_in() -> SourceError {
    SourceError::AuthRequired("import your YouTube browser session to see your library".to_string())
}

/// Map ytmapi errors onto the source error kinds.
fn map_api_error(e: ytmapi_rs::Error) -> SourceError {
    let message = e.to_string();
    log::debug!("YouTube Music API error: {message}");
    match e.into_kind() {
        ErrorKind::Web { message } => SourceError::Network(short(&message)),
        ErrorKind::Io(err) => SourceError::Network(short(&err.to_string())),
        ErrorKind::Header => SourceError::AuthRequired(
            "the imported YouTube session is incomplete (no SAPISID cookie); import it again"
                .to_string(),
        ),
        ErrorKind::OAuthTokenExpired { .. } => {
            SourceError::AuthRequired("the YouTube session expired".to_string())
        }
        ErrorKind::OtherErrorCodeInResponse { code, message } => map_http_code(code, &message),
        ErrorKind::ApiStatusFailed => {
            SourceError::Unavailable("YouTube Music rejected the request".to_string())
        }
        _ => SourceError::Parse(short(&message)),
    }
}

fn map_http_code(code: u64, message: &str) -> SourceError {
    match code {
        401 | 403 => SourceError::AuthRequired(format!(
            "YouTube Music refused the session ({code}); import your browser session again"
        )),
        400 | 404 => SourceError::NotFound,
        429 => SourceError::RateLimited {
            retry_after_secs: 60,
        },
        _ => SourceError::Network(short(&format!(
            "YouTube Music returned HTTP {code}: {message}"
        ))),
    }
}

/// Keep user-facing messages short (some ytmapi errors embed whole responses).
fn short(s: &str) -> String {
    const MAX: usize = 200;
    match s.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}…", &s[..cut]),
        None => s.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// Mapping

/// Smallest thumbnail at least `MIN_THUMB_WIDTH` wide, else the largest.
fn pick_thumbnail(thumbs: &[Thumbnail]) -> Option<String> {
    thumbs
        .iter()
        .filter(|t| t.width >= MIN_THUMB_WIDTH)
        .min_by_key(|t| t.width)
        .or_else(|| thumbs.iter().max_by_key(|t| t.width))
        .map(|t| t.url.clone())
}

fn video_thumbnail(video_id: &str) -> String {
    format!("https://i.ytimg.com/vi/{video_id}/mqdefault.jpg")
}

/// `m:ss`, `h:mm:ss`, or InnerTube's spelled-out podcast lengths (`1 hr 5 min`, `58 min`).
fn parse_text_duration(text: &str) -> Option<u32> {
    if let Some(secs) = parse_duration(text) {
        return Some(secs);
    }
    let mut total: u32 = 0;
    let mut matched = false;
    let mut words = text.split_whitespace().peekable();
    while let Some(word) = words.next() {
        let Ok(value) = word.parse::<u32>() else {
            continue;
        };
        let Some(unit) = words.peek().map(|u| u.to_ascii_lowercase()) else {
            break;
        };
        let factor = if unit.starts_with('h') {
            3600
        } else if unit.starts_with("m") {
            60
        } else if unit.starts_with('s') {
            1
        } else {
            continue;
        };
        total = total.checked_add(value.checked_mul(factor)?)?;
        matched = true;
        words.next();
    }
    matched.then_some(total)
}

fn join_names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    names
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn subtitle(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|p| !p.trim().is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Collection ids are plain playlist ids; InnerTube browses them as `VL<id>`.
fn normalize_playlist_id(raw: &str) -> &str {
    raw.strip_prefix("VL").unwrap_or(raw)
}

fn playlist_browse_id(id: &str) -> String {
    format!("VL{}", normalize_playlist_id(id))
}

/// Podcast episodes are browsed as `MPED<video id>`; Tracks carry the playable video id.
fn episode_video_id(raw: &str) -> String {
    raw.strip_prefix("MPED").unwrap_or(raw).to_string()
}

/// Library artists are browsed as `MPLA<channel id>`; Collections carry the channel id.
fn artist_channel_id(raw: &str) -> &str {
    raw.strip_prefix("MPLA").unwrap_or(raw)
}

/// One raw InnerTube page, with in-band error codes mapped.
async fn raw_page<A: AuthToken, Q: Query<A>>(yt: &YtMusic<A>, query: Q) -> SourceResult<Value> {
    let raw = timed(API_TIMEOUT, yt.raw_json_query(query)).await?;
    let json: Value = serde_json::from_str(&raw)
        .map_err(|e| SourceError::Parse(format!("YouTube Music response is not JSON: {e}")))?;
    check_error(&json)?;
    Ok(json)
}

fn check_error(json: &Value) -> SourceResult<()> {
    match json.get("error") {
        Some(err) => Err(map_http_code(
            err.get("code").and_then(Value::as_u64).unwrap_or(0),
            err.get("message")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )),
        None => Ok(()),
    }
}

/// Podcast episode rows (`musicMultiRowListItemRenderer`) of a podcast page or episode feed.
/// A row naming its show overrides `artist`/`album`.
fn episode_rows(
    json: &Value,
    artist: &str,
    album: &str,
    fallback_thumb: Option<&str>,
) -> Vec<Track> {
    fn walk<'a>(json: &'a Value, out: &mut Vec<&'a Value>) {
        match json {
            Value::Object(map) => match map.get("musicMultiRowListItemRenderer") {
                Some(row) => out.push(row),
                None => map.values().for_each(|v| walk(v, out)),
            },
            Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let mut rows = Vec::new();
    walk(json, &mut rows);
    rows.into_iter()
        .filter_map(|row| {
            let title_runs = runs_of(row.get("title"));
            let id = row
                .get("onTap")
                .and_then(watch_target)
                .map(|(id, _)| id.to_string())
                .or_else(|| {
                    title_runs
                        .first()?
                        .get("navigationEndpoint")
                        .and_then(browse_target)
                        .map(|(id, _)| episode_video_id(id))
                })
                .filter(|id| is_video_id(id))?;
            let title = runs_text(title_runs);
            if title.is_empty() {
                return None;
            }
            let show = Some(runs_text(runs_of(row.get("secondTitle")))).filter(|s| !s.is_empty());
            let duration =
                row.pointer("/playbackProgress/musicPlaybackProgressRenderer/durationText");
            Some(Track {
                id,
                kind: MediaKind::Episode,
                source: SourceKind::YouTubeMusic,
                title,
                artist: show.clone().unwrap_or_else(|| artist.to_string()),
                artist_id: None,
                album: Some(show.unwrap_or_else(|| album.to_string())),
                duration_secs: parse_text_duration(&runs_text(runs_of(duration))),
                thumbnail_url: row
                    .get("thumbnail")
                    .and_then(json_thumbnail)
                    .or_else(|| fallback_thumb.map(str::to_string)),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Search
//
// Search responses are parsed from raw InnerTube JSON. The unfiltered search no longer groups
// results into titled shelves (which `ytmapi-rs` 0.3 expects — it returns only id-less top
// results), but a top-result card followed by self-describing list items; and the typed
// filtered-search parsers fail the whole list when one item lacks a field. Every list item
// carries its own endpoint and type, so one tolerant item parser serves all searches.

/// Result groups in display order after the top results.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Songs,
    Videos,
    Episodes,
    Albums,
    Artists,
    Playlists,
    Podcasts,
}
const GROUPS: usize = 7;

/// One parsed search response.
#[derive(Default)]
struct SearchPage {
    /// The top-result card and its related items, in order.
    top: Vec<(Group, SearchItem)>,
    groups: [Vec<SearchItem>; GROUPS],
}

impl SearchPage {
    /// Top results first, then songs, videos, episodes, albums, artists, playlists, podcasts,
    /// each in relevance order, deduplicated. `only` restricts the groups (empty = all).
    fn into_items(self, only: &[Group]) -> Vec<SearchItem> {
        let wanted = |g: Group| only.is_empty() || only.contains(&g);
        let SearchPage { top, groups } = self;
        let top = top
            .into_iter()
            .filter(|(g, _)| wanted(*g))
            .map(|(_, item)| item);
        let rest = [
            Group::Songs,
            Group::Videos,
            Group::Episodes,
            Group::Albums,
            Group::Artists,
            Group::Playlists,
            Group::Podcasts,
        ]
        .into_iter()
        .zip(groups)
        .filter(|(g, _)| wanted(*g))
        .flat_map(|(_, items)| items);
        let mut seen = HashSet::new();
        top.chain(rest)
            .filter(|item| {
                seen.insert(match item {
                    SearchItem::Track(t) => t.key(),
                    SearchItem::Collection(c) => c.key(),
                })
            })
            .collect()
    }
}

async fn search_page<Q: Query<NoAuthToken>>(
    yt: &YtMusic<NoAuthToken>,
    query: Q,
) -> SourceResult<SearchPage> {
    parse_search(&raw_page(yt, query).await?)
}

fn parse_search(json: &Value) -> SourceResult<SearchPage> {
    check_error(json)?;
    let tabs = json
        .pointer("/contents/tabbedSearchResultsRenderer/tabs")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            SourceError::Parse("unexpected YouTube Music search response".to_string())
        })?;
    let sections = tabs
        .first()
        .and_then(|t| t.pointer("/tabRenderer/content/sectionListRenderer/contents"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    let mut page = SearchPage::default();
    for section in sections {
        if let Some(card) = section.get("musicCardShelfRenderer") {
            page.top.extend(parse_top_card(card));
            page.top
                .extend(list_items(card).filter_map(parse_list_item));
            continue;
        }
        for container in ["itemSectionRenderer", "musicShelfRenderer"] {
            if let Some(c) = section.get(container) {
                for (group, item) in list_items(c).filter_map(parse_list_item) {
                    page.groups[group as usize].push(item);
                }
            }
        }
    }
    Ok(page)
}

fn list_items(container: &Value) -> impl Iterator<Item = &Value> {
    container
        .get("contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("musicResponsiveListItemRenderer"))
}

fn runs_of(text: Option<&Value>) -> &[Value] {
    text.and_then(|t| t.get("runs"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn runs_text(runs: &[Value]) -> String {
    runs.iter()
        .filter_map(|r| r.get("text").and_then(Value::as_str))
        .collect::<String>()
        .trim()
        .to_string()
}

fn json_thumbnail(v: &Value) -> Option<String> {
    let thumbs: Vec<Thumbnail> = v
        .pointer("/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|t| {
            Some(Thumbnail {
                url: t.get("url")?.as_str()?.to_string(),
                width: t.get("width").and_then(Value::as_u64).unwrap_or(0),
                height: t.get("height").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect();
    pick_thumbnail(&thumbs)
}

/// `browseEndpoint` → (browse id, page type).
fn browse_target(endpoint: &Value) -> Option<(&str, &str)> {
    let browse = endpoint.get("browseEndpoint")?;
    let id = browse.get("browseId")?.as_str()?;
    let page = browse
        .pointer("/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some((id, page))
}

/// `watchEndpoint` → (video id, music video type).
fn watch_target(endpoint: &Value) -> Option<(&str, &str)> {
    let watch = endpoint.get("watchEndpoint")?;
    let id = watch.get("videoId")?.as_str()?;
    let kind = watch
        .pointer("/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some((id, kind))
}

/// Subtitle runs split at the ` • ` separators.
fn segments(runs: &[Value]) -> Vec<&[Value]> {
    runs.split(|r| {
        r.get("text")
            .and_then(Value::as_str)
            .is_some_and(|t| t.trim() == "•")
    })
    .filter(|s| !runs_text(s).is_empty())
    .collect()
}

const TYPE_LABELS: [&str; 10] = [
    "Song", "Video", "Episode", "Podcast", "Album", "Single", "EP", "Artist", "Playlist", "Profile",
];

fn is_type_label(segment: &[Value]) -> bool {
    let text = runs_text(segment);
    TYPE_LABELS.iter().any(|l| l.eq_ignore_ascii_case(&text))
}

fn parse_top_card(card: &Value) -> Option<(Group, SearchItem)> {
    let title_runs = runs_of(card.get("title"));
    let endpoint = title_runs.first()?.get("navigationEndpoint")?;
    let thumb = card.get("thumbnail").and_then(json_thumbnail);
    classify(
        endpoint,
        runs_text(title_runs),
        runs_of(card.get("subtitle")),
        thumb,
    )
}

fn parse_list_item(item: &Value) -> Option<(Group, SearchItem)> {
    let column = |i: usize| {
        item.get("flexColumns")
            .and_then(|c| c.get(i))
            .and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"))
            .map(|c| runs_of(c.get("text")))
            .unwrap_or_default()
    };
    let title_runs = column(0);
    let thumb = item.get("thumbnail").and_then(json_thumbnail);
    let endpoint = item
        .get("navigationEndpoint")
        .or_else(|| {
            item.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint")
        })
        .or_else(|| title_runs.first().and_then(|r| r.get("navigationEndpoint")))?;
    classify(endpoint, runs_text(title_runs), column(1), thumb)
}

fn classify(
    endpoint: &Value,
    title: String,
    sub_runs: &[Value],
    thumbnail_url: Option<String>,
) -> Option<(Group, SearchItem)> {
    if title.is_empty() {
        return None;
    }
    if let Some((id, video_type)) = watch_target(endpoint) {
        let kind = match video_type {
            "MUSIC_VIDEO_TYPE_ATV" => MediaKind::Music,
            "MUSIC_VIDEO_TYPE_PODCAST_EPISODE" => MediaKind::Episode,
            _ => MediaKind::Video,
        };
        let group = match kind {
            MediaKind::Music => Group::Songs,
            MediaKind::Video => Group::Videos,
            MediaKind::Episode => Group::Episodes,
        };
        let track = track_from_subtitle(id.to_string(), kind, title, sub_runs, thumbnail_url)?;
        return Some((group, SearchItem::Track(track)));
    }
    let (id, page) = browse_target(endpoint)?;
    let (group, kind, label) = match page {
        "MUSIC_PAGE_TYPE_ALBUM" => (Group::Albums, CollectionKind::Album, "Album"),
        "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_LIBRARY_ARTIST" => {
            (Group::Artists, CollectionKind::Artist, "Artist")
        }
        "MUSIC_PAGE_TYPE_PLAYLIST" => (Group::Playlists, CollectionKind::Playlist, "Playlist"),
        "MUSIC_PAGE_TYPE_PODCAST_SHOW_DETAIL_PAGE" => {
            (Group::Podcasts, CollectionKind::Podcast, "Podcast")
        }
        "MUSIC_PAGE_TYPE_NON_MUSIC_AUDIO_TRACK_PAGE" => {
            let track = track_from_subtitle(
                episode_video_id(id),
                MediaKind::Episode,
                title,
                sub_runs,
                thumbnail_url,
            )?;
            return Some((Group::Episodes, SearchItem::Track(track)));
        }
        _ => return None,
    };
    let parts = segments(sub_runs);
    let texts: Vec<String> = parts.iter().map(|s| runs_text(s)).collect();
    let mut detail: Vec<&str> = texts.iter().map(String::as_str).collect();
    if !parts.first().is_some_and(|s| is_type_label(s)) {
        detail.insert(0, label);
    }
    let id = match kind {
        CollectionKind::Playlist => normalize_playlist_id(id),
        CollectionKind::Artist => artist_channel_id(id),
        _ => id,
    };
    Some((
        group,
        SearchItem::Collection(Collection {
            id: id.to_string(),
            source: SourceKind::YouTubeMusic,
            kind,
            title,
            subtitle: subtitle(&detail),
            thumbnail_url,
        }),
    ))
}

/// Track from a subtitle like `Radiohead • OK Computer • 4:25` or `Channel • 1M views • 4:23`.
fn track_from_subtitle(
    id: String,
    kind: MediaKind,
    title: String,
    sub_runs: &[Value],
    thumbnail_url: Option<String>,
) -> Option<Track> {
    if !is_video_id(&id) {
        return None;
    }
    let mut parts = segments(sub_runs);
    if parts.first().is_some_and(|s| is_type_label(s)) {
        parts.remove(0);
    }
    fn target_of(run: &Value) -> Option<(&str, &str)> {
        run.get("navigationEndpoint").and_then(browse_target)
    }
    let links_to = |seg: &&[Value], page: &str| {
        seg.iter()
            .any(|r| target_of(r).is_some_and(|(_, p)| p == page))
    };
    // Episodes read `Apr 27, 2020 • Podcast name`: the byline is the segment linking the show.
    let byline = match kind {
        MediaKind::Episode => parts
            .iter()
            .find(|seg| links_to(seg, "MUSIC_PAGE_TYPE_PODCAST_SHOW_DETAIL_PAGE"))
            .or_else(|| parts.get(1))
            .or_else(|| parts.first()),
        MediaKind::Music | MediaKind::Video => parts.first(),
    };
    let artist_id = byline.and_then(|seg| {
        seg.iter().find_map(|r| match target_of(r) {
            Some((id, "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL"))
                if id.starts_with("UC") =>
            {
                Some(id.to_string())
            }
            _ => None,
        })
    });
    let album = parts
        .iter()
        .find(|seg| links_to(seg, "MUSIC_PAGE_TYPE_ALBUM"))
        .map(|seg| runs_text(seg));
    let duration_secs = parts.last().and_then(|seg| parse_duration(&runs_text(seg)));
    Some(Track {
        id,
        kind,
        source: SourceKind::YouTubeMusic,
        title,
        artist: byline.map(|seg| runs_text(seg)).unwrap_or_default(),
        artist_id,
        album,
        duration_secs,
        thumbnail_url,
    })
}

// ---------------------------------------------------------------------------------------------
// yt-dlp

#[derive(Deserialize)]
struct YtDlpInfo {
    url: Option<String>,
    requested_formats: Option<Vec<YtDlpFormat>>,
    http_headers: Option<BTreeMap<String, String>>,
    channel_id: Option<String>,
    duration: Option<f64>,
}

#[derive(Deserialize)]
struct YtDlpFormat {
    url: Option<String>,
    http_headers: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
struct YtDlpMeta {
    title: Option<String>,
    uploader: Option<String>,
    channel: Option<String>,
    channel_id: Option<String>,
    duration: Option<f64>,
    thumbnail: Option<String>,
}

fn parse_ytdlp_metadata(stdout: &[u8], id: String, kind: MediaKind) -> SourceResult<Track> {
    let m: YtDlpMeta = serde_json::from_slice(stdout)
        .map_err(|e| SourceError::Parse(format!("yt-dlp returned unreadable JSON: {e}")))?;
    let title = m
        .title
        .filter(|t| !t.is_empty())
        .ok_or(SourceError::NotFound)?;
    Ok(Track {
        id,
        kind,
        source: SourceKind::YouTubeMusic,
        title,
        artist: m.channel.or(m.uploader).unwrap_or_default(),
        artist_id: m.channel_id.filter(|c| !c.is_empty()),
        album: None,
        duration_secs: m
            .duration
            .filter(|d| d.is_finite() && *d >= 0.0)
            .map(|d| d.round() as u32),
        thumbnail_url: m.thumbnail,
    })
}

fn parse_ytdlp_json(stdout: &[u8], video: bool) -> SourceResult<Resolved> {
    let info: YtDlpInfo = serde_json::from_slice(stdout)
        .map_err(|e| SourceError::Parse(format!("yt-dlp returned unreadable JSON: {e}")))?;
    let first = info
        .requested_formats
        .and_then(|f| f.into_iter().find(|f| f.url.is_some()));
    let (uri, headers) = match (info.url, first) {
        (Some(url), first) => (
            url,
            info.http_headers
                .or_else(|| first.and_then(|f| f.http_headers)),
        ),
        (None, Some(f)) => (
            f.url.unwrap_or_default(),
            f.http_headers.or(info.http_headers),
        ),
        (None, None) => {
            return Err(SourceError::Extraction(
                "yt-dlp returned no stream URL".to_string(),
            ));
        }
    };
    if !uri.starts_with("https://") && !uri.starts_with("http://") {
        return Err(SourceError::Extraction(
            "yt-dlp returned no stream URL".to_string(),
        ));
    }
    Ok(Resolved {
        playable: Playable::Uri {
            uri,
            video,
            headers: headers.unwrap_or_default().into_iter().collect(),
        },
        artist_id: info.channel_id.filter(|c| !c.is_empty()),
        duration_secs: info
            .duration
            .filter(|d| d.is_finite() && *d >= 0.0)
            .map(|d| d.round().min(f64::from(u32::MAX)) as u32),
    })
}

fn map_ytdlp_failure(stderr: &str) -> SourceError {
    let line = cookies::ytdlp_error_line(stderr, ErrorLinePick::Last);
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("sign in to confirm")
        || lower.contains("confirm your age")
        || lower.contains("age-restricted")
        || lower.contains("inappropriate for some users")
        || lower.contains("members-only")
        || lower.contains("join this channel")
    {
        SourceError::AuthRequired(line)
    } else if lower.contains("video unavailable")
        || lower.contains("private video")
        || lower.contains("this video is not available")
        || lower.contains("has been removed")
        || lower.contains("not available in your country")
    {
        SourceError::Unavailable(line)
    } else {
        SourceError::Extraction(line)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn logged_out_flag_is_read_from_service_tracking_params() {
        let out: Value = serde_json::from_str(r#"{"responseContext":{"serviceTrackingParams":[{"service":"CSI","params":[{"key":"c","value":"WEB_REMIX"}]},{"service":"GFEEDBACK","params":[{"key":"logged_in","value":"0"}]}]}}"#).unwrap();
        let inn: Value = serde_json::from_str(r#"{"responseContext":{"serviceTrackingParams":[{"service":"GFEEDBACK","params":[{"key":"logged_in","value":"1"}]}]}}"#).unwrap();
        assert!(reports_logged_out(&out));
        assert!(!reports_logged_out(&inn));
        assert!(!reports_logged_out(&serde_json::json!({})));
    }

    #[test]
    fn ytdlp_metadata_maps_title_channel_and_duration() {
        let json = br#"{"title":"Never Gonna Give You Up","uploader":"RickAstleyVEVO","channel":"Rick Astley","channel_id":"UCuAXFkgsw1L7xaCfnd5JJOw","duration":212.0,"thumbnail":"https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg"}"#;
        let t = parse_ytdlp_metadata(json, "dQw4w9WgXcQ".into(), MediaKind::Video).unwrap();
        assert_eq!(t.title, "Never Gonna Give You Up");
        assert_eq!(t.artist, "Rick Astley");
        assert_eq!(t.artist_id.as_deref(), Some("UCuAXFkgsw1L7xaCfnd5JJOw"));
        assert_eq!(t.duration_secs, Some(212));
        assert_eq!(
            parse_ytdlp_metadata(b"{}", "x".into(), MediaKind::Video),
            Err(SourceError::NotFound)
        );
    }

    use super::*;

    #[test]
    fn parses_youtube_links() {
        let id = "1uYWYWPc9HU";
        let music = VideoLink {
            id: id.to_string(),
            kind: MediaKind::Music,
        };
        let video = VideoLink {
            id: id.to_string(),
            kind: MediaKind::Video,
        };
        assert_eq!(
            parse_video_url(&format!(
                "https://music.youtube.com/watch?v={id}&list=RDAMVM"
            )),
            Some(music)
        );
        assert_eq!(
            parse_video_url(&format!("https://www.youtube.com/watch?feature=x&v={id}")),
            Some(video.clone())
        );
        assert_eq!(
            parse_video_url(&format!("https://youtu.be/{id}?t=10")),
            Some(video.clone())
        );
        assert_eq!(
            parse_video_url(&format!("https://m.youtube.com/shorts/{id}")),
            Some(video)
        );
        assert_eq!(
            parse_video_url("https://www.youtube.com/watch?v=short"),
            None
        );
        assert_eq!(
            parse_video_url(&format!("https://evil.com/watch?v={id}")),
            None
        );
        assert_eq!(parse_video_url("https://open.spotify.com/track/abc"), None);
    }

    fn run(text: &str) -> serde_json::Value {
        serde_json::json!({ "text": text })
    }

    fn browse_run(text: &str, id: &str, page: &str) -> serde_json::Value {
        serde_json::json!({ "text": text, "navigationEndpoint": { "browseEndpoint": { "browseId": id,
            "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": { "pageType": page } } } } })
    }

    fn watch(id: &str, kind: &str) -> serde_json::Value {
        serde_json::json!({ "watchEndpoint": { "videoId": id,
            "watchEndpointMusicSupportedConfigs": { "watchEndpointMusicConfig": { "musicVideoType": kind } } } })
    }

    fn item(
        title: Vec<serde_json::Value>,
        subtitle: Vec<serde_json::Value>,
        nav: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let mut r = serde_json::json!({
            "thumbnail": { "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [
                { "url": "small", "width": 60, "height": 60 }, { "url": "fit", "width": 120, "height": 120 },
                { "url": "big", "width": 544, "height": 544 } ] } } },
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": title } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": subtitle } } } ] });
        if let Some(nav) = nav {
            r["navigationEndpoint"] = nav;
        }
        serde_json::json!({ "musicResponsiveListItemRenderer": r })
    }

    fn response(sections: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({ "contents": { "tabbedSearchResultsRenderer": { "tabs": [
            { "tabRenderer": { "content": { "sectionListRenderer": { "contents": sections } } } } ] } } })
    }

    #[test]
    fn unfiltered_search_puts_top_card_first_then_groups() {
        let sep = run(" • ");
        let song_title = serde_json::json!({ "text": "Karma Police",
            "navigationEndpoint": watch("nbCOAPR33ME", "MUSIC_VIDEO_TYPE_ATV") });
        let song = item(
            vec![song_title],
            vec![
                browse_run(
                    "Radiohead",
                    "UCr_iyUANcn9OX_yy9piYoLw",
                    "MUSIC_PAGE_TYPE_ARTIST",
                ),
                sep.clone(),
                browse_run("OK Computer", "MPREb_1", "MUSIC_PAGE_TYPE_ALBUM"),
                sep.clone(),
                run("4:22"),
            ],
            None,
        );
        let playlist = item(
            vec![run("Presenting Radiohead")],
            vec![run("YouTube Music"), sep.clone(), run("31 songs")],
            Some(
                serde_json::json!({ "browseEndpoint": { "browseId": "VLRDCLAK5uy_x",
                "browseEndpointContextSupportedConfigs": { "browseEndpointContextMusicConfig": {
                    "pageType": "MUSIC_PAGE_TYPE_PLAYLIST" } } } }),
            ),
        );
        let card = serde_json::json!({ "musicCardShelfRenderer": {
            "title": { "runs": [ { "text": "Karma Police", "navigationEndpoint": watch("1uYWYWPc9HU", "MUSIC_VIDEO_TYPE_OMV") } ] },
            "subtitle": { "runs": [ run("Radiohead"), sep.clone(), run("141M views"), sep.clone(), run("4:23") ] },
            "contents": [ { "messageRenderer": {} } ] } });
        let json = response(vec![
            card,
            serde_json::json!({ "itemSectionRenderer": { "contents": [playlist] } }),
            serde_json::json!({ "itemSectionRenderer": { "contents": [song] } }),
        ]);
        let items = parse_search(&json).expect("parses").into_items(&[]);
        assert_eq!(items.len(), 3);
        let SearchItem::Track(top) = &items[0] else {
            panic!("top card is a track")
        };
        assert_eq!(
            (top.id.as_str(), top.kind, top.duration_secs),
            ("1uYWYWPc9HU", MediaKind::Video, Some(263))
        );
        let SearchItem::Track(song) = &items[1] else {
            panic!("songs precede playlists")
        };
        assert_eq!(song.kind, MediaKind::Music);
        assert_eq!(song.artist, "Radiohead");
        assert_eq!(song.artist_id.as_deref(), Some("UCr_iyUANcn9OX_yy9piYoLw"));
        assert_eq!(song.album.as_deref(), Some("OK Computer"));
        assert_eq!(song.duration_secs, Some(262));
        assert_eq!(song.thumbnail_url.as_deref(), Some("fit"));
        let SearchItem::Collection(pl) = &items[2] else {
            panic!("playlist last")
        };
        assert_eq!(
            (pl.id.as_str(), pl.kind),
            ("RDCLAK5uy_x", CollectionKind::Playlist)
        );
        assert_eq!(pl.subtitle, "Playlist · YouTube Music · 31 songs");
    }

    #[test]
    fn episode_byline_is_the_linked_show() {
        let ep_title = serde_json::json!({ "text": "Thom Yorke interview",
            "navigationEndpoint": watch("uRs2yuQbH1Q", "MUSIC_VIDEO_TYPE_PODCAST_EPISODE") });
        let ep = item(
            vec![ep_title],
            vec![
                run("Apr 27"),
                run(" • "),
                browse_run(
                    "You'll Hear it",
                    "MPSPPL1",
                    "MUSIC_PAGE_TYPE_PODCAST_SHOW_DETAIL_PAGE",
                ),
            ],
            None,
        );
        let json = response(vec![
            serde_json::json!({ "musicShelfRenderer": { "contents": [ep] } }),
        ]);
        let items = parse_search(&json)
            .expect("parses")
            .into_items(&[Group::Episodes]);
        let [SearchItem::Track(t)] = items.as_slice() else {
            panic!("one episode")
        };
        assert_eq!(
            (t.kind, t.artist.as_str(), t.id.as_str()),
            (MediaKind::Episode, "You'll Hear it", "uRs2yuQbH1Q")
        );
        let json = response(vec![]);
        assert!(
            parse_search(&json)
                .expect("parses")
                .into_items(&[])
                .is_empty()
        );
        assert!(matches!(
            parse_search(&serde_json::json!({ "error": { "code": 401 } })),
            Err(SourceError::AuthRequired(_))
        ));
    }

    #[test]
    fn parses_spelled_out_durations() {
        assert_eq!(parse_text_duration("3:45"), Some(225));
        assert_eq!(parse_text_duration("1 hr 5 min"), Some(3900));
        assert_eq!(parse_text_duration("58 min"), Some(3480));
        assert_eq!(parse_text_duration("45 sec"), Some(45));
        assert_eq!(parse_text_duration("Live"), None);
    }

    #[test]
    fn classifies_ytdlp_failures() {
        let e = map_ytdlp_failure("ERROR: [youtube] abc: Sign in to confirm you're not a bot.\n");
        assert!(matches!(e, SourceError::AuthRequired(_)));
        let e =
            map_ytdlp_failure("ERROR: [youtube] abc: Video unavailable. This video is private\n");
        assert!(matches!(&e, SourceError::Unavailable(m) if m.starts_with("Video unavailable")));
        let e = map_ytdlp_failure("ERROR: [youtube] abc: Requested format is not available\n");
        assert_eq!(
            e,
            SourceError::Extraction("Requested format is not available".to_string())
        );
    }

    #[test]
    fn picks_stream_url_and_headers_from_ytdlp_json() {
        let json = br#"{"url":"https://a/1","http_headers":{"User-Agent":"UA"},"channel_id":"UCx","duration":262.6}"#;
        let r = parse_ytdlp_json(json, false).expect("parses");
        assert_eq!(
            r.playable,
            Playable::Uri {
                uri: "https://a/1".into(),
                video: false,
                headers: vec![("User-Agent".into(), "UA".into())]
            }
        );
        assert_eq!(
            (r.artist_id.as_deref(), r.duration_secs),
            (Some("UCx"), Some(263))
        );
        let json = br#"{"requested_formats":[{"url":"https://v/1","http_headers":{"A":"b"}},{"url":"https://a/2"}]}"#;
        let r = parse_ytdlp_json(json, true).expect("parses");
        assert!(
            matches!(&r.playable, Playable::Uri { uri, video: true, .. } if uri == "https://v/1")
        );
        assert!(matches!(
            parse_ytdlp_json(b"{}", false),
            Err(SourceError::Extraction(_))
        ));
    }
}
