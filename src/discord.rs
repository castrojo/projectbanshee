//! Discord Rich Presence over Discord's local IPC socket: while Banshee plays, the user's
//! Discord status shows "Listening to" with the track title, artist and artwork.
//!
//! All socket I/O runs on the shared Tokio runtime ([`crate::runtime::runtime`]); the
//! [`Presence`] handle is cheap to call from the GTK main thread and never blocks it.
//!
//! Protocol: every frame is a little-endian `u32` opcode, a little-endian `u32` payload
//! length, then that many bytes of JSON. The client sends HANDSHAKE (`{"v":1,"client_id"}`),
//! Discord answers with a DISPATCH `READY` frame (or CLOSE with an error code), after which
//! `SET_ACTIVITY` commands update the status. Discord rate-limits `SET_ACTIVITY` (about five
//! per 20 s), so updates are coalesced: at most one per [`MIN_SEND_INTERVAL`], always the
//! newest state.

use crate::model::SourceKind;
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, sleep, sleep_until, timeout};

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

/// Minimum spacing between two `SET_ACTIVITY` commands on one connection.
pub const MIN_SEND_INTERVAL: Duration = Duration::from_secs(2);
const BACKOFF_MIN: Duration = Duration::from_secs(5);
const BACKOFF_MAX: Duration = Duration::from_secs(60);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Best-effort budget for clearing the activity before closing the socket.
const CLEAR_TIMEOUT: Duration = Duration::from_secs(1);
/// Frames larger than this are treated as a protocol error instead of being buffered.
const MAX_FRAME_LEN: u32 = 1 << 20;
/// Timestamp jitter (from whole-second positions) below which an activity is not re-sent.
const TIMESTAMP_TOLERANCE_MS: u64 = 2_500;

/// Discord field limits (characters).
const TEXT_MIN: usize = 2;
const TEXT_MAX: usize = 128;
const IMAGE_URL_MAX: usize = 256;
const BUTTON_LABEL_MAX: usize = 32;
const BUTTON_URL_MAX: usize = 512;
/// Padding for one-character strings; not whitespace, so Discord does not trim it away.
const PAD: char = '\u{2800}';

/// Discord's `ActivityType::Listening`.
const ACTIVITY_LISTENING: u8 = 2;

/// What Banshee is playing right now, as shown in the Discord status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// Public `https://` artwork URL; Discord fetches it itself.
    pub artwork_url: Option<String>,
    /// Link to the track on its source, used for the status button.
    pub url: String,
    pub source: SourceKind,
    pub duration_secs: Option<u32>,
    pub position_secs: u64,
    pub playing: bool,
}

/// Connection state of the Rich Presence client, for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresenceStatus {
    /// Disabled by the user.
    Off,
    /// Enabled, but no Discord application (client) ID is configured.
    NoClientId,
    /// No Discord IPC socket accepted the connection; retrying with backoff.
    DiscordNotRunning,
    Connecting,
    Connected {
        user: String,
    },
    /// The client ID is invalid or Discord refused it; not retried until reconfigured.
    Rejected(String),
}

#[derive(Debug, thiserror::Error)]
enum PresenceError {
    #[error("Discord is not running")]
    NotRunning,
    #[error("{0}")]
    Rejected(String),
    #[error("Discord closed the connection: {0}")]
    Closed(String),
    #[error("Discord IPC I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Discord IPC protocol error: {0}")]
    Protocol(String),
}

/// Handle to the Rich Presence client. Dropping it clears the activity and disconnects.
pub struct Presence {
    desired: watch::Sender<Desired>,
    shared: Arc<Shared>,
}

impl Default for Presence {
    fn default() -> Self {
        Self::new()
    }
}

impl Presence {
    /// Start an idle client (status [`PresenceStatus::Off`]).
    pub fn new() -> Self {
        let (desired, rx) = watch::channel(Desired::default());
        let shared = Arc::new(Shared {
            status: Mutex::new(PresenceStatus::Off),
            subscribers: Mutex::new(Vec::new()),
        });
        crate::runtime::runtime().spawn(actor(Arc::clone(&shared), rx));
        Self { desired, shared }
    }

    /// Enable (connecting with `client_id`) or disable the presence. Disabling, or switching
    /// to another client ID, clears the current activity before closing the connection.
    pub fn configure(&self, enabled: bool, client_id: Option<String>) {
        let client_id = client_id
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty());
        self.desired.send_if_modified(move |d| {
            let changed = d.enabled != enabled || d.client_id != client_id;
            d.enabled = enabled;
            d.client_id = client_id;
            changed
        });
    }

    /// Publish the current playback state (`None` clears the activity). The latest call
    /// wins; sends are coalesced to respect Discord's rate limit.
    pub fn update(&self, now: Option<NowPlaying>) {
        self.desired.send_if_modified(move |d| {
            if d.now == now {
                false
            } else {
                d.now = now;
                true
            }
        });
    }

    pub fn status(&self) -> PresenceStatus {
        lock(&self.shared.status).clone()
    }

    /// Receive every status change from now on.
    pub fn subscribe(&self) -> async_channel::Receiver<PresenceStatus> {
        let (tx, rx) = async_channel::unbounded();
        lock(&self.shared.subscribers).push(tx);
        rx
    }
}

/// Build the `activity` object for `SET_ACTIVITY`. `now_unix_ms` anchors the progress bar.
pub fn activity_json(now: &NowPlaying, now_unix_ms: u64) -> Value {
    let mut activity = Map::new();
    activity.insert("type".into(), json!(ACTIVITY_LISTENING));

    if let Some(details) = discord_text(&now.title) {
        activity.insert("details".into(), json!(details));
    }
    let artist = now.artist.trim();
    let state = match (now.playing, artist.is_empty()) {
        (true, false) => Some(format!("by {artist}")),
        (true, true) => None,
        (false, false) => Some(format!("Paused · {artist}")),
        (false, true) => Some("Paused".to_owned()),
    };
    if let Some(state) = state.as_deref().and_then(discord_text) {
        activity.insert("state".into(), json!(state));
    }

    if now.playing {
        let start = now_unix_ms.saturating_sub(now.position_secs.saturating_mul(1000));
        let mut timestamps = Map::new();
        timestamps.insert("start".into(), json!(start));
        if let Some(duration) = now.duration_secs.filter(|&d| d > 0) {
            timestamps.insert(
                "end".into(),
                json!(start.saturating_add(u64::from(duration) * 1000)),
            );
        }
        activity.insert("timestamps".into(), Value::Object(timestamps));
    }

    let mut assets = Map::new();
    if let Some(image) = now
        .artwork_url
        .as_deref()
        .map(str::trim)
        .filter(|u| u.starts_with("https://") && u.chars().count() <= IMAGE_URL_MAX)
    {
        assets.insert("large_image".into(), json!(image));
    }
    let large_text = now
        .album
        .as_deref()
        .and_then(discord_text)
        .or_else(|| discord_text(&now.title));
    if let Some(text) = large_text {
        assets.insert("large_text".into(), json!(text));
    }
    if !assets.is_empty() {
        activity.insert("assets".into(), Value::Object(assets));
    }

    let url = now.url.trim();
    if (url.starts_with("https://") || url.starts_with("http://"))
        && url.chars().count() <= BUTTON_URL_MAX
    {
        let label = match now.source {
            SourceKind::YouTubeMusic => "Open on YouTube Music",
            SourceKind::Spotify => "Open on Spotify",
        };
        debug_assert!(label.chars().count() <= BUTTON_LABEL_MAX);
        activity.insert("buttons".into(), json!([{ "label": label, "url": url }]));
    }

    Value::Object(activity)
}

/// Fit `s` into Discord's 2–128 character text fields; `None` for blank input.
fn discord_text(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut out = if s.chars().count() > TEXT_MAX {
        let mut cut: String = s.chars().take(TEXT_MAX - 1).collect();
        cut.truncate(cut.trim_end().len());
        cut.push('…');
        cut
    } else {
        s.to_owned()
    };
    while out.chars().count() < TEXT_MIN {
        out.push(PAD);
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------
// Background client

#[derive(Debug, Clone, Default, PartialEq)]
struct Desired {
    enabled: bool,
    client_id: Option<String>,
    now: Option<NowPlaying>,
}

struct Shared {
    status: Mutex<PresenceStatus>,
    subscribers: Mutex<Vec<async_channel::Sender<PresenceStatus>>>,
}

impl Shared {
    fn set(&self, status: PresenceStatus) {
        {
            let mut current = lock(&self.status);
            if *current == status {
                return;
            }
            log::debug!("Discord presence: {status:?}");
            *current = status.clone();
        }
        lock(&self.subscribers).retain(|tx| tx.try_send(status.clone()).is_ok());
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The part of [`Desired`] that decides which connection (if any) should exist.
#[derive(Debug, Clone, PartialEq)]
struct Config {
    enabled: bool,
    client_id: Option<String>,
}

impl Config {
    fn of(d: &Desired) -> Self {
        Self {
            enabled: d.enabled,
            client_id: d.client_id.clone(),
        }
    }
}

fn valid_client_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_digit())
}

/// Wait until the connection config differs from `config`. `false` once the handle is gone.
async fn config_changed(rx: &mut watch::Receiver<Desired>, config: &Config) -> bool {
    loop {
        if rx.changed().await.is_err() {
            return false;
        }
        if Config::of(&rx.borrow_and_update()) != *config {
            return true;
        }
    }
}

enum SessionEnd {
    /// The user disabled the presence or changed the client ID.
    Reconfigured,
    /// The [`Presence`] handle was dropped.
    Dropped,
    /// Discord went away; reconnect with backoff.
    Lost,
    Rejected(String),
}

async fn actor(shared: Arc<Shared>, mut rx: watch::Receiver<Desired>) {
    let mut backoff = BACKOFF_MIN;
    loop {
        let config = Config::of(&rx.borrow_and_update());
        let idle = match (&config.enabled, &config.client_id) {
            (false, _) => Some(PresenceStatus::Off),
            (true, None) => Some(PresenceStatus::NoClientId),
            (true, Some(id)) if !valid_client_id(id) => Some(PresenceStatus::Rejected(
                "The Discord application ID must contain only digits".to_owned(),
            )),
            (true, Some(_)) => None,
        };
        let Some(client_id) = config.client_id.clone().filter(|_| idle.is_none()) else {
            shared.set(idle.unwrap_or(PresenceStatus::Off));
            backoff = BACKOFF_MIN;
            if !config_changed(&mut rx, &config).await {
                return;
            }
            continue;
        };

        shared.set(PresenceStatus::Connecting);
        let attempt = tokio::select! {
            result = connect(&client_id) => Some(result),
            alive = config_changed(&mut rx, &config) => {
                if !alive {
                    return;
                }
                None
            }
        };
        let Some(result) = attempt else { continue };

        match result {
            Ok(conn) => {
                backoff = BACKOFF_MIN;
                log::info!("Connected to Discord Rich Presence as {}", conn.user);
                shared.set(PresenceStatus::Connected {
                    user: conn.user.clone(),
                });
                match session(conn, &mut rx, &config).await {
                    SessionEnd::Reconfigured => continue,
                    SessionEnd::Dropped => return,
                    SessionEnd::Lost => shared.set(PresenceStatus::DiscordNotRunning),
                    SessionEnd::Rejected(reason) => {
                        log::warn!("Discord rejected Rich Presence: {reason}");
                        shared.set(PresenceStatus::Rejected(reason));
                        if !config_changed(&mut rx, &config).await {
                            return;
                        }
                        continue;
                    }
                }
            }
            Err(PresenceError::Rejected(reason)) => {
                log::warn!("Discord rejected Rich Presence: {reason}");
                shared.set(PresenceStatus::Rejected(reason));
                if !config_changed(&mut rx, &config).await {
                    return;
                }
                continue;
            }
            Err(PresenceError::NotRunning) => {
                log::debug!("No Discord IPC socket found");
                shared.set(PresenceStatus::DiscordNotRunning);
            }
            Err(e) => {
                log::info!("Discord Rich Presence connection failed: {e}");
                shared.set(PresenceStatus::DiscordNotRunning);
            }
        }

        log::debug!("Retrying Discord in {}s", backoff.as_secs());
        let reconfigured = tokio::select! {
            () = sleep(backoff) => false,
            alive = config_changed(&mut rx, &config) => {
                if !alive {
                    return;
                }
                true
            }
        };
        backoff = if reconfigured {
            BACKOFF_MIN
        } else {
            (backoff * 2).min(BACKOFF_MAX)
        };
    }
}

/// IPC socket paths in preference order: Flatpak Discord, native, Snap, Flatpak Vesktop.
fn socket_candidates() -> Vec<PathBuf> {
    // Same lookup order as Discord itself when XDG_RUNTIME_DIR is unset.
    let base = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .find_map(|key| std::env::var_os(key).filter(|v| !v.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let dirs = [
        base.join("app/com.discordapp.Discord"),
        base.clone(),
        base.join("snap.discord"),
        base.join("app/dev.vencord.Vesktop"),
    ];
    dirs.iter()
        .flat_map(|dir| (0..10).map(move |i| dir.join(format!("discord-ipc-{i}"))))
        .collect()
}

struct Connection {
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
    user: String,
}

async fn connect(client_id: &str) -> Result<Connection, PresenceError> {
    let mut stream = None;
    for path in socket_candidates() {
        match UnixStream::connect(&path).await {
            Ok(s) => {
                log::debug!("Discord IPC socket: {}", path.display());
                stream = Some(s);
                break;
            }
            Err(e) => log::trace!("{}: {e}", path.display()),
        }
    }
    let (mut reader, mut writer) = stream.ok_or(PresenceError::NotRunning)?.into_split();
    let user = timeout(
        HANDSHAKE_TIMEOUT,
        handshake(&mut reader, &mut writer, client_id),
    )
    .await
    .map_err(|_| PresenceError::Protocol("no reply to the handshake".to_owned()))??;
    Ok(Connection {
        reader,
        writer,
        user,
    })
}

async fn handshake<R, W>(
    reader: &mut R,
    writer: &mut W,
    client_id: &str,
) -> Result<String, PresenceError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    write_frame(
        writer,
        OP_HANDSHAKE,
        &json!({ "v": 1, "client_id": client_id }),
    )
    .await?;
    loop {
        let (op, payload) = read_frame(reader).await?;
        match op {
            OP_FRAME if payload["evt"] == "READY" => return Ok(user_name(&payload)),
            OP_FRAME if payload["evt"] == "ERROR" => {
                return Err(PresenceError::Rejected(error_text(&payload["data"])));
            }
            OP_CLOSE => {
                let reason = error_text(&payload);
                return Err(if is_rejection(&payload) {
                    PresenceError::Rejected(reason)
                } else {
                    PresenceError::Closed(reason)
                });
            }
            OP_PING => write_frame(writer, OP_PONG, &payload).await?,
            _ => log::debug!("Ignoring Discord frame (op {op}) before READY"),
        }
    }
}

/// Close codes that retrying cannot fix: invalid client ID, origin, token, version, encoding.
fn is_rejection(close: &Value) -> bool {
    matches!(
        close["code"].as_u64(),
        Some(4000 | 4001 | 4003 | 4004 | 4005)
    )
}

fn error_text(v: &Value) -> String {
    let message = v["message"].as_str().filter(|m| !m.is_empty());
    match (message, v["code"].as_u64()) {
        (Some(m), Some(code)) => format!("{m} ({code})"),
        (Some(m), None) => m.to_owned(),
        (None, Some(code)) => format!("Discord error {code}"),
        (None, None) => "Discord refused the request".to_owned(),
    }
}

fn user_name(ready: &Value) -> String {
    let user = &ready["data"]["user"];
    ["global_name", "username"]
        .iter()
        .find_map(|k| user[*k].as_str().filter(|s| !s.is_empty()))
        .unwrap_or("Discord user")
        .to_owned()
}

async fn session(
    conn: Connection,
    rx: &mut watch::Receiver<Desired>,
    config: &Config,
) -> SessionEnd {
    let Connection {
        reader, mut writer, ..
    } = conn;
    let (tx, mut frames) = mpsc::channel(8);
    let reader_task = tokio::spawn(read_loop(reader, tx));
    let end = drive(&mut writer, &mut frames, rx, config).await;
    reader_task.abort();
    if let Err(e) = writer.shutdown().await {
        log::debug!("Closing the Discord IPC socket: {e}");
    }
    end
}

type FrameResult = Result<(u32, Value), PresenceError>;

async fn read_loop(mut reader: OwnedReadHalf, tx: mpsc::Sender<FrameResult>) {
    loop {
        let frame = read_frame(&mut reader).await;
        let failed = frame.is_err();
        if tx.send(frame).await.is_err() || failed {
            return;
        }
    }
}

async fn drive(
    writer: &mut OwnedWriteHalf,
    frames: &mut mpsc::Receiver<FrameResult>,
    rx: &mut watch::Receiver<Desired>,
    config: &Config,
) -> SessionEnd {
    let pid = std::process::id();
    // A fresh connection shows no activity.
    let mut shown = Value::Null;
    let mut last_sent: Option<Instant> = None;
    let mut dirty = true;
    loop {
        let due = last_sent.map_or_else(Instant::now, |t| t + MIN_SEND_INTERVAL);
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() {
                    clear(writer, pid).await;
                    return SessionEnd::Dropped;
                }
                let same_config = Config::of(&rx.borrow_and_update()) == *config;
                if !same_config {
                    clear(writer, pid).await;
                    return SessionEnd::Reconfigured;
                }
                dirty = true;
            }
            () = sleep_until(due), if dirty => {
                dirty = false;
                let activity = rx
                    .borrow()
                    .now
                    .as_ref()
                    .map_or(Value::Null, |now| activity_json(now, unix_ms()));
                if same_activity(&activity, &shown) {
                    continue;
                }
                if let Err(e) = write_frame(writer, OP_FRAME, &set_activity(pid, &activity)).await {
                    log::info!("Discord connection lost: {e}");
                    return SessionEnd::Lost;
                }
                shown = activity;
                last_sent = Some(Instant::now());
            }
            frame = frames.recv() => match frame {
                None => return SessionEnd::Lost,
                Some(Err(e)) => {
                    log::info!("Discord connection lost: {e}");
                    return SessionEnd::Lost;
                }
                Some(Ok((OP_PING, payload))) => {
                    if let Err(e) = write_frame(writer, OP_PONG, &payload).await {
                        log::info!("Discord connection lost: {e}");
                        return SessionEnd::Lost;
                    }
                }
                Some(Ok((OP_CLOSE, payload))) => {
                    let reason = error_text(&payload);
                    if is_rejection(&payload) {
                        return SessionEnd::Rejected(reason);
                    }
                    log::info!("Discord closed the connection: {reason}");
                    return SessionEnd::Lost;
                }
                Some(Ok((OP_FRAME, payload))) if payload["evt"] == "ERROR" => {
                    log::warn!("Discord rejected the activity: {}", error_text(&payload["data"]));
                }
                Some(Ok((op, _))) => log::trace!("Discord frame op {op}"),
            },
        }
    }
}

/// Best effort: remove the activity before the connection closes.
async fn clear(writer: &mut OwnedWriteHalf, pid: u32) {
    let frame = set_activity(pid, &Value::Null);
    match timeout(CLEAR_TIMEOUT, write_frame(writer, OP_FRAME, &frame)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::debug!("Clearing the Discord activity: {e}"),
        Err(_) => log::debug!("Clearing the Discord activity timed out"),
    }
}

fn set_activity(pid: u32, activity: &Value) -> Value {
    json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": pid, "activity": activity },
        "nonce": nonce(),
    })
}

/// Equal apart from timestamp jitter caused by whole-second playback positions.
fn same_activity(a: &Value, b: &Value) -> bool {
    let (Some(a), Some(b)) = (a.as_object(), b.as_object()) else {
        return a == b;
    };
    let close = |key: &str| {
        let (x, y) = (&a.get("timestamps"), &b.get("timestamps"));
        match (
            x.and_then(|t| t[key].as_u64()),
            y.and_then(|t| t[key].as_u64()),
        ) {
            (Some(x), Some(y)) => x.abs_diff(y) <= TIMESTAMP_TOLERANCE_MS,
            (x, y) => x.is_none() && y.is_none(),
        }
    };
    let rest_equal = a.len() == b.len()
        && a.iter()
            .all(|(k, v)| k == "timestamps" || b.get(k) == Some(v));
    rest_equal
        && a.contains_key("timestamps") == b.contains_key("timestamps")
        && close("start")
        && close("end")
}

fn nonce() -> String {
    let h = hex::encode(rand::random::<[u8; 16]>());
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    op: u32,
    payload: &Value,
) -> Result<(), PresenceError> {
    let body = serde_json::to_vec(payload).map_err(|e| PresenceError::Protocol(e.to_string()))?;
    let len = u32::try_from(body.len())
        .map_err(|_| PresenceError::Protocol("frame too large".to_owned()))?;
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&len.to_le_bytes());
    frame.extend_from_slice(&body);
    w.write_all(&frame).await?;
    w.flush().await?;
    Ok(())
}

async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<(u32, Value), PresenceError> {
    let mut header = [0u8; 8];
    r.read_exact(&mut header).await?;
    let op = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    if len > MAX_FRAME_LEN {
        return Err(PresenceError::Protocol(format!("frame of {len} bytes")));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    let payload = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).map_err(|e| PresenceError::Protocol(e.to_string()))?
    };
    Ok((op, payload))
}
