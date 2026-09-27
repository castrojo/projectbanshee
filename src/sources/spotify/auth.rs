//! Spotify Browser Sign-in (CONTEXT.md): OAuth PKCE in the real browser with librespot's
//! client id and a loopback redirect, plus the 0600 token file.

use crate::sources::{SourceError, SourceResult};
use librespot_oauth::{OAuthClient, OAuthClientBuilder, OAuthError, OAuthToken};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// librespot's keymaster client id; its registered redirect includes the loopback URI below.
pub(crate) const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
pub(crate) const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
const LISTENER_ADDR: SocketAddr = SocketAddr::V4(std::net::SocketAddrV4::new(
    std::net::Ipv4Addr::LOCALHOST,
    8898,
));

pub(crate) const SCOPES: [&str; 8] = [
    "streaming",
    "user-read-private",
    "user-read-email",
    "playlist-read-private",
    "playlist-read-collaborative",
    "user-library-read",
    "user-follow-read",
    "user-read-playback-position",
];

/// Refresh when the access token has at most this long left.
const REFRESH_MARGIN_SECS: u64 = 60;

const SIGNED_IN_PAGE: &str = concat!(
    "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">",
    "<meta name=\"viewport\" content=\"width=device-width\"><title>Banshee</title>",
    "<style>body{font-family:Cantarell,system-ui,sans-serif;display:flex;align-items:center;",
    "justify-content:center;height:100vh;margin:0;background:#fafafb;color:#222226}",
    "@media(prefers-color-scheme:dark){body{background:#222226;color:#fff}}",
    "h1{font-size:1.4em;font-weight:700}</style></head>",
    "<body><h1>Signed in to Banshee \u{2014} you can close this tab</h1></body></html>"
);

/// Persisted Spotify credentials (`spotify_token.json`, mode 0600).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_unix: u64,
    #[serde(default)]
    pub display_name: Option<String>,
}

impl StoredToken {
    /// Convert a librespot token. Spotify may omit a rotated refresh token; the previous
    /// one then stays valid.
    pub fn from_oauth(
        t: &OAuthToken,
        previous_refresh: Option<&str>,
        display_name: Option<String>,
    ) -> Self {
        let refresh_token = if t.refresh_token.is_empty() {
            previous_refresh.unwrap_or_default().to_string()
        } else {
            t.refresh_token.clone()
        };
        let remaining = t
            .expires_at
            .saturating_duration_since(Instant::now())
            .as_secs();
        Self {
            access_token: t.access_token.clone(),
            refresh_token,
            expires_at_unix: now_unix().saturating_add(remaining),
            display_name,
        }
    }

    pub fn needs_refresh(&self, now_unix: u64) -> bool {
        self.expires_at_unix <= now_unix.saturating_add(REFRESH_MARGIN_SECS)
    }
}

pub(crate) fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Read the token file. A missing file means signed out; a corrupt one is logged and ignored.
pub(crate) fn load(path: &Path) -> Option<StoredToken> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            log::warn!("cannot read {}: {e}", path.display());
            return None;
        }
    };
    match serde_json::from_slice::<StoredToken>(&bytes) {
        Ok(t) if !t.refresh_token.is_empty() => Some(t),
        Ok(_) => {
            log::warn!("{} has no refresh token; ignoring it", path.display());
            None
        }
        Err(e) => {
            log::warn!("cannot parse {}: {e}", path.display());
            None
        }
    }
}

pub(crate) fn save(path: &Path, token: &StoredToken) -> SourceResult<()> {
    let data = serde_json::to_vec_pretty(token).map_err(|e| SourceError::Parse(e.to_string()))?;
    crate::paths::write_private(path, &data)
        .map_err(|e| SourceError::Unavailable(format!("Could not save the Spotify sign-in: {e}")))
}

pub(crate) fn delete(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("cannot remove {}: {e}", path.display()),
    }
}

pub(crate) fn oauth_client(open_in_browser: bool) -> Result<OAuthClient, OAuthError> {
    let builder = OAuthClientBuilder::new(CLIENT_ID, REDIRECT_URI, SCOPES.to_vec())
        .with_custom_message(SIGNED_IN_PAGE);
    if open_in_browser {
        builder.open_in_browser().build()
    } else {
        builder.build()
    }
}

/// Map a failed browser sign-in. `cancelled` is set when `cancel_sign_in` woke the listener.
pub(crate) fn sign_in_error(e: OAuthError, cancelled: bool) -> SourceError {
    if cancelled {
        return SourceError::AuthRequired("Sign-in cancelled".to_string());
    }
    match e {
        OAuthError::AuthCodeNotFound { .. } => {
            SourceError::AuthRequired("Spotify sign-in was declined".to_string())
        }
        OAuthError::AuthCodeListenerBind { .. } => SourceError::Unavailable(
            "Port 8898 is busy; close other Spotify sign-in pages or apps and try again"
                .to_string(),
        ),
        OAuthError::ExchangeCode { e } => {
            SourceError::Network(format!("Could not finish Spotify sign-in: {e}"))
        }
        other => SourceError::AuthRequired(format!("Spotify sign-in failed: {other}")),
    }
}

/// Map a failed token refresh. `invalid_grant` means the refresh token was revoked.
pub(crate) fn refresh_error(e: &OAuthError) -> SourceError {
    let text = e.to_string();
    if text.contains("invalid_grant") || text.contains("invalid_client") {
        SourceError::AuthRequired("Your Spotify sign-in has expired; sign in again".to_string())
    } else {
        SourceError::Network(format!("Could not refresh the Spotify sign-in: {text}"))
    }
}

/// Wake the blocking loopback listener with a code-less redirect so its thread returns.
pub(crate) fn wake_listener() -> std::io::Result<()> {
    let mut stream = TcpStream::connect_timeout(&LISTENER_ADDR, Duration::from_millis(500))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    stream.write_all(
        b"GET /login?error=cancelled HTTP/1.1\r\nHost: 127.0.0.1:8898\r\nConnection: close\r\n\r\n",
    )?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(expires_at_unix: u64) -> StoredToken {
        StoredToken {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at_unix,
            display_name: Some("Jorge".into()),
        }
    }

    #[test]
    fn refresh_is_due_within_sixty_seconds_of_expiry() {
        let now = 1_000_000;
        assert!(!token(now + 3600).needs_refresh(now));
        assert!(!token(now + 61).needs_refresh(now));
        assert!(token(now + 60).needs_refresh(now));
        assert!(token(now).needs_refresh(now));
        assert!(token(0).needs_refresh(now));
        assert!(token(u64::MAX).needs_refresh(u64::MAX));
    }

    #[test]
    fn oauth_token_conversion_keeps_previous_refresh_token() {
        let t = OAuthToken {
            access_token: "new".into(),
            refresh_token: String::new(),
            expires_at: Instant::now() + Duration::from_secs(3600),
            token_type: "Bearer".into(),
            scopes: vec![],
        };
        let s = StoredToken::from_oauth(&t, Some("old-refresh"), None);
        assert_eq!(s.refresh_token, "old-refresh");
        let left = s.expires_at_unix - now_unix();
        assert!((3598..=3600).contains(&left), "{left}");
        assert!(!s.needs_refresh(now_unix()));
    }

    #[test]
    fn token_file_round_trips_and_rejects_garbage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("spotify_token.json");
        assert_eq!(load(&path), None);
        save(&path, &token(42)).expect("save");
        assert_eq!(load(&path), Some(token(42)));
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        std::fs::write(&path, b"{not json").expect("write");
        assert_eq!(load(&path), None);
        delete(&path);
        assert!(!path.exists());
    }

    #[test]
    fn cancelled_and_declined_sign_ins_map_to_auth_required() {
        let declined = OAuthError::AuthCodeNotFound {
            uri: "http://localhost/login?error=access_denied".into(),
        };
        assert_eq!(
            sign_in_error(declined, false),
            SourceError::AuthRequired("Spotify sign-in was declined".into())
        );
        let woke = OAuthError::AuthCodeNotFound {
            uri: "http://localhost/login?error=cancelled".into(),
        };
        assert_eq!(
            sign_in_error(woke, true),
            SourceError::AuthRequired("Sign-in cancelled".into())
        );
        let revoked = OAuthError::ExchangeCode {
            e: "Server returned error response: invalid_grant".into(),
        };
        assert!(matches!(
            refresh_error(&revoked),
            SourceError::AuthRequired(_)
        ));
    }
}
