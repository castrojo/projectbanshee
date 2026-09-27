//! Discord Rich Presence: the activity payload, and the IPC client against a fake Discord
//! server listening on a Unix socket inside a temporary `XDG_RUNTIME_DIR`.

use banshee::discord::{NowPlaying, Presence, PresenceStatus, activity_json};
use banshee::model::SourceKind;
use serde_json::{Value, json};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::{Duration, Instant};

const NOW_MS: u64 = 1_700_000_000_000;

fn np(title: &str) -> NowPlaying {
    NowPlaying {
        title: title.into(),
        artist: "Artist".into(),
        album: Some("Album".into()),
        artwork_url: Some("https://lh3.googleusercontent.com/cover=w544-h544".into()),
        url: "https://music.youtube.com/watch?v=abc".into(),
        source: SourceKind::YouTubeMusic,
        duration_secs: Some(200),
        position_secs: 30,
        playing: true,
    }
}

// ---------------------------------------------------------------------------------------
// activity_json

#[test]
fn playing_activity_has_progress_timestamps_from_position() {
    let a = activity_json(&np("Song"), NOW_MS);
    assert_eq!(a["type"], 2);
    assert_eq!(a["details"], "Song");
    assert_eq!(a["state"], "by Artist");
    let start = NOW_MS - 30_000;
    assert_eq!(a["timestamps"]["start"], start);
    assert_eq!(a["timestamps"]["end"], start + 200_000);
    assert_eq!(
        a["assets"]["large_image"],
        "https://lh3.googleusercontent.com/cover=w544-h544"
    );
    assert_eq!(a["assets"]["large_text"], "Album");

    let live = NowPlaying {
        duration_secs: None,
        ..np("Stream")
    };
    let a = activity_json(&live, NOW_MS);
    assert_eq!(a["timestamps"]["start"], start);
    assert!(a["timestamps"].get("end").is_none(), "{a}");
}

#[test]
fn paused_activity_has_no_timestamps() {
    let paused = NowPlaying {
        playing: false,
        ..np("Song")
    };
    let a = activity_json(&paused, NOW_MS);
    assert!(a.get("timestamps").is_none(), "{a}");
    assert_eq!(a["state"], "Paused · Artist");
    assert_eq!(a["details"], "Song");
}

#[test]
fn text_fields_fit_discord_limits() {
    let long = "x".repeat(300);
    let t = NowPlaying {
        artist: "y".repeat(300),
        album: Some("z".repeat(300)),
        ..np(&long)
    };
    let a = activity_json(&t, NOW_MS);
    for field in [&a["details"], &a["state"], &a["assets"]["large_text"]] {
        let s = field.as_str().unwrap();
        assert_eq!(s.chars().count(), 128, "{s}");
        assert!(s.ends_with('…'), "{s}");
    }

    // Multi-byte characters are counted as characters, never split.
    let a = activity_json(&np(&"é".repeat(200)), NOW_MS);
    assert_eq!(a["details"].as_str().unwrap().chars().count(), 128);

    // One-character strings are padded to Discord's two-character minimum.
    let a = activity_json(&np("X"), NOW_MS);
    let details = a["details"].as_str().unwrap();
    assert_eq!(details.chars().count(), 2);
    assert!(details.starts_with('X'));

    // Blank titles are omitted instead of sending an invalid empty string; the album
    // falls back to the title for the hover text.
    let blank = NowPlaying {
        album: None,
        ..np("  ")
    };
    let a = activity_json(&blank, NOW_MS);
    assert!(a.get("details").is_none(), "{a}");
    assert!(a["assets"].get("large_text").is_none(), "{a}");
    let no_album = NowPlaying {
        album: None,
        ..np("Song")
    };
    assert_eq!(
        activity_json(&no_album, NOW_MS)["assets"]["large_text"],
        "Song"
    );
}

#[test]
fn artwork_must_be_short_https_url() {
    let with_art = |url: String| NowPlaying {
        artwork_url: Some(url),
        ..np("Song")
    };
    let ok = format!("https://i.scdn.co/{}", "a".repeat(256 - 18));
    assert_eq!(ok.len(), 256);
    assert_eq!(
        activity_json(&with_art(ok.clone()), NOW_MS)["assets"]["large_image"],
        ok
    );

    let too_long = format!("{ok}a");
    let a = activity_json(&with_art(too_long), NOW_MS);
    assert!(a["assets"].get("large_image").is_none(), "{a}");
    assert_eq!(a["assets"]["large_text"], "Album");

    let a = activity_json(&with_art("file:///tmp/cover.jpg".into()), NOW_MS);
    assert!(a["assets"].get("large_image").is_none(), "{a}");
}

#[test]
fn button_label_follows_source() {
    let a = activity_json(&np("Song"), NOW_MS);
    assert_eq!(
        a["buttons"],
        json!([{ "label": "Open on YouTube Music", "url": "https://music.youtube.com/watch?v=abc" }])
    );
    let spotify = NowPlaying {
        source: SourceKind::Spotify,
        url: "https://open.spotify.com/track/xyz".into(),
        ..np("Song")
    };
    let a = activity_json(&spotify, NOW_MS);
    assert_eq!(a["buttons"][0]["label"], "Open on Spotify");
    assert_eq!(a["buttons"][0]["url"], "https://open.spotify.com/track/xyz");

    let no_link = NowPlaying {
        url: String::new(),
        ..np("Song")
    };
    assert!(activity_json(&no_link, NOW_MS).get("buttons").is_none());
}

// ---------------------------------------------------------------------------------------
// Fake Discord IPC server

fn read_frame(s: &mut UnixStream) -> std::io::Result<(u32, Value)> {
    let mut header = [0u8; 8];
    s.read_exact(&mut header)?;
    let op = u32::from_le_bytes(header[..4].try_into().unwrap());
    let len = u32::from_le_bytes(header[4..].try_into().unwrap());
    let mut body = vec![0u8; len as usize];
    s.read_exact(&mut body)?;
    Ok((op, serde_json::from_slice(&body).unwrap()))
}

fn write_frame(s: &mut UnixStream, op: u32, payload: &Value) {
    let body = serde_json::to_vec(payload).unwrap();
    let mut frame = op.to_le_bytes().to_vec();
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    s.write_all(&frame).unwrap();
}

fn accept(listener: &UnixListener, within: Duration) -> Option<UnixStream> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((s, _)) => {
                s.set_nonblocking(false).unwrap();
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                return Some(s);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("accept: {e}"),
        }
    }
    None
}

fn wait_status(p: &Presence, want: impl Fn(&PresenceStatus) -> bool, what: &str) -> PresenceStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = p.status();
        if want(&status) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "waiting for {what}, status is {status:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Accept a connection, check the handshake and answer READY.
fn serve_handshake(listener: &UnixListener, client_id: &str) -> UnixStream {
    let mut s = accept(listener, Duration::from_secs(10)).expect("client connects");
    let (op, hello) = read_frame(&mut s).unwrap();
    assert_eq!(op, 0, "{hello}");
    assert_eq!(hello, json!({ "v": 1, "client_id": client_id }));
    write_frame(
        &mut s,
        1,
        &json!({
            "cmd": "DISPATCH", "evt": "READY", "nonce": null,
            "data": { "v": 1, "user": { "id": "42", "username": "tester", "global_name": "Tester" } }
        }),
    );
    s
}

/// Read the next SET_ACTIVITY command and return its `activity`.
fn next_activity(s: &mut UnixStream) -> Value {
    let (op, cmd) = read_frame(s).unwrap();
    assert_eq!(op, 1, "{cmd}");
    assert_eq!(cmd["cmd"], "SET_ACTIVITY");
    assert_eq!(cmd["args"]["pid"], std::process::id());
    assert!(
        cmd["nonce"].as_str().is_some_and(|n| !n.is_empty()),
        "{cmd}"
    );
    cmd["args"]["activity"].clone()
}

fn assert_silent(s: &mut UnixStream, for_: Duration) {
    s.set_read_timeout(Some(for_)).unwrap();
    match read_frame(s) {
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
        other => panic!("expected no frame, got {other:?}"),
    }
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
}

#[test]
fn presence_client_against_fake_discord() {
    let dir = tempfile::tempdir().unwrap();
    // Only this test reads the environment, so mutating it here is race-free.
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
    let listener = UnixListener::bind(dir.path().join("discord-ipc-0")).unwrap();
    listener.set_nonblocking(true).unwrap();

    let id = "123456789012345678";
    let p = Presence::new();
    assert_eq!(p.status(), PresenceStatus::Off);
    let events = p.subscribe();

    // Handshake carries the client ID; the current track is sent right after READY.
    p.update(Some(np("Song A")));
    p.configure(true, Some(id.into()));
    let mut s = serve_handshake(&listener, id);
    wait_status(
        &p,
        |st| {
            *st == PresenceStatus::Connected {
                user: "Tester".into(),
            }
        },
        "READY",
    );
    let a = next_activity(&mut s);
    let first_sent = Instant::now();
    assert_eq!(a["details"], "Song A");
    assert_eq!(a["state"], "by Artist");
    assert_eq!(
        a["assets"]["large_image"],
        "https://lh3.googleusercontent.com/cover=w544-h544"
    );

    // PING is answered with PONG carrying the same payload.
    write_frame(&mut s, 3, &json!({ "probe": 7 }));
    assert_eq!(read_frame(&mut s).unwrap(), (4, json!({ "probe": 7 })));

    // A burst of updates is coalesced into one frame, 2 s after the previous one, newest wins.
    for title in ["Song B", "Song C", "Song D"] {
        p.update(Some(np(title)));
        std::thread::sleep(Duration::from_millis(50));
    }
    let a = next_activity(&mut s);
    let gap = first_sent.elapsed();
    assert_eq!(a["details"], "Song D");
    assert!(gap >= Duration::from_millis(1900), "sent after {gap:?}");
    assert_silent(&mut s, Duration::from_millis(2500));

    // Discord restarts: the client reconnects after backoff and re-sends the current track.
    drop(s);
    wait_status(
        &p,
        |st| *st == PresenceStatus::DiscordNotRunning,
        "disconnect",
    );
    let mut s = serve_handshake(&listener, id);
    assert_eq!(next_activity(&mut s)["details"], "Song D");
    wait_status(
        &p,
        |st| matches!(st, PresenceStatus::Connected { .. }),
        "reconnect",
    );

    // Pausing drops the progress timestamps.
    p.update(Some(NowPlaying {
        playing: false,
        ..np("Song D")
    }));
    let a = next_activity(&mut s);
    assert_eq!(a["state"], "Paused · Artist");
    assert!(a.get("timestamps").is_none(), "{a}");

    // Disabling clears the activity, then closes the socket.
    p.configure(false, Some(id.into()));
    assert_eq!(next_activity(&mut s), Value::Null);
    let mut rest = Vec::new();
    assert_eq!(
        s.read_to_end(&mut rest).unwrap(),
        0,
        "socket closed after clearing"
    );
    wait_status(&p, |st| *st == PresenceStatus::Off, "Off");

    // CLOSE 4000 (invalid client ID) is surfaced as Rejected and not retried.
    let bad = "1000000000000000000";
    p.configure(true, Some(bad.into()));
    let mut s = accept(&listener, Duration::from_secs(10)).expect("client connects");
    let (op, hello) = read_frame(&mut s).unwrap();
    assert_eq!((op, hello["client_id"].as_str()), (0, Some(bad)));
    write_frame(
        &mut s,
        2,
        &json!({ "code": 4000, "message": "Invalid Client ID" }),
    );
    drop(s);
    let rejected = wait_status(
        &p,
        |st| matches!(st, PresenceStatus::Rejected(_)),
        "Rejected",
    );
    assert_eq!(
        rejected,
        PresenceStatus::Rejected("Invalid Client ID (4000)".into())
    );
    assert!(
        accept(&listener, Duration::from_millis(1500)).is_none(),
        "no retry after 4000"
    );

    // Malformed or missing client IDs never touch the socket.
    p.configure(true, Some("12ab".into()));
    wait_status(
        &p,
        |st| matches!(st, PresenceStatus::Rejected(r) if r.contains("digits")),
        "digits",
    );
    p.configure(true, None);
    wait_status(&p, |st| *st == PresenceStatus::NoClientId, "NoClientId");
    assert!(accept(&listener, Duration::from_millis(300)).is_none());

    let seen: Vec<PresenceStatus> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert_eq!(seen.first(), Some(&PresenceStatus::Connecting), "{seen:?}");
    assert!(
        seen.contains(&PresenceStatus::Connected {
            user: "Tester".into()
        }),
        "{seen:?}"
    );
    assert!(
        seen.contains(&PresenceStatus::DiscordNotRunning),
        "{seen:?}"
    );
    assert!(seen.contains(&PresenceStatus::Off), "{seen:?}");
    assert_eq!(seen.last(), Some(&PresenceStatus::NoClientId), "{seen:?}");
}
