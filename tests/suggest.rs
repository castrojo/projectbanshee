//! Keep Going: what to suggest when nothing is Up next (ADR 0016).

use banshee::model::{
    Collection, CollectionKind, HomeShelf, MediaKind, SearchItem, SourceKind, Track,
};
use banshee::suggest::{keep_going, quick_picks};
use std::collections::HashSet;

fn song(id: &str) -> Track {
    Track {
        id: id.to_string(),
        kind: MediaKind::Music,
        source: SourceKind::YouTubeMusic,
        title: format!("Song {id}"),
        artist: "Artist".to_string(),
        artist_id: None,
        album: None,
        duration_secs: Some(200),
        thumbnail_url: None,
    }
}

fn ids(tracks: &[Track]) -> Vec<&str> {
    tracks.iter().map(|t| t.id.as_str()).collect()
}

fn queued(tracks: &[&Track]) -> HashSet<String> {
    tracks.iter().map(|t| t.key()).collect()
}

#[test]
fn up_next_comes_first_then_quick_picks_fill_up_to_the_limit() {
    let up = [song("u1"), song("u2")];
    let picks = [song("p1"), song("p2"), song("p3")];
    let got = keep_going(&up, &picks, &HashSet::new(), 4);
    assert_eq!(ids(&got), ["u1", "u2", "p1", "p2"]);
}

#[test]
fn queued_tracks_and_the_seed_are_never_suggested() {
    // YouTube Music's up-next for a song starts with the song itself (the seed, which is
    // the last Queue Entry); anything already queued is left out too.
    let seed = song("seed");
    let earlier = song("u2");
    let up = [seed.clone(), song("u1"), earlier.clone(), song("u3")];
    let picks = [song("p1"), earlier.clone()];
    let got = keep_going(&up, &picks, &queued(&[&seed, &earlier]), 8);
    assert_eq!(ids(&got), ["u1", "u3", "p1"]);
}

#[test]
fn a_track_in_both_lists_is_suggested_once_at_its_first_place() {
    let up = [song("a"), song("b"), song("a")];
    let picks = [song("b"), song("c")];
    let got = keep_going(&up, &picks, &HashSet::new(), 8);
    assert_eq!(ids(&got), ["a", "b", "c"]);
}

#[test]
fn the_same_id_from_another_source_is_a_different_track() {
    let mut spotify = song("x");
    spotify.source = SourceKind::Spotify;
    let got = keep_going(&[song("x")], &[spotify], &queued(&[&song("x")]), 8);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].source, SourceKind::Spotify);
}

#[test]
fn nothing_to_suggest_when_everything_is_queued_or_no_room() {
    let up = [song("a")];
    let picks = [song("b")];
    assert!(keep_going(&up, &picks, &queued(&[&up[0], &picks[0]]), 8).is_empty());
    assert!(keep_going(&up, &picks, &HashSet::new(), 0).is_empty());
}

#[test]
fn quick_picks_are_the_songs_of_the_quick_picks_shelf() {
    let album = SearchItem::Collection(Collection {
        id: "MPRE1".to_string(),
        kind: CollectionKind::Album,
        source: SourceKind::YouTubeMusic,
        title: "Album".to_string(),
        subtitle: String::new(),
        thumbnail_url: None,
    });
    let shelves = [
        HomeShelf {
            title: "Listen again".to_string(),
            strapline: None,
            items: vec![SearchItem::Track(song("l1"))],
        },
        HomeShelf {
            title: "Quick picks".to_string(),
            strapline: Some("Start radio from a song".to_string()),
            items: vec![
                SearchItem::Track(song("q1")),
                album,
                SearchItem::Track(song("q2")),
            ],
        },
    ];
    assert_eq!(ids(&quick_picks(&shelves)), ["q1", "q2"]);
    assert!(quick_picks(&shelves[..1]).is_empty());
}
