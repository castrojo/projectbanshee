use banshee::cache::{JsonCache, Lookup};
use banshee::lru::WeightedLru;
use std::time::{Duration, SystemTime};

const HOUR: Duration = Duration::from_secs(3600);

#[test]
fn entry_is_fresh_within_ttl_then_stale_but_still_readable() {
    let dir = tempfile::tempdir().unwrap();
    let c = JsonCache::new(dir.path()).unwrap();
    let t0 = SystemTime::now();
    c.put_at("youtube", "library", &vec!["a".to_string()], t0)
        .unwrap();
    assert_eq!(
        c.get_at::<Vec<String>>("youtube", "library", HOUR, t0 + HOUR / 2),
        Lookup::Fresh(vec!["a".into()])
    );
    let stale = c.get_at::<Vec<String>>("youtube", "library", HOUR, t0 + 2 * HOUR);
    assert!(stale.needs_fetch());
    assert_eq!(stale.value(), Some(vec!["a".to_string()]));
}

#[test]
fn missing_and_type_mismatch_and_corrupt_read_as_missing_and_corrupt_is_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let c = JsonCache::new(dir.path()).unwrap();
    assert_eq!(c.get::<u32>("youtube", "nope", HOUR), Lookup::Missing);
    c.put("youtube", "k", &"text").unwrap();
    assert_eq!(c.get::<u32>("youtube", "k", HOUR), Lookup::Missing);
    // A torn/corrupt file on disk.
    c.put("spotify", "x", &1u32).unwrap();
    let file = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with("spotify-"))
        .unwrap()
        .path();
    std::fs::write(&file, b"{\"stored_at\": 12, \"val").unwrap();
    assert_eq!(c.get::<u32>("spotify", "x", HOUR), Lookup::Missing);
    assert!(!file.exists());
}

#[test]
fn overwrite_replaces_value_and_refreshes_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let c = JsonCache::new(dir.path()).unwrap();
    let t0 = SystemTime::now();
    c.put_at("youtube", "k", &1u32, t0).unwrap();
    c.put_at("youtube", "k", &2u32, t0 + 3 * HOUR).unwrap();
    assert_eq!(
        c.get_at::<u32>("youtube", "k", HOUR, t0 + 3 * HOUR),
        Lookup::Fresh(2)
    );
}

#[test]
fn purging_a_namespace_leaves_other_sources_intact() {
    let dir = tempfile::tempdir().unwrap();
    let c = JsonCache::new(dir.path()).unwrap();
    c.put("youtube", "a", &1u32).unwrap();
    c.put("youtube", "b", &2u32).unwrap();
    c.put("spotify", "a", &3u32).unwrap();
    assert_eq!(c.purge_namespace("youtube").unwrap(), 2);
    assert_eq!(c.get::<u32>("youtube", "a", HOUR), Lookup::Missing);
    assert_eq!(c.get::<u32>("spotify", "a", HOUR), Lookup::Fresh(3));
}

#[test]
fn prune_removes_only_old_files() {
    let dir = tempfile::tempdir().unwrap();
    let c = JsonCache::new(dir.path()).unwrap();
    c.put("youtube", "a", &1u32).unwrap();
    let now = SystemTime::now();
    assert_eq!(c.prune_older_than(HOUR, now), 0);
    assert_eq!(c.prune_older_than(HOUR, now + 2 * HOUR), 1);
    assert_eq!(c.get::<u32>("youtube", "a", HOUR), Lookup::Missing);
}

#[test]
fn weighted_lru_evicts_least_recently_used_to_stay_in_budget() {
    let mut l = WeightedLru::new(10);
    l.insert("a", 1, 4);
    l.insert("b", 2, 4);
    assert_eq!(l.get(&"a"), Some(&1)); // a is now most recent
    let evicted = l.insert("c", 3, 4);
    assert_eq!(evicted, vec![2]);
    assert!(l.total_weight() <= 10);
    assert!(l.contains(&"a") && l.contains(&"c") && !l.contains(&"b"));
    // Oversized item is rejected rather than flushing everything.
    assert_eq!(l.insert("huge", 9, 11), vec![9]);
    assert_eq!(l.len(), 2);
}

#[test]
fn weighted_lru_ttl_expires_entries() {
    let mut l = WeightedLru::new(10).with_ttl(Duration::from_millis(20));
    l.insert("q", 1, 1);
    assert_eq!(l.get(&"q"), Some(&1));
    std::thread::sleep(Duration::from_millis(40));
    assert_eq!(l.get(&"q"), None);
    assert_eq!(l.total_weight(), 0);
}
