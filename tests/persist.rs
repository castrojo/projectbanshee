use banshee::model::{MediaKind, SearchFilter, SearchItem, SourceKind, Track};
use banshee::persist::{
    Prefs, RECENT_SEARCHES_CAP, SAVED_RESULT_SETS_CAP, SearchHistory, Session, StateStore,
};
use banshee::queue::{Advance, Queue, RepeatMode};
use rand::SeedableRng;

fn t(id: &str, kind: MediaKind, source: SourceKind) -> Track {
    Track {
        id: id.into(),
        kind,
        source,
        title: format!("title {id}"),
        artist: "a".into(),
        artist_id: None,
        album: None,
        duration_secs: Some(10),
        thumbnail_url: None,
    }
}

#[test]
fn queue_and_resume_point_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut q = Queue::new();
    q.append(t("song", MediaKind::Music, SourceKind::YouTubeMusic));
    q.append(t("video", MediaKind::Video, SourceKind::YouTubeMusic));
    q.append(t("ep", MediaKind::Episode, SourceKind::Spotify));
    q.append(t("dup", MediaKind::Music, SourceKind::Spotify));
    q.jump(1);
    q.set_repeat(RepeatMode::All);
    q.set_shuffle(true, &mut rand::rngs::StdRng::seed_from_u64(3));
    let before: Vec<_> = q.entries().to_vec();
    {
        let store = StateStore::new(dir.path()).unwrap();
        store
            .save(
                "session",
                &Session {
                    queue: q,
                    position_secs: 42,
                },
            )
            .unwrap();
    }
    // "Restart": a fresh store reading the same directory.
    let store = StateStore::new(dir.path()).unwrap();
    let mut s: Session = store.load("session");
    assert_eq!(s.position_secs, 42);
    assert_eq!(s.queue.entries(), &before[..]);
    assert_eq!(s.queue.current().unwrap().track.id, "video");
    assert!(s.queue.is_shuffled());
    assert_eq!(s.queue.repeat(), RepeatMode::All);
    // New entries after restart never reuse restored ids, and unshuffle still works.
    let new_id = s
        .queue
        .append(t("late", MediaKind::Music, SourceKind::YouTubeMusic));
    assert!(before.iter().all(|e| e.id != new_id));
    s.queue.set_shuffle(false, &mut rand::rng());
    let ids: Vec<_> = s
        .queue
        .entries()
        .iter()
        .map(|e| e.track.id.as_str())
        .collect();
    assert_eq!(ids, ["song", "video", "ep", "dup", "late"]);
    assert_eq!(s.queue.advance(Advance::User).unwrap().track.id, "ep");
}

#[test]
fn missing_state_is_default_and_corrupt_state_is_kept_aside() {
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::new(dir.path()).unwrap();
    let p: Prefs = store.load("prefs");
    assert_eq!(p.volume, 1.0);
    std::fs::write(dir.path().join("session.json"), b"{\"queue\": [trunc").unwrap();
    let s: Session = store.load("session");
    assert!(s.queue.is_empty());
    assert!(
        dir.path().join("session.json.corrupt").exists(),
        "user data must not be silently deleted"
    );
}

#[test]
fn saved_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = StateStore::new(dir.path()).unwrap();
    store.save("search", &SearchHistory::default()).unwrap();
    let mode = std::fs::metadata(dir.path().join("search.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn recent_searches_dedup_case_insensitively_newest_first_and_cap() {
    let mut h = SearchHistory::default();
    h.remember_query("Karma  Police");
    h.remember_query("creep");
    h.remember_query("karma police");
    assert_eq!(h.recent, ["karma police", "creep"]);
    for i in 0..(RECENT_SEARCHES_CAP + 10) {
        h.remember_query(&format!("q{i}"));
    }
    assert_eq!(h.recent.len(), RECENT_SEARCHES_CAP);
    assert_eq!(h.recent[0], format!("q{}", RECENT_SEARCHES_CAP + 9));
    h.remember_query("   ");
    assert_eq!(h.recent.len(), RECENT_SEARCHES_CAP);
}

#[test]
fn search_results_are_recalled_by_normalised_query_and_replaced_and_evicted() {
    let mut h = SearchHistory::default();
    let a = vec![SearchItem::Track(t(
        "a",
        MediaKind::Music,
        SourceKind::YouTubeMusic,
    ))];
    let b = vec![SearchItem::Track(t(
        "b",
        MediaKind::Music,
        SourceKind::YouTubeMusic,
    ))];
    h.store_results(
        SourceKind::YouTubeMusic,
        SearchFilter::All,
        "Radiohead ",
        &a,
    );
    assert_eq!(
        h.results_for(SourceKind::YouTubeMusic, SearchFilter::All, "  radiohead"),
        Some(a.clone())
    );
    assert_eq!(
        h.results_for(SourceKind::YouTubeMusic, SearchFilter::Music, "radiohead"),
        None
    );
    assert_eq!(
        h.results_for(SourceKind::Spotify, SearchFilter::All, "radiohead"),
        None
    );
    h.store_results(SourceKind::YouTubeMusic, SearchFilter::All, "radiohead", &b);
    assert_eq!(
        h.results_for(SourceKind::YouTubeMusic, SearchFilter::All, "radiohead"),
        Some(b)
    );
    assert_eq!(h.results.len(), 1);
    for i in 0..SAVED_RESULT_SETS_CAP {
        h.store_results(SourceKind::Spotify, SearchFilter::All, &format!("q{i}"), &a);
    }
    assert_eq!(h.results.len(), SAVED_RESULT_SETS_CAP);
    assert_eq!(
        h.results_for(SourceKind::YouTubeMusic, SearchFilter::All, "radiohead"),
        None,
        "oldest set evicted"
    );
    h.purge_source(SourceKind::Spotify);
    assert!(h.results.is_empty());
}
