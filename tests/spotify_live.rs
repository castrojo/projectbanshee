//! Live Spotify checks against the real Web API. They need a signed-in account
//! (`spotify_token.json` in Banshee's config dir, created by signing in from the app) and
//! skip themselves otherwise. Run with:
//! `build-aux/sdk-run.sh cargo test --test spotify_live -- --ignored --nocapture`

use banshee::model::{MediaKind, SearchFilter, SearchItem};
use banshee::runtime::runtime;
use banshee::sources::AudioSource;
use banshee::sources::spotify::{SpotifySource, track_from_url};
use std::time::Instant;

fn signed_in_source() -> Option<SpotifySource> {
    let path = banshee::paths::config_dir().join("spotify_token.json");
    if !path.exists() {
        eprintln!(
            "skipping: {} not found (sign in to Spotify from Banshee first)",
            path.display()
        );
        return None;
    }
    let source = SpotifySource::new();
    if !source.is_signed_in() {
        eprintln!(
            "skipping: {} is not a usable Spotify sign-in",
            path.display()
        );
        return None;
    }
    Some(source)
}

fn describe(item: &SearchItem) -> String {
    match item {
        SearchItem::Track(t) => format!(
            "{:?} {} — {} [{}] {}",
            t.kind,
            t.title,
            t.artist,
            t.duration_label(),
            t.id
        ),
        SearchItem::Collection(c) => format!("{:?} {} — {} {}", c.kind, c.title, c.subtitle, c.id),
    }
}

#[test]
#[ignore = "needs a Spotify sign-in and network"]
fn live_search() {
    let Some(source) = signed_in_source() else {
        return;
    };
    println!("signed in as {:?}", source.display_name());
    for (query, filter) in [
        ("daft punk", SearchFilter::All),
        ("get lucky", SearchFilter::Music),
        ("lex fridman", SearchFilter::Podcasts),
    ] {
        // Second round is warm: it should reuse the pooled HTTP/2 connection.
        for round in ["cold", "warm"] {
            let started = Instant::now();
            let items = runtime()
                .block_on(source.search(query.to_string(), filter))
                .expect("search");
            println!(
                "{query:?} {filter:?} {round}: {} results in {:?}",
                items.len(),
                started.elapsed()
            );
            if round == "warm" {
                for item in items.iter().take(12) {
                    println!("  {}", describe(item));
                }
            }
            assert!(!items.is_empty(), "no results for {query}");
        }
    }
    assert!(
        runtime()
            .block_on(source.search("anything".into(), SearchFilter::Videos))
            .expect("videos")
            .is_empty()
    );
}

#[test]
#[ignore = "needs a Spotify sign-in and network"]
fn live_library_and_collections() {
    let Some(source) = signed_in_source() else {
        return;
    };
    let started = Instant::now();
    let sections = runtime().block_on(source.library()).expect("library");
    println!("library in {:?}", started.elapsed());
    for section in &sections {
        println!("{} ({})", section.title, section.collections.len());
        for c in section.collections.iter().take(5) {
            println!("  {:?} {} — {}", c.kind, c.title, c.subtitle);
        }
        if let Some(first) = section.collections.first() {
            match runtime().block_on(source.collection(first.clone())) {
                Ok(tracks) => {
                    println!("  → {} opened: {} tracks", first.title, tracks.len());
                    for t in tracks.iter().take(3) {
                        println!(
                            "     {:?} {} — {} [{}]",
                            t.kind,
                            t.title,
                            t.artist,
                            t.duration_label()
                        );
                    }
                }
                Err(e) => println!("  → {} failed: {e}", first.title),
            }
        }
    }
}

#[test]
#[ignore = "needs a Spotify sign-in and network"]
fn live_lookup_resolve_and_session() {
    let Some(source) = signed_in_source() else {
        return;
    };
    let (kind, id) =
        track_from_url("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=x").expect("url");
    assert_eq!(kind, MediaKind::Music);
    let track = runtime().block_on(source.lookup(kind, id)).expect("lookup");
    println!(
        "lookup: {} — {} ({:?}) [{}]",
        track.title,
        track.artist,
        track.album,
        track.duration_label()
    );
    match runtime().block_on(source.resolve(track)) {
        Ok(resolved) => println!("resolved: {:?}", resolved.playable),
        Err(e) => println!("resolve refused: {e}"),
    }
    match runtime().block_on(source.session()) {
        Ok(session) => println!(
            "session: user {} country {}",
            session.username(),
            session.country()
        ),
        Err(e) => println!("session failed: {e}"),
    }
}
