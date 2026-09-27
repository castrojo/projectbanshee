//! Live smoke for the YouTube Music source (network, yt-dlp). Run inside the SDK:
//! `build-aux/sdk-run.sh cargo test --test youtube_live -- --ignored --nocapture --test-threads=1`
//! `live_signed_in_library` additionally needs `BANSHEE_LIVE_IMPORT=1`: it imports the first
//! detected Flatpak browser session, reads the library, and signs out again.

use banshee::model::{
    Collection, CollectionKind, MediaKind, Playable, SearchFilter, SearchItem, SourceKind, Track,
};
use banshee::runtime::runtime;
use banshee::sources::youtube::{YouTubeMusicSource, parse_video_url};
use banshee::sources::{AudioSource, cookies};
use std::time::Instant;

const QUERY: &str = "radiohead karma police";

fn init_logging() {
    let _ = env_logger::builder().is_test(true).try_init();
}

fn describe(items: &[SearchItem]) -> String {
    let (mut music, mut video, mut episode, mut collections) = (0, 0, 0, 0);
    for item in items {
        match item {
            SearchItem::Track(t) => match t.kind {
                MediaKind::Music => music += 1,
                MediaKind::Video => video += 1,
                MediaKind::Episode => episode += 1,
            },
            SearchItem::Collection(_) => collections += 1,
        }
    }
    let first: Vec<String> = items
        .iter()
        .take(4)
        .map(|i| match i {
            SearchItem::Track(t) => format!("{:?} '{}' by {}", t.kind, t.title, t.artist),
            SearchItem::Collection(c) => format!("{:?} '{}' ({})", c.kind, c.title, c.subtitle),
        })
        .collect();
    format!(
        "{} items (music {music}, video {video}, episode {episode}, collections {collections}); first: {first:?}",
        items.len()
    )
}

fn first_track(items: &[SearchItem], kind: MediaKind) -> Option<Track> {
    items.iter().find_map(|i| match i {
        SearchItem::Track(t) if t.kind == kind => Some(t.clone()),
        _ => None,
    })
}

/// Fetch the first KiB of the stream with the resolved headers; returns (status, content-type).
async fn range_probe(uri: &str, headers: &[(String, String)]) -> (u16, String) {
    let client = reqwest::Client::new();
    let mut req = client.get(uri).header("Range", "bytes=0-1023");
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let resp = req.send().await.expect("range request");
    let status = resp.status().as_u16();
    let ctype = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    (status, ctype)
}

fn discover(uri: &str) -> String {
    match std::process::Command::new("gst-discoverer-1.0")
        .args(["-t", "20", uri])
        .output()
    {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| {
                let l = l.trim_start();
                l.starts_with("Duration")
                    || l.starts_with("video #")
                    || l.starts_with("audio #")
                    || l.contains("rror")
            })
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" | "),
        Err(e) => format!("gst-discoverer-1.0 unavailable: {e}"),
    }
}

#[test]
#[ignore = "live network"]
fn live_search_and_resolve() {
    init_logging();
    runtime().block_on(async {
        let yt = YouTubeMusicSource::new();
        let t = Instant::now();
        yt.prewarm().await.expect("prewarm");
        println!("prewarm (visitor token): {:?}", t.elapsed());

        let mut music_results = Vec::new();
        let mut video_results = Vec::new();
        for filter in [SearchFilter::All, SearchFilter::Music, SearchFilter::Videos, SearchFilter::Podcasts] {
            let t = Instant::now();
            let items = yt.search(QUERY.to_string(), filter).await.expect("search");
            let cold = t.elapsed();
            let t = Instant::now();
            let again = yt.search(QUERY.to_string(), filter).await.expect("search again");
            println!("{filter:?}: {cold:?}, repeat {:?}: {}", t.elapsed(), describe(&items));
            assert!(!items.is_empty(), "{filter:?} returned nothing");
            assert_eq!(items.len(), again.len());
            match filter {
                SearchFilter::Music => music_results = items,
                SearchFilter::Videos => video_results = items,
                SearchFilter::All => {
                    assert!(first_track(&items, MediaKind::Music).is_some());
                    assert!(first_track(&items, MediaKind::Video).is_some());
                }
                SearchFilter::Podcasts => {
                    assert!(items.iter().all(|i| match i {
                        SearchItem::Track(t) => t.kind == MediaKind::Episode,
                        SearchItem::Collection(c) => c.kind == CollectionKind::Podcast,
                    }));
                }
            }
        }
        let t = Instant::now();
        let other = yt.search("massive attack teardrop".to_string(), SearchFilter::All).await.expect("search");
        println!("warm All (new query): {:?}, {} items", t.elapsed(), other.len());

        for (label, results, kind) in [("song", &music_results, MediaKind::Music), ("video", &video_results, MediaKind::Video)] {
            let track = first_track(results, kind).expect("a track");
            let t = Instant::now();
            let resolved = yt.resolve(track.clone()).await.expect("resolve");
            let took = t.elapsed();
            let Playable::Uri { uri, video, headers } = &resolved.playable else { panic!("expected a URI") };
            let host = url::Url::parse(uri).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
            println!(
                "resolve {label} '{}' ({}): {took:?}, video={video}, host={host}, headers={:?}, artist_id={:?}, duration={:?}",
                track.title,
                track.id,
                headers.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
                resolved.artist_id,
                resolved.duration_secs
            );
            assert!(uri.starts_with("https://"));
            assert_eq!(*video, kind == MediaKind::Video);
            let (status, ctype) = range_probe(uri, headers).await;
            println!("  range GET: HTTP {status}, content-type {ctype}");
            assert!(status == 200 || status == 206);
            println!("  gst-discoverer: {}", discover(uri));
        }
    });
}

#[test]
#[ignore = "live network"]
fn live_collections_and_links() {
    init_logging();
    runtime().block_on(async {
        let yt = YouTubeMusicSource::new();
        let items = yt
            .search(QUERY.to_string(), SearchFilter::All)
            .await
            .expect("search");
        let podcasts = yt
            .search("radiohead".to_string(), SearchFilter::Podcasts)
            .await
            .expect("search");
        let mut wanted = vec![
            CollectionKind::Album,
            CollectionKind::Artist,
            CollectionKind::Playlist,
            CollectionKind::Podcast,
        ];
        for item in items.iter().chain(podcasts.iter()) {
            let SearchItem::Collection(c) = item else {
                continue;
            };
            let Some(pos) = wanted.iter().position(|k| *k == c.kind) else {
                continue;
            };
            wanted.remove(pos);
            let t = Instant::now();
            let tracks = yt.collection(c.clone()).await.expect("collection");
            println!(
                "{:?} '{}' [{}]: {:?}, {} tracks; first: {:?}",
                c.kind,
                c.title,
                c.id,
                t.elapsed(),
                tracks.len(),
                tracks.first().map(|t| (
                    t.kind,
                    &t.title,
                    &t.artist,
                    t.duration_secs,
                    t.thumbnail_url.is_some()
                ))
            );
            assert!(!tracks.is_empty());
        }
        println!("collection kinds not found in results: {wanted:?}");

        let link = parse_video_url("https://youtu.be/1uYWYWPc9HU?si=x").expect("link");
        let t = Instant::now();
        let track = yt.lookup_video(link.id, link.kind).await.expect("lookup");
        println!(
            "lookup_video: {:?}, {:?} '{}' by {} ({:?} s)",
            t.elapsed(),
            track.kind,
            track.title,
            track.artist,
            track.duration_secs
        );
        assert!(!track.title.is_empty() && track.title != track.id);
    });
}

#[test]
#[ignore = "live network"]
fn live_home() {
    // Signed out: an empty private config dir hides any imported jar. glib caches the config
    // dir on first use, so this only takes effect when the test runs first in its process.
    let config = std::env::temp_dir().join(format!("banshee-live-home-{}", std::process::id()));
    // SAFETY: set before this test reads the environment or spawns threads; threads left by
    // earlier tests (idle runtime workers) do not read it concurrently.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &config) };
    init_logging();
    runtime().block_on(async {
        let yt = YouTubeMusicSource::new();
        if yt.is_signed_in() {
            println!("config dir already resolved to an imported jar; run live_home on its own");
            return;
        }
        let t = Instant::now();
        let shelves = yt.home().await.expect("home");
        let took = t.elapsed();
        for s in &shelves {
            println!(
                "{}{}: {}",
                s.title,
                s.strapline
                    .as_deref()
                    .map(|l| format!(" [{l}]"))
                    .unwrap_or_default(),
                describe(&s.items)
            );
        }
        println!("home (signed out): {} shelves in {took:?}", shelves.len());
        assert!(!shelves.is_empty());
        assert!(
            shelves
                .iter()
                .all(|s| !s.items.is_empty() && !s.title.is_empty())
        );

        let collections: Vec<Collection> = shelves
            .iter()
            .flat_map(|s| &s.items)
            .filter_map(|i| match i {
                SearchItem::Collection(c) => Some(c.clone()),
                SearchItem::Track(_) => None,
            })
            .take(2)
            .collect();
        // A song radio has no playlist page: it opens through its watch playlist.
        let radio = Collection {
            id: "RDAMVMdQw4w9WgXcQ".to_string(),
            source: SourceKind::YouTubeMusic,
            kind: CollectionKind::Playlist,
            title: "Song radio".to_string(),
            subtitle: String::new(),
            thumbnail_url: None,
        };
        for c in collections.into_iter().chain([radio]) {
            let t = Instant::now();
            let tracks = yt.collection(c.clone()).await.expect("collection");
            println!(
                "open {:?} '{}' [{}]: {:?}, {} tracks; first: {:?}",
                c.kind,
                c.title,
                c.id,
                t.elapsed(),
                tracks.len(),
                tracks
                    .first()
                    .map(|t| (t.kind, &t.title, &t.artist, t.duration_secs))
            );
            assert!(!tracks.is_empty());
        }
    });
    let _ = std::fs::remove_dir_all(&config);
}

#[test]
#[ignore = "reads local browser profiles"]
fn live_detect_browsers() {
    for p in cookies::detect_browsers() {
        println!("detected: {} -> {}", p.label, p.spec);
    }
}

#[test]
#[ignore = "live network; imports and removes a browser session"]
fn live_signed_in_library() {
    if std::env::var_os("BANSHEE_LIVE_IMPORT").is_none() {
        println!("BANSHEE_LIVE_IMPORT not set; skipping");
        return;
    }
    if cookies::has_jar() {
        println!(
            "an imported jar already exists at {}; not touching it",
            cookies::jar_path().display()
        );
        return;
    }
    init_logging();
    runtime().block_on(async {
        let profile = cookies::detect_browsers().into_iter().next().expect("a Flatpak browser profile");
        let t = Instant::now();
        cookies::import_from_browser(&profile.spec).await.expect("import");
        println!("import from {}: {:?}", profile.label, t.elapsed());
        let yt = YouTubeMusicSource::new();
        let t = Instant::now();
        let signed_in = yt.reload_auth().await.expect("reload_auth");
        println!("reload_auth: {signed_in} in {:?}; is_signed_in={}", t.elapsed(), yt.is_signed_in());
        let t = Instant::now();
        let library = yt.library().await;
        println!("library: {:?}", t.elapsed());
        let result = match library {
            Ok(sections) => {
                for s in &sections {
                    println!(
                        "  {}: {} collections; first: {:?}",
                        s.title,
                        s.collections.len(),
                        s.collections.first().map(|c| (&c.title, &c.subtitle, &c.id))
                    );
                }
                let mut failures = Vec::new();
                let mut opened = 0;
                for s in &sections {
                    // Every playlist (auto playlists have their own page shapes), a few of the rest.
                    let take = if s.title == "Playlists" { s.collections.len() } else { 3 };
                    for c in s.collections.iter().take(take) {
                        let t = Instant::now();
                        match yt.collection(c.clone()).await {
                            Ok(tracks) => {
                                opened += 1;
                                println!(
                                    "  open {:?} '{}': {:?}, {} tracks; first: {:?}",
                                    c.kind,
                                    c.title,
                                    t.elapsed(),
                                    tracks.len(),
                                    tracks.first().map(|t| (t.kind, &t.title, &t.artist, t.duration_secs))
                                );
                            }
                            Err(e) => failures.push(format!("{:?} '{}' [{}]: {e}", c.kind, c.title, c.id)),
                        }
                    }
                }
                println!("opened {opened} collections, {} failed", failures.len());
                let liked = sections.iter().flat_map(|s| &s.collections).find(|c| c.kind == CollectionKind::LikedSongs);
                if let Some(liked) = liked {
                    match yt.collection(liked.clone()).await.map(|t| t.into_iter().next()) {
                        Ok(Some(track)) => {
                            let t = Instant::now();
                            match yt.resolve(track.clone()).await {
                                Ok(r) => println!(
                                    "  signed-in resolve '{}': {:?}, https={}",
                                    track.title,
                                    t.elapsed(),
                                    matches!(&r.playable, Playable::Uri { uri, .. } if uri.starts_with("https://"))
                                ),
                                Err(e) => failures.push(format!("resolve '{}': {e}", track.title)),
                            }
                        }
                        Ok(None) => println!("  Liked Songs is empty"),
                        Err(e) => failures.push(format!("Liked Songs: {e}")),
                    }
                }
                if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
            }
            Err(e) => Err(e.to_string()),
        };
        yt.sign_out().await.expect("sign out");
        println!("signed out; jar exists: {}", cookies::has_jar());
        if let Err(e) = result {
            panic!("library failed: {e}");
        }
    });
}
