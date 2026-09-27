use banshee::model::{MediaKind, SourceKind, Track};
use banshee::queue::{Advance, Queue, RepeatMode};
use rand::SeedableRng;
use rand::rngs::StdRng;

fn t(id: &str, kind: MediaKind, source: SourceKind) -> Track {
    Track {
        id: id.into(),
        kind,
        source,
        title: format!("title {id}"),
        artist: "artist".into(),
        artist_id: None,
        album: None,
        duration_secs: Some(100),
        thumbnail_url: None,
    }
}

fn ids(q: &Queue) -> Vec<&str> {
    q.entries().iter().map(|e| e.track.id.as_str()).collect()
}

fn mixed() -> Queue {
    let mut q = Queue::new();
    q.append(t("song", MediaKind::Music, SourceKind::YouTubeMusic));
    q.append(t("video", MediaKind::Video, SourceKind::YouTubeMusic));
    q.append(t("episode", MediaKind::Episode, SourceKind::Spotify));
    q.append(t("sp-song", MediaKind::Music, SourceKind::Spotify));
    q
}

#[test]
fn mixed_media_kinds_play_in_insertion_order() {
    let mut q = mixed();
    let mut played = vec![];
    while let Some(e) = q.advance(Advance::Finished) {
        played.push((e.track.id.clone(), e.track.kind, e.track.source));
    }
    assert_eq!(
        played,
        vec![
            ("song".into(), MediaKind::Music, SourceKind::YouTubeMusic),
            ("video".into(), MediaKind::Video, SourceKind::YouTubeMusic),
            ("episode".into(), MediaKind::Episode, SourceKind::Spotify),
            ("sp-song".into(), MediaKind::Music, SourceKind::Spotify),
        ]
    );
    // End of queue keeps the last entry current, so Previous still works.
    assert_eq!(q.current().unwrap().track.id, "sp-song");
}

#[test]
fn same_track_can_be_queued_twice_as_distinct_entries() {
    let mut q = Queue::new();
    let a = q.append(t("x", MediaKind::Music, SourceKind::YouTubeMusic));
    let b = q.append(t("x", MediaKind::Music, SourceKind::YouTubeMusic));
    assert_ne!(a, b);
    q.remove(a);
    assert_eq!(q.len(), 1);
    assert_eq!(q.entries()[0].id, b);
}

#[test]
fn play_next_inserts_after_current_across_kinds() {
    let mut q = mixed();
    q.jump(1); // video playing
    q.play_next(t(
        "podcast-next",
        MediaKind::Episode,
        SourceKind::YouTubeMusic,
    ));
    assert_eq!(
        ids(&q),
        ["song", "video", "podcast-next", "episode", "sp-song"]
    );
    assert_eq!(q.advance(Advance::User).unwrap().track.id, "podcast-next");
}

#[test]
fn play_next_with_nothing_current_goes_first() {
    let mut q = mixed();
    q.play_next(t("first", MediaKind::Music, SourceKind::Spotify));
    assert_eq!(ids(&q)[0], "first");
}

#[test]
fn moving_entries_keeps_the_current_entry_current() {
    let mut q = mixed();
    q.jump(2); // episode
    assert!(q.move_entry(3, 0));
    assert_eq!(ids(&q), ["sp-song", "song", "video", "episode"]);
    assert_eq!(q.current().unwrap().track.id, "episode");
    assert!(q.move_entry(3, 0));
    assert_eq!(q.current_index(), Some(0));
    assert!(!q.move_entry(9, 0));
}

#[test]
fn removing_before_current_shifts_current_and_removing_current_clears_it() {
    let mut q = mixed();
    q.jump(2);
    let first = q.entries()[0].id;
    q.remove(first);
    assert_eq!(q.current().unwrap().track.id, "episode");
    let cur = q.current().unwrap().id;
    let (idx, entry) = q.remove(cur).unwrap();
    assert_eq!(idx, 1);
    assert!(q.current().is_none());
    // Undo restores the entry in place with the same id.
    q.restore(idx, entry.clone());
    assert_eq!(q.entries()[1], entry);
}

#[test]
fn removing_the_playing_entry_continues_from_its_slot_not_the_top() {
    let mut q = mixed();
    q.jump(1); // video playing
    let cur = q.current().unwrap().id;
    q.remove(cur);
    assert!(q.current().is_none());
    assert_eq!(q.resume_index(), 1);
    assert_eq!(q.peek_next(Advance::User).unwrap().track.id, "episode");
    // Play Next with nothing current goes into the freed slot, ahead of what followed.
    q.play_next(t("urgent", MediaKind::Music, SourceKind::YouTubeMusic));
    assert_eq!(ids(&q), ["song", "urgent", "episode", "sp-song"]);
    assert_eq!(q.advance(Advance::User).unwrap().track.id, "urgent");
    assert_eq!(q.advance(Advance::User).unwrap().track.id, "episode");
}

#[test]
fn removing_the_last_playing_entry_ends_the_queue_unless_repeating() {
    let mut q = mixed();
    q.jump(3);
    let cur = q.current().unwrap().id;
    q.remove(cur);
    assert!(q.peek_next(Advance::User).is_none());
    q.set_repeat(RepeatMode::All);
    assert_eq!(q.peek_next(Advance::User).unwrap().track.id, "song");
}

#[test]
fn repeat_one_replays_on_finish_but_user_next_moves_on() {
    let mut q = mixed();
    q.jump(0);
    q.set_repeat(RepeatMode::One);
    assert_eq!(q.advance(Advance::Finished).unwrap().track.id, "song");
    assert_eq!(q.advance(Advance::User).unwrap().track.id, "video");
}

#[test]
fn repeat_all_wraps_both_directions() {
    let mut q = mixed();
    q.set_repeat(RepeatMode::All);
    q.jump(3);
    assert_eq!(q.peek_next(Advance::Finished).unwrap().track.id, "song");
    assert_eq!(q.advance(Advance::Finished).unwrap().track.id, "song");
    assert_eq!(q.previous().unwrap().track.id, "sp-song");
}

#[test]
fn repeat_off_stops_at_end() {
    let mut q = mixed();
    q.jump(3);
    assert!(q.peek_next(Advance::User).is_none());
    assert!(q.advance(Advance::User).is_none());
}

#[test]
fn shuffle_keeps_current_first_and_restores_order_including_new_entries() {
    let mut q = mixed();
    for i in 0..20 {
        q.append(t(
            &format!("n{i}"),
            MediaKind::Music,
            SourceKind::YouTubeMusic,
        ));
    }
    let before: Vec<String> = ids(&q).iter().map(|s| s.to_string()).collect();
    q.jump(2);
    let mut rng = StdRng::seed_from_u64(7);
    q.set_shuffle(true, &mut rng);
    assert_eq!(q.current_index(), Some(0));
    assert_eq!(q.current().unwrap().track.id, "episode");
    let shuffled: Vec<String> = ids(&q).iter().map(|s| s.to_string()).collect();
    assert_ne!(shuffled, before);
    let mut sorted_a = shuffled.clone();
    sorted_a.sort();
    let mut sorted_b = before.clone();
    sorted_b.sort();
    assert_eq!(sorted_a, sorted_b);

    q.append(t("late", MediaKind::Video, SourceKind::YouTubeMusic));
    let gone = q.entries().iter().find(|e| e.track.id == "n3").unwrap().id;
    q.remove(gone);
    q.set_shuffle(false, &mut rng);

    let mut expected: Vec<String> = before.into_iter().filter(|s| s != "n3").collect();
    expected.push("late".into());
    assert_eq!(
        ids(&q),
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_eq!(q.current().unwrap().track.id, "episode");
}

#[test]
fn clear_empties_and_advance_is_none() {
    let mut q = mixed();
    q.jump(1);
    q.clear();
    assert!(q.is_empty());
    assert!(q.current().is_none());
    assert!(q.advance(Advance::User).is_none());
    assert!(q.previous().is_none());
}
