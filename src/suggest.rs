//! Keep Going (CONTEXT.md, ADR 0016): songs suggested when nothing is Up next. Pure
//! selection; the controller fetches the inputs and never plays a suggestion by itself.

use crate::model::{HomeShelf, SearchItem, Track};
use std::collections::HashSet;

/// Title of the Home shelf whose songs fill Keep Going.
const QUICK_PICKS: &str = "Quick picks";

/// Suggestions in order: YouTube Music's up-next for the last Queue Entry first, then Quick
/// picks, each Track (by [`Track::key`]) once, leaving out everything in `queued` (the keys
/// of the queued Tracks, which include the seed itself), at most `max`.
pub fn keep_going(
    up_next: &[Track],
    quick_picks: &[Track],
    queued: &HashSet<String>,
    max: usize,
) -> Vec<Track> {
    let mut seen = HashSet::new();
    up_next
        .iter()
        .chain(quick_picks)
        .filter(|t| {
            let key = t.key();
            !queued.contains(&key) && seen.insert(key)
        })
        .take(max)
        .cloned()
        .collect()
}

/// The songs of the Home feed's "Quick picks" shelf (collections skipped).
pub fn quick_picks(shelves: &[HomeShelf]) -> Vec<Track> {
    shelves
        .iter()
        .filter(|s| s.title.eq_ignore_ascii_case(QUICK_PICKS))
        .flat_map(|s| &s.items)
        .filter_map(|i| match i {
            SearchItem::Track(t) => Some(t.clone()),
            SearchItem::Collection(_) => None,
        })
        .collect()
}
