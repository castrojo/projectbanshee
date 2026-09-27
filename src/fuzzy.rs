//! Fuzzy ranking for the Search Engine.
//!
//! Every keystroke re-ranks the Local Index (all Tracks seen) synchronously; remote results
//! are merged in when they arrive and ranked with the same scorer so the list never reshuffles
//! arbitrarily. Remote rank is a tiebreak/bonus, not the primary key.

use crate::model::{MediaKind, SearchFilter, SearchItem, Track};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use std::collections::{HashMap, HashSet, VecDeque};

/// Reusable scorer (holds the matcher's scratch buffers).
pub struct Scorer {
    matcher: Matcher,
    buf: Vec<char>,
}

impl Default for Scorer {
    fn default() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
            buf: Vec::new(),
        }
    }
}

impl Scorer {
    /// Fuzzy score of `haystack` against `query` (all whitespace-separated atoms must match).
    /// `None` = no match. Empty query matches everything with score 0.
    pub fn score(&mut self, query: &str, haystack: &str) -> Option<u32> {
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        self.score_pattern(&pattern, haystack)
    }

    fn score_pattern(&mut self, pattern: &Pattern, haystack: &str) -> Option<u32> {
        if pattern.atoms.is_empty() {
            return Some(0);
        }
        let hay = Utf32Str::new(haystack, &mut self.buf);
        pattern.score(hay, &mut self.matcher)
    }
}

/// Bonus for an item's position in a source's own relevance ordering.
fn rank_bonus(rank: usize) -> u32 {
    60u32.saturating_sub((rank as u32) * 6)
}

fn item_haystack(item: &SearchItem) -> String {
    match item {
        SearchItem::Track(t) => t.haystack(),
        SearchItem::Collection(c) => format!("{} {}", c.title, c.subtitle),
    }
}

fn item_key(item: &SearchItem) -> String {
    match item {
        SearchItem::Track(t) => t.key(),
        SearchItem::Collection(c) => c.key(),
    }
}

pub fn track_accepts(filter: SearchFilter, t: &Track) -> bool {
    match filter {
        SearchFilter::All => true,
        SearchFilter::Music => t.kind == MediaKind::Music,
        SearchFilter::Videos => t.kind == MediaKind::Video,
        SearchFilter::Podcasts => t.kind == MediaKind::Episode,
    }
}

pub fn filter_accepts(filter: SearchFilter, item: &SearchItem) -> bool {
    match (filter, item) {
        (SearchFilter::All, _) => true,
        (_, SearchItem::Track(t)) => track_accepts(filter, t),
        (SearchFilter::Podcasts, SearchItem::Collection(c)) => {
            c.kind == crate::model::CollectionKind::Podcast
        }
        (SearchFilter::Music, SearchItem::Collection(c)) => matches!(
            c.kind,
            crate::model::CollectionKind::Album
                | crate::model::CollectionKind::Playlist
                | crate::model::CollectionKind::Artist
        ),
        (SearchFilter::Videos, SearchItem::Collection(_)) => false,
    }
}

/// A ranked result list entry.
#[derive(Debug, Clone)]
pub struct Ranked {
    pub item: SearchItem,
    pub score: u32,
    /// Whether the item fuzzy-matched the query text (remote items may be semantic matches).
    pub matched: bool,
}

/// Rank remote result lists (one per source, each in the source's relevance order) together
/// with local matches. Duplicates (same key) keep the best score. Output is sorted: fuzzy
/// matches first by score, then non-matching remote items by remote rank.
pub fn merge(
    scorer: &mut Scorer,
    query: &str,
    filter: SearchFilter,
    local: &[Track],
    remote: &[Vec<SearchItem>],
    limit: usize,
) -> Vec<Ranked> {
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut best: HashMap<String, Ranked> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut push =
        |key: String, r: Ranked, best: &mut HashMap<String, Ranked>| match best.get_mut(&key) {
            Some(existing) => {
                if (r.matched, r.score) > (existing.matched, existing.score) {
                    *existing = r;
                }
            }
            None => {
                order.push(key.clone());
                best.insert(key, r);
            }
        };

    for list in remote {
        for (rank, item) in list.iter().enumerate() {
            if !filter_accepts(filter, item) {
                continue;
            }
            let fuzzy = scorer.score_pattern(&pattern, &item_haystack(item));
            let bonus = rank_bonus(rank);
            let (matched, score) = match fuzzy {
                Some(s) => (true, s + bonus),
                None => (false, bonus),
            };
            push(
                item_key(item),
                Ranked {
                    item: item.clone(),
                    score,
                    matched,
                },
                &mut best,
            );
        }
    }
    for t in local {
        if !track_accepts(filter, t) {
            continue;
        }
        if let Some(s) = scorer.score_pattern(&pattern, &t.haystack()) {
            let item = SearchItem::Track(t.clone());
            push(
                t.key(),
                Ranked {
                    item,
                    score: s,
                    matched: true,
                },
                &mut best,
            );
        }
    }

    let mut out: Vec<Ranked> = order.into_iter().filter_map(|k| best.remove(&k)).collect();
    // Stable sort keeps first-seen order for ties.
    out.sort_by_key(|r| std::cmp::Reverse((r.matched, r.score)));
    out.truncate(limit);
    out
}

/// Every Track seen this session (search results, library, playlists), bounded FIFO.
pub struct LocalIndex {
    tracks: HashMap<String, Track>,
    order: VecDeque<String>,
    cap: usize,
}

impl LocalIndex {
    pub fn new(cap: usize) -> Self {
        Self {
            tracks: HashMap::new(),
            order: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn insert(&mut self, t: &Track) {
        let key = t.key();
        if self.tracks.insert(key.clone(), t.clone()).is_none() {
            self.order.push_back(key);
            while self.order.len() > self.cap {
                if let Some(old) = self.order.pop_front() {
                    self.tracks.remove(&old);
                }
            }
        }
    }

    pub fn extend<'a>(&mut self, tracks: impl IntoIterator<Item = &'a Track>) {
        for t in tracks {
            self.insert(t);
        }
    }

    /// Best `limit` local matches for `query`, sorted by score.
    pub fn search(
        &self,
        scorer: &mut Scorer,
        query: &str,
        filter: SearchFilter,
        limit: usize,
    ) -> Vec<Track> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
        let mut scored: Vec<(u32, &Track)> = self
            .tracks
            .values()
            .filter(|t| track_accepts(filter, t))
            .filter_map(|t| {
                scorer
                    .score_pattern(&pattern, &t.haystack())
                    .map(|s| (s, t))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
        let mut seen = HashSet::new();
        scored
            .into_iter()
            .filter(|(_, t)| seen.insert(t.key()))
            .take(limit)
            .map(|(_, t)| t.clone())
            .collect()
    }

    pub fn tracks(&self) -> impl Iterator<Item = &Track> {
        self.order.iter().filter_map(|k| self.tracks.get(k))
    }
}
