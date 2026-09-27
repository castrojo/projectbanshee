//! Audio Source interface (CONTEXT.md) and shared error type.

pub mod cookies;
pub mod spotify;
pub mod youtube;

use crate::model::{
    Collection, LibrarySection, Playable, SearchFilter, SearchItem, SourceKind, Track,
};
use futures::future::BoxFuture;

/// Typed failures every source reports. Displayed to the user as toasts/status pages.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("Sign-in required: {0}")]
    AuthRequired(String),
    #[error("Rate limited by the service; try again in {retry_after_secs} s")]
    RateLimited { retry_after_secs: u64 },
    #[error("Not found")]
    NotFound,
    #[error("Unavailable: {0}")]
    Unavailable(String),
    #[error("Could not extract a stream: {0}")]
    Extraction(String),
    #[error("Unexpected response from the service: {0}")]
    Parse(String),
}

pub type SourceResult<T> = Result<T, SourceError>;

/// Result of resolving a Track right before playback. Metadata discovered while resolving
/// (e.g. the artist channel id from yt-dlp) is fed back into the Track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub playable: Playable,
    pub artist_id: Option<String>,
    pub duration_secs: Option<u32>,
}

/// Pluggable streaming backend. All methods run on the Tokio runtime and must be cheap to
/// call concurrently (sources are shared behind `Arc`).
pub trait AudioSource: Send + Sync {
    fn kind(&self) -> SourceKind;

    /// Whether library calls can succeed right now.
    fn is_signed_in(&self) -> bool;

    /// Structured search. Results are in the service's relevance order.
    fn search(
        &self,
        query: String,
        filter: SearchFilter,
    ) -> BoxFuture<'static, SourceResult<Vec<SearchItem>>>;

    /// Library surfaces (playlists, artists, albums, liked songs, podcasts…).
    fn library(&self) -> BoxFuture<'static, SourceResult<Vec<LibrarySection>>>;

    /// Contents of a playlist / album / artist / podcast, in order.
    fn collection(&self, collection: Collection) -> BoxFuture<'static, SourceResult<Vec<Track>>>;

    /// Produce what Player Core needs to start `track`.
    fn resolve(&self, track: Track) -> BoxFuture<'static, SourceResult<Resolved>>;
}
