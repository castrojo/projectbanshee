use banshee::fuzzy::{LocalIndex, Scorer, merge};
use banshee::model::{
    Collection, CollectionKind, MediaKind, SearchFilter, SearchItem, SourceKind, Track,
};

fn t(id: &str, title: &str, artist: &str, kind: MediaKind) -> Track {
    Track {
        id: id.into(),
        kind,
        source: SourceKind::YouTubeMusic,
        title: title.into(),
        artist: artist.into(),
        artist_id: None,
        album: None,
        duration_secs: None,
        thumbnail_url: None,
    }
}

fn titles(r: &[banshee::fuzzy::Ranked]) -> Vec<String> {
    r.iter()
        .map(|x| match &x.item {
            SearchItem::Track(t) => t.title.clone(),
            SearchItem::Collection(c) => c.title.clone(),
        })
        .collect()
}

#[test]
fn typos_by_omission_and_word_order_still_match() {
    let mut s = Scorer::default();
    let hay = "Yesterday The Beatles";
    assert!(s.score("beatls yesterdy", hay).is_some());
    assert!(s.score("YESTERDAY beatles", hay).is_some());
    assert!(s.score("stones", hay).is_none());
}

#[test]
fn exact_title_outranks_scattered_subsequence() {
    let mut s = Scorer::default();
    let exact = s.score("hurt", "Hurt Johnny Cash").unwrap();
    let scattered = s.score("hurt", "Hey Ukulele Rhythm Tune Someone").unwrap();
    assert!(exact > scattered, "{exact} <= {scattered}");
}

#[test]
fn merge_puts_fuzzy_matches_above_semantic_remote_hits_and_dedups() {
    let mut s = Scorer::default();
    let remote = vec![vec![
        // A semantic match YouTube returned first that shares no text with the query.
        SearchItem::Track(t("a", "Lyric Snippet Video", "Uploader", MediaKind::Video)),
        SearchItem::Track(t("b", "Paranoid Android", "Radiohead", MediaKind::Music)),
        SearchItem::Track(t("c", "Paranoid", "Black Sabbath", MediaKind::Music)),
    ]];
    let local = vec![t("b", "Paranoid Android", "Radiohead", MediaKind::Music)];
    let r = merge(
        &mut s,
        "paranoid radiohead",
        SearchFilter::All,
        &local,
        &remote,
        10,
    );
    assert_eq!(
        titles(&r),
        ["Paranoid Android", "Lyric Snippet Video", "Paranoid"]
    );
    assert!(r[0].matched && !r[1].matched);
}

#[test]
fn remote_rank_breaks_ties_between_equal_text_matches() {
    let mut s = Scorer::default();
    let remote = vec![vec![
        SearchItem::Track(t("1", "Intro", "Artist A", MediaKind::Music)),
        SearchItem::Track(t("2", "Intro", "Artist B", MediaKind::Music)),
    ]];
    let r = merge(&mut s, "intro", SearchFilter::All, &[], &remote, 10);
    assert_eq!(
        r.iter()
            .map(|x| match &x.item {
                SearchItem::Track(t) => t.id.clone(),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>(),
        ["1", "2"]
    );
}

#[test]
fn filter_restricts_media_kinds_across_merged_lists() {
    let mut s = Scorer::default();
    let pod = Collection {
        id: "p".into(),
        source: SourceKind::YouTubeMusic,
        kind: CollectionKind::Podcast,
        title: "Jazz Talk".into(),
        subtitle: "Podcast".into(),
        thumbnail_url: None,
    };
    let remote = vec![vec![
        SearchItem::Track(t("s", "Jazz Song", "X", MediaKind::Music)),
        SearchItem::Track(t("v", "Jazz Video", "X", MediaKind::Video)),
        SearchItem::Track(t("e", "Jazz Episode", "X", MediaKind::Episode)),
        SearchItem::Collection(pod),
    ]];
    let r = merge(&mut s, "jazz", SearchFilter::Podcasts, &[], &remote, 10);
    let mut got = titles(&r);
    got.sort();
    assert_eq!(got, ["Jazz Episode", "Jazz Talk"]);
    let r = merge(&mut s, "jazz", SearchFilter::Videos, &[], &remote, 10);
    assert_eq!(titles(&r), ["Jazz Video"]);
}

#[test]
fn local_index_ranks_instantly_and_evicts_oldest_beyond_capacity() {
    let mut idx = LocalIndex::new(3);
    idx.insert(&t("1", "Old Song", "Band", MediaKind::Music));
    idx.insert(&t("2", "Karma Police", "Radiohead", MediaKind::Music));
    idx.insert(&t("3", "Karma Chameleon", "Culture Club", MediaKind::Music));
    idx.insert(&t("4", "Creep", "Radiohead", MediaKind::Music));
    assert_eq!(idx.len(), 3);
    let mut s = Scorer::default();
    assert!(
        idx.search(&mut s, "old song", SearchFilter::All, 5)
            .is_empty()
    );
    let r = idx.search(&mut s, "karma radiohead", SearchFilter::All, 5);
    assert_eq!(
        r.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
        ["Karma Police"]
    );
    assert!(idx.search(&mut s, "  ", SearchFilter::All, 5).is_empty());
}

#[test]
fn ranking_ten_thousand_local_tracks_is_interactive() {
    let mut idx = LocalIndex::new(20_000);
    for i in 0..10_000 {
        idx.insert(&t(
            &i.to_string(),
            &format!("Song number {i}"),
            &format!("Artist {}", i % 97),
            MediaKind::Music,
        ));
    }
    let mut s = Scorer::default();
    let start = std::time::Instant::now();
    let r = idx.search(&mut s, "song 4242 artist", SearchFilter::All, 50);
    let took = start.elapsed();
    assert!(!r.is_empty());
    // One keystroke must re-rank well inside a frame budget even in debug builds.
    assert!(took.as_millis() < 250, "took {took:?}");
}
