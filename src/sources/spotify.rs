//! Spotify Audio Source (ADR 0006): librespot for sign-in and the playback Session, the
//! Spotify Web API over one shared `reqwest::Client` for search, library and metadata.

mod api;
mod auth;

pub use api::track_from_url;

use crate::model::{
    Collection, CollectionKind, LibrarySection, MediaKind, Playable, SearchFilter, SearchItem,
    SourceKind, Track,
};
use crate::sources::{AudioSource, Resolved, SourceError, SourceResult};
use api::{
    Album, ApiTrack, Artist, Episode, Me, Page, Parent, Playlist, SearchResponse, Show, TopTracks,
};
use auth::StoredToken;
use futures::FutureExt;
use futures::future::{BoxFuture, ready};
use librespot_core::authentication::Credentials;
use librespot_core::{Session, SessionConfig};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

const API_BASE: &str = "https://api.spotify.com/v1";
/// Web API search `limit` maximum since February 2026 (applies per requested type).
const SEARCH_LIMIT: &str = "10";
const PAGE_LIMIT: &str = "50";
/// Upper bound on items fetched for one library section or collection.
const MAX_ITEMS: usize = 1000;
const DEFAULT_RETRY_AFTER_SECS: u64 = 5;
const PREMIUM_REQUIRED: &str = "Spotify Premium is required for playback";
const NOT_SIGNED_IN: &str = "Sign in to Spotify";
const SIGNED_OUT_MEANWHILE: &str = "Signed out of Spotify";
/// How long to wait for the access point's product info after connecting.
const PRODUCT_INFO_WAIT: Duration = Duration::from_secs(3);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn network(e: impl std::fmt::Display) -> SourceError {
    SourceError::Network(e.to_string())
}

/// Spotify search, library, metadata and the librespot playback Session.
pub struct SpotifySource {
    inner: Arc<Inner>,
}

struct Inner {
    http: Result<reqwest::Client, String>,
    token_path: PathBuf,
    token: Mutex<Option<StoredToken>>,
    /// Serializes refreshes so concurrent requests share one refreshed token.
    refresh_lock: tokio::sync::Mutex<()>,
    session: Mutex<Option<Session>>,
    /// Serializes connects so concurrent callers share one Session.
    connect_lock: tokio::sync::Mutex<()>,
    premium: Mutex<Option<bool>>,
    /// Bumped on every sign-in/sign-out so in-flight work from a previous account is dropped.
    generation: AtomicU64,
    signing_in: AtomicBool,
    sign_in_cancelled: AtomicBool,
}

impl Default for SpotifySource {
    fn default() -> Self {
        Self::new()
    }
}

impl SpotifySource {
    /// Load persisted credentials from `config_dir()/spotify_token.json` (if any). No network.
    pub fn new() -> Self {
        Self::with_token_file(crate::paths::config_dir().join("spotify_token.json"))
    }

    fn with_token_file(token_path: PathBuf) -> Self {
        let token = auth::load(&token_path);
        let http = reqwest::Client::builder()
            .user_agent(concat!("Banshee/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()
            .map_err(|e| {
                log::error!("cannot build the Spotify HTTP client: {e}");
                format!("Could not start the Spotify HTTP client: {e}")
            });
        Self {
            inner: Arc::new(Inner {
                http,
                token_path,
                token: Mutex::new(token),
                refresh_lock: tokio::sync::Mutex::new(()),
                session: Mutex::new(None),
                connect_lock: tokio::sync::Mutex::new(()),
                premium: Mutex::new(None),
                generation: AtomicU64::new(0),
                signing_in: AtomicBool::new(false),
                sign_in_cancelled: AtomicBool::new(false),
            }),
        }
    }

    /// Sign in through the user's browser (OAuth PKCE, loopback redirect). Resolves with the
    /// account's display name once the browser redirect arrives; `cancel_sign_in` aborts it.
    pub fn sign_in(&self) -> BoxFuture<'static, SourceResult<String>> {
        let inner = self.inner.clone();
        async move { inner.sign_in().await }.boxed()
    }

    /// Abort a pending `sign_in`: the waiting future resolves to
    /// `AuthRequired("Sign-in cancelled")`. Returns immediately.
    pub fn cancel_sign_in(&self) {
        self.inner.cancel_sign_in();
    }

    /// Forget the credentials (deletes the token file) and drop the playback Session.
    pub fn sign_out(&self) {
        let inner = &self.inner;
        inner.generation.fetch_add(1, Ordering::SeqCst);
        *lock(&inner.token) = None;
        *lock(&inner.premium) = None;
        auth::delete(&inner.token_path);
        if let Some(session) = lock(&inner.session).take() {
            session.shutdown();
        }
        log::info!("signed out of Spotify");
    }

    /// The signed-in account's display name.
    pub fn display_name(&self) -> Option<String> {
        lock(&self.inner.token)
            .as_ref()
            .and_then(|t| t.display_name.clone())
    }

    /// The librespot Session for playback, connected lazily with the OAuth access token and
    /// reconnected when the previous one became invalid.
    pub fn session(&self) -> BoxFuture<'static, SourceResult<Session>> {
        let inner = self.inner.clone();
        async move { inner.session().await }.boxed()
    }

    /// Metadata for one track or episode (e.g. from a pasted link, see `track_from_url`).
    pub fn lookup(&self, kind: MediaKind, id: String) -> BoxFuture<'static, SourceResult<Track>> {
        let inner = self.inner.clone();
        async move { inner.lookup(kind, &id).await }.boxed()
    }
}

impl AudioSource for SpotifySource {
    fn kind(&self) -> SourceKind {
        SourceKind::Spotify
    }

    fn is_signed_in(&self) -> bool {
        self.inner.is_signed_in()
    }

    fn search(
        &self,
        query: String,
        filter: SearchFilter,
    ) -> BoxFuture<'static, SourceResult<Vec<SearchItem>>> {
        let types = match filter {
            SearchFilter::All => "track,episode,album,playlist,artist,show",
            SearchFilter::Music => "track,album",
            SearchFilter::Podcasts => "show,episode",
            SearchFilter::Videos => return ready(Ok(Vec::new())).boxed(),
        };
        if !self.inner.is_signed_in() {
            return ready(Err(SourceError::AuthRequired(NOT_SIGNED_IN.to_string()))).boxed();
        }
        let query = query.trim().to_string();
        if query.is_empty() {
            return ready(Ok(Vec::new())).boxed();
        }
        let inner = self.inner.clone();
        async move {
            let resp: SearchResponse = inner
                .get(
                    "/search",
                    &[
                        ("q", query.as_str()),
                        ("type", types),
                        ("limit", SEARCH_LIMIT),
                        ("market", "from_token"),
                    ],
                )
                .await?;
            Ok(api::search_items(resp))
        }
        .boxed()
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
        let inner = self.inner.clone();
        async move {
            if track.source != SourceKind::Spotify {
                return Err(SourceError::Unavailable(format!(
                    "{} is not a Spotify track",
                    track.title
                )));
            }
            if !api::is_spotify_id(&track.id) {
                return Err(SourceError::NotFound);
            }
            let prefix = match track.kind {
                MediaKind::Music => "track",
                MediaKind::Episode => "episode",
                MediaKind::Video => return Err(SourceError::NotFound),
            };
            if !inner.is_signed_in() {
                return Err(SourceError::AuthRequired(NOT_SIGNED_IN.to_string()));
            }
            inner.ensure_premium().await?;
            Ok(Resolved {
                playable: Playable::Spotify {
                    uri: format!("spotify:{prefix}:{}", track.id),
                },
                artist_id: track.artist_id,
                duration_secs: track.duration_secs,
            })
        }
        .boxed()
    }
}

/// Clears `signing_in` when the sign-in thread ends.
struct SigningInFlag(Arc<Inner>);

impl Drop for SigningInFlag {
    fn drop(&mut self) {
        self.0.signing_in.store(false, Ordering::SeqCst);
    }
}

/// Wakes the listener if the `sign_in` future is dropped before the browser redirect.
struct AbandonGuard {
    inner: Arc<Inner>,
    armed: bool,
}

impl Drop for AbandonGuard {
    fn drop(&mut self) {
        if self.armed {
            self.inner.cancel_sign_in();
        }
    }
}

impl Inner {
    fn is_signed_in(&self) -> bool {
        lock(&self.token).is_some()
    }

    async fn sign_in(self: Arc<Self>) -> SourceResult<String> {
        if self.signing_in.swap(true, Ordering::SeqCst) {
            return Err(SourceError::Unavailable(
                "A Spotify sign-in is already waiting in your browser".to_string(),
            ));
        }
        self.sign_in_cancelled.store(false, Ordering::SeqCst);
        let flag = SigningInFlag(self.clone());
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("spotify-sign-in".into())
            .spawn(move || {
                let _flag = flag;
                let result = auth::oauth_client(true).and_then(|c| c.get_access_token());
                if tx.send(result).is_err() {
                    log::debug!("Spotify sign-in finished after its caller went away");
                }
            })
            .map_err(|e| {
                SourceError::Unavailable(format!("Could not start Spotify sign-in: {e}"))
            })?;
        let mut guard = AbandonGuard {
            inner: self.clone(),
            armed: true,
        };
        let received = rx.await;
        guard.armed = false;
        let oauth = received
            .map_err(|_| {
                SourceError::Unavailable("Spotify sign-in stopped unexpectedly".to_string())
            })?
            .map_err(|e| auth::sign_in_error(e, self.sign_in_cancelled.load(Ordering::SeqCst)))?;

        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(old) = lock(&self.session).take() {
            old.shutdown();
        }
        *lock(&self.premium) = None;
        let mut token = StoredToken::from_oauth(&oauth, None, None);
        if token.refresh_token.is_empty() {
            return Err(SourceError::AuthRequired(
                "Spotify did not return a refresh token; try again".to_string(),
            ));
        }
        *lock(&self.token) = Some(token.clone());

        let name = match self.get::<Me>("/me", &[]).await {
            Ok(me) => {
                self.remember_product(&me, generation);
                me.name()
            }
            Err(e) => {
                log::warn!("signed in to Spotify but could not read the profile: {e}");
                "Spotify user".to_string()
            }
        };
        if self.generation.load(Ordering::SeqCst) != generation {
            return Err(SourceError::AuthRequired(SIGNED_OUT_MEANWHILE.to_string()));
        }
        // A refresh may have happened during `/me`; persist the newest token.
        if let Some(current) = lock(&self.token).as_mut() {
            current.display_name = Some(name.clone());
            token = current.clone();
        }
        auth::save(&self.token_path, &token)?;
        log::info!("signed in to Spotify as {name}");
        Ok(name)
    }

    fn cancel_sign_in(self: &Arc<Self>) {
        if !self.signing_in.load(Ordering::SeqCst) {
            return;
        }
        self.sign_in_cancelled.store(true, Ordering::SeqCst);
        let inner = self.clone();
        // The listener binds right after the browser is launched; retry briefly in case the
        // cancel arrives first.
        let spawned = std::thread::Builder::new()
            .name("spotify-sign-in-cancel".into())
            .spawn(move || {
                for _ in 0..30 {
                    if !inner.signing_in.load(Ordering::SeqCst) {
                        return;
                    }
                    match auth::wake_listener() {
                        Ok(()) => return,
                        Err(e) => log::debug!("waking the Spotify sign-in listener failed: {e}"),
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                log::warn!("could not wake the Spotify sign-in listener");
            });
        if let Err(e) = spawned {
            log::warn!("cannot cancel Spotify sign-in: {e}");
        }
    }

    /// A valid access token, refreshing it when it expires within 60 s or when `stale` (a
    /// token the API just rejected with 401) is still the current one.
    async fn access_token(&self, stale: Option<&str>) -> SourceResult<String> {
        let fresh = |t: &StoredToken| {
            stale.is_none_or(|s| s != t.access_token) && !t.needs_refresh(auth::now_unix())
        };
        {
            let guard = lock(&self.token);
            let t = guard
                .as_ref()
                .ok_or_else(|| SourceError::AuthRequired(NOT_SIGNED_IN.to_string()))?;
            if fresh(t) {
                return Ok(t.access_token.clone());
            }
        }
        let _refreshing = self.refresh_lock.lock().await;
        let current = {
            let guard = lock(&self.token);
            let t = guard
                .as_ref()
                .ok_or_else(|| SourceError::AuthRequired(NOT_SIGNED_IN.to_string()))?;
            if fresh(t) {
                return Ok(t.access_token.clone());
            }
            t.clone()
        };
        let client =
            auth::oauth_client(false).map_err(|e| SourceError::Unavailable(e.to_string()))?;
        match client.refresh_token_async(&current.refresh_token).await {
            Ok(oauth) => {
                let refreshed = StoredToken::from_oauth(
                    &oauth,
                    Some(&current.refresh_token),
                    current.display_name.clone(),
                );
                {
                    let mut guard = lock(&self.token);
                    match guard.as_ref() {
                        Some(t) if t.refresh_token == current.refresh_token => {
                            *guard = Some(refreshed.clone())
                        }
                        _ => {
                            return Err(SourceError::AuthRequired(
                                SIGNED_OUT_MEANWHILE.to_string(),
                            ));
                        }
                    }
                }
                if let Err(e) = auth::save(&self.token_path, &refreshed) {
                    log::warn!("{e}");
                }
                log::debug!("refreshed the Spotify access token");
                Ok(refreshed.access_token)
            }
            Err(e) => {
                let err = auth::refresh_error(&e);
                log::warn!("Spotify token refresh failed: {e}");
                if matches!(err, SourceError::AuthRequired(_)) {
                    let mut guard = lock(&self.token);
                    if guard
                        .as_ref()
                        .is_some_and(|t| t.refresh_token == current.refresh_token)
                    {
                        *guard = None;
                        auth::delete(&self.token_path);
                    }
                }
                Err(err)
            }
        }
    }

    async fn send(
        &self,
        url: &str,
        query: &[(&str, &str)],
        token: &str,
    ) -> SourceResult<reqwest::Response> {
        let http = self
            .http
            .as_ref()
            .map_err(|e| SourceError::Network(e.clone()))?;
        let mut req = http.get(url).bearer_auth(token);
        if !query.is_empty() {
            req = req.query(query);
        }
        req.send().await.map_err(|e| {
            if e.is_timeout() {
                SourceError::Network("Spotify did not answer in time".to_string())
            } else {
                network(e)
            }
        })
    }

    /// GET a Web API path (or an absolute `next` URL) as JSON, refreshing and retrying once
    /// on 401.
    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> SourceResult<T> {
        let url = if path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{API_BASE}{path}")
        };
        let token = self.access_token(None).await?;
        let mut resp = self.send(&url, query, &token).await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let token = self.access_token(Some(&token)).await?;
            resp = self.send(&url, query, &token).await?;
        }
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let body = resp.bytes().await.map_err(network)?;
        if status.is_success() {
            return serde_json::from_slice(&body)
                .map_err(|e| SourceError::Parse(format!("Spotify {path}: {e}")));
        }
        let message = api::error_message(&body);
        log::debug!(
            "Spotify {path} → HTTP {status}: {}",
            message.as_deref().unwrap_or("")
        );
        Err(match status.as_u16() {
            401 => {
                SourceError::AuthRequired("Spotify rejected the sign-in; sign in again".to_string())
            }
            403 => SourceError::Unavailable(
                message.unwrap_or_else(|| "Spotify refused the request".to_string()),
            ),
            404 => SourceError::NotFound,
            429 => SourceError::RateLimited {
                retry_after_secs: retry_after.unwrap_or(DEFAULT_RETRY_AFTER_SECS),
            },
            s if status.is_server_error() => {
                SourceError::Network(format!("Spotify is having trouble (HTTP {s})"))
            }
            s => SourceError::Unavailable(match message {
                Some(m) => format!("Spotify returned HTTP {s}: {m}"),
                None => format!("Spotify returned HTTP {s}"),
            }),
        })
    }

    /// Follow `next` links up to `MAX_ITEMS`. `wrapper` selects the paging object inside the
    /// response (`/me/following` returns `{ "artists": {…} }`).
    async fn paged(
        &self,
        path: &str,
        query: &[(&str, &str)],
        wrapper: Option<&str>,
    ) -> SourceResult<Vec<Value>> {
        let mut items = Vec::new();
        let mut next = Some(path.to_string());
        let mut first = true;
        while let Some(url) = next.take() {
            let mut body: Value = self.get(&url, if first { query } else { &[] }).await?;
            first = false;
            if let Some(key) = wrapper {
                body = body.get_mut(key).map(Value::take).unwrap_or(Value::Null);
            }
            let page: Page = serde_json::from_value(body)
                .map_err(|e| SourceError::Parse(format!("Spotify {path}: {e}")))?;
            items.extend(page.items);
            if items.len() >= MAX_ITEMS {
                items.truncate(MAX_ITEMS);
                break;
            }
            next = page.next.filter(|n| n.starts_with(API_BASE));
        }
        Ok(items)
    }

    async fn library(&self) -> SourceResult<Vec<LibrarySection>> {
        let limit = [("limit", PAGE_LIMIT)];
        let (playlists, liked, albums, artists, shows) = futures::join!(
            self.paged("/me/playlists", &limit, None),
            self.get::<Page>("/me/tracks", &[("limit", "1")]),
            self.paged("/me/albums", &limit, None),
            self.paged(
                "/me/following",
                &[("type", "artist"), ("limit", PAGE_LIMIT)],
                Some("artists")
            ),
            self.paged("/me/shows", &limit, None),
        );
        let mut sections = Vec::new();
        let mut push = |title: &str, collections: Vec<Collection>| {
            if !collections.is_empty() {
                sections.push(LibrarySection {
                    title: title.to_string(),
                    collections,
                });
            }
        };
        if let Some(items) = optional_section("Playlists", playlists)? {
            push(
                "Playlists",
                api::parse_each::<Playlist>(items)
                    .into_iter()
                    .filter_map(api::playlist_collection)
                    .collect(),
            );
        }
        if let Some(page) = optional_section("Liked Songs", liked)? {
            push("Liked Songs", vec![api::liked_songs_collection(page.total)]);
        }
        if let Some(items) = optional_section("Albums", albums)? {
            let albums = api::parse_each::<Album>(api::unwrap_saved(items, "album"));
            push(
                "Albums",
                albums
                    .into_iter()
                    .filter_map(api::album_collection)
                    .collect(),
            );
        }
        if let Some(items) = optional_section("Artists", artists)? {
            push(
                "Artists",
                api::parse_each::<Artist>(items)
                    .into_iter()
                    .filter_map(api::artist_collection)
                    .collect(),
            );
        }
        if let Some(items) = optional_section("Podcasts", shows)? {
            let shows = api::parse_each::<Show>(api::unwrap_saved(items, "show"));
            push(
                "Podcasts",
                shows.into_iter().filter_map(api::show_collection).collect(),
            );
        }
        Ok(sections)
    }

    async fn collection(&self, c: Collection) -> SourceResult<Vec<Track>> {
        if c.source != SourceKind::Spotify {
            return Err(SourceError::NotFound);
        }
        if c.kind != CollectionKind::LikedSongs && !api::is_spotify_id(&c.id) {
            return Err(SourceError::NotFound);
        }
        let market = ("market", "from_token");
        let limit = ("limit", PAGE_LIMIT);
        match c.kind {
            CollectionKind::LikedSongs => Ok(api::saved_tracks(
                self.paged("/me/tracks", &[limit, market], None).await?,
            )),
            CollectionKind::Playlist => {
                let query = [limit, market, ("additional_types", "track,episode")];
                let items = match self
                    .paged(&format!("/playlists/{}/items", c.id), &query, None)
                    .await
                {
                    // Pre-February-2026 deployments only know `/tracks`.
                    Err(SourceError::NotFound) => {
                        self.paged(&format!("/playlists/{}/tracks", c.id), &query, None)
                            .await
                    }
                    other => other,
                }?;
                Ok(api::saved_tracks(items))
            }
            CollectionKind::Album => {
                let parent = Parent::of(&c);
                let items = self
                    .paged(&format!("/albums/{}/tracks", c.id), &[limit, market], None)
                    .await?;
                Ok(api::parse_each::<ApiTrack>(items)
                    .into_iter()
                    .filter_map(|t| api::track(t, Some(&parent)))
                    .collect())
            }
            CollectionKind::Podcast => {
                let parent = Parent::of(&c);
                let items = self
                    .paged(&format!("/shows/{}/episodes", c.id), &[limit, market], None)
                    .await?;
                Ok(api::parse_each::<Episode>(items)
                    .into_iter()
                    .filter_map(|e| api::episode(e, Some(&parent)))
                    .collect())
            }
            CollectionKind::Artist => self.artist_tracks(&c).await,
        }
    }

    /// Artist top tracks; the endpoint was removed in February 2026, so fall back to a
    /// catalog search for the artist's tracks.
    async fn artist_tracks(&self, c: &Collection) -> SourceResult<Vec<Track>> {
        match self
            .get::<TopTracks>(
                &format!("/artists/{}/top-tracks", c.id),
                &[("market", "from_token")],
            )
            .await
        {
            Ok(top) => {
                return Ok(top
                    .tracks
                    .into_iter()
                    .filter_map(|v| api::playable(v, None))
                    .collect());
            }
            Err(SourceError::NotFound | SourceError::Unavailable(_)) => {
                log::debug!("artist top tracks unavailable; searching for {}", c.title);
            }
            Err(e) => return Err(e),
        }
        let name = c.title.replace('"', "");
        let query = format!("artist:\"{name}\"");
        let page = |offset: &'static str| {
            let query = query.clone();
            async move {
                self.get::<SearchResponse>(
                    "/search",
                    &[
                        ("q", query.as_str()),
                        ("type", "track"),
                        ("limit", SEARCH_LIMIT),
                        ("offset", offset),
                        ("market", "from_token"),
                    ],
                )
                .await
            }
        };
        let (first, second) = futures::join!(page("0"), page("10"));
        let mut tracks: Vec<Track> = Vec::new();
        for resp in [first?, second.unwrap_or_default()] {
            let found = resp.tracks.map(|p| p.items).unwrap_or_default();
            for t in api::parse_each::<ApiTrack>(found)
                .into_iter()
                .filter_map(|t| api::track(t, None))
            {
                if !tracks.iter().any(|seen| seen.id == t.id) {
                    tracks.push(t);
                }
            }
        }
        let by_artist: Vec<Track> = tracks
            .iter()
            .filter(|t| t.artist_id.as_deref() == Some(c.id.as_str()))
            .cloned()
            .collect();
        Ok(if by_artist.is_empty() {
            tracks
        } else {
            by_artist
        })
    }

    async fn lookup(&self, kind: MediaKind, id: &str) -> SourceResult<Track> {
        if !api::is_spotify_id(id) {
            return Err(SourceError::NotFound);
        }
        let market = [("market", "from_token")];
        let unplayable = || {
            SourceError::Unavailable(
                "This Spotify item can't be played in your country".to_string(),
            )
        };
        match kind {
            MediaKind::Music => api::track(
                self.get::<ApiTrack>(&format!("/tracks/{id}"), &market)
                    .await?,
                None,
            )
            .ok_or_else(unplayable),
            MediaKind::Episode => api::episode(
                self.get::<Episode>(&format!("/episodes/{id}"), &market)
                    .await?,
                None,
            )
            .ok_or_else(unplayable),
            MediaKind::Video => Err(SourceError::NotFound),
        }
    }

    fn remember_product(&self, me: &Me, generation: u64) {
        let Some(product) = me.product.as_deref() else {
            return;
        };
        if self.generation.load(Ordering::SeqCst) == generation {
            *lock(&self.premium) = Some(product == "premium");
        }
    }

    /// Premium check from `/v1/me` `product`, or — where the Web API no longer returns it —
    /// from the access point's account `type` attribute on the librespot Session.
    async fn ensure_premium(self: &Arc<Self>) -> SourceResult<()> {
        let known = *lock(&self.premium);
        let premium = match known {
            Some(p) => p,
            None => {
                let generation = self.generation.load(Ordering::SeqCst);
                let me = self.get::<Me>("/me", &[]).await?;
                self.remember_product(&me, generation);
                match me.product {
                    Some(product) => product == "premium",
                    None => match self.account_type().await? {
                        Some(kind) => {
                            let premium = kind == "premium";
                            if self.generation.load(Ordering::SeqCst) == generation {
                                *lock(&self.premium) = Some(premium);
                            }
                            premium
                        }
                        None => {
                            log::warn!(
                                "Spotify account type unknown; assuming playback is allowed"
                            );
                            // Remember it so later resolves don't repeat the /me call and wait.
                            if self.generation.load(Ordering::SeqCst) == generation {
                                *lock(&self.premium) = Some(true);
                            }
                            true
                        }
                    },
                }
            }
        };
        if premium {
            Ok(())
        } else {
            Err(SourceError::Unavailable(PREMIUM_REQUIRED.to_string()))
        }
    }

    async fn account_type(self: &Arc<Self>) -> SourceResult<Option<String>> {
        let session = self.clone().session().await?;
        let deadline = tokio::time::Instant::now() + PRODUCT_INFO_WAIT;
        loop {
            if let Some(kind) = session.get_user_attribute("type") {
                return Ok(Some(kind));
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(None);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn session(self: Arc<Self>) -> SourceResult<Session> {
        let _connecting = self.connect_lock.lock().await;
        if let Some(s) = lock(&self.session).as_ref().filter(|s| !s.is_invalid()) {
            return Ok(s.clone());
        }
        let generation = self.generation.load(Ordering::SeqCst);
        let token = self.access_token(None).await?;
        // Session::new captures the current Tokio handle and connect spawns its I/O tasks,
        // so both run on the shared runtime regardless of the caller's executor.
        let session = crate::runtime::runtime()
            .spawn(async move {
                let session = Session::new(SessionConfig::default(), None);
                session
                    .connect(Credentials::with_access_token(token), false)
                    .await
                    .map(|()| session)
            })
            .await
            .map_err(|e| SourceError::Unavailable(format!("Spotify connection task failed: {e}")))?
            .map_err(session_error)?;
        if self.generation.load(Ordering::SeqCst) != generation {
            session.shutdown();
            return Err(SourceError::AuthRequired(SIGNED_OUT_MEANWHILE.to_string()));
        }
        log::info!("connected the Spotify playback session");
        *lock(&self.session) = Some(session.clone());
        Ok(session)
    }
}

/// A library section that failed with NotFound/Unavailable (e.g. an endpoint Spotify
/// removed) is omitted; auth, rate-limit and network failures fail the whole library.
fn optional_section<T>(title: &str, r: SourceResult<T>) -> SourceResult<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e @ (SourceError::NotFound | SourceError::Unavailable(_))) => {
            log::warn!("Spotify library section {title} skipped: {e}");
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

fn session_error(e: librespot_core::Error) -> SourceError {
    use librespot_core::error::ErrorKind;
    let text = e.to_string();
    if text.contains("Premium account required") {
        return SourceError::Unavailable(PREMIUM_REQUIRED.to_string());
    }
    log::warn!("Spotify session connect failed: {text}");
    match e.kind {
        ErrorKind::PermissionDenied | ErrorKind::Unauthenticated => {
            SourceError::AuthRequired("Spotify rejected the sign-in; sign in again".to_string())
        }
        _ => SourceError::Network(format!("Could not connect to Spotify: {text}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_out() -> (tempfile::TempDir, SpotifySource) {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = SpotifySource::with_token_file(dir.path().join("spotify_token.json"));
        (dir, source)
    }

    fn song() -> Track {
        Track {
            id: "4uLU6hMCjMI75M1A2tKUQC".into(),
            kind: MediaKind::Music,
            source: SourceKind::Spotify,
            title: "Song".into(),
            artist: "Artist".into(),
            artist_id: None,
            album: None,
            duration_secs: Some(213),
            thumbnail_url: None,
        }
    }

    #[test]
    fn signed_out_calls_fail_fast_without_network() {
        let (_dir, source) = signed_out();
        assert!(!source.is_signed_in());
        assert_eq!(source.display_name(), None);
        let rt = crate::runtime::runtime();
        let auth = Err(SourceError::AuthRequired(NOT_SIGNED_IN.to_string()));
        assert_eq!(
            rt.block_on(source.search("daft punk".into(), SearchFilter::All)),
            auth
        );
        assert_eq!(
            rt.block_on(source.search("daft punk".into(), SearchFilter::Videos)),
            Ok(vec![])
        );
        assert!(matches!(
            rt.block_on(source.resolve(song())),
            Err(SourceError::AuthRequired(_))
        ));
        assert!(matches!(
            rt.block_on(source.library()),
            Err(SourceError::AuthRequired(_))
        ));
        assert!(matches!(
            rt.block_on(source.session()),
            Err(SourceError::AuthRequired(_))
        ));
    }

    #[test]
    fn stored_token_signs_in_and_sign_out_removes_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("spotify_token.json");
        let token = StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at_unix: auth::now_unix() + 3600,
            display_name: Some("Jorge".into()),
        };
        auth::save(&path, &token).expect("save");
        let source = SpotifySource::with_token_file(path.clone());
        assert!(source.is_signed_in());
        assert_eq!(source.display_name().as_deref(), Some("Jorge"));
        source.sign_out();
        assert!(!source.is_signed_in());
        assert!(!path.exists());
    }

    #[test]
    fn resolve_rejects_foreign_and_malformed_tracks() {
        let (_dir, source) = signed_out();
        let rt = crate::runtime::runtime();
        let mut yt = song();
        yt.source = SourceKind::YouTubeMusic;
        assert!(matches!(
            rt.block_on(source.resolve(yt)),
            Err(SourceError::Unavailable(_))
        ));
        let mut bad = song();
        bad.id = "../me".into();
        assert_eq!(rt.block_on(source.resolve(bad)), Err(SourceError::NotFound));
    }
}
