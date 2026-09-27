//! The Queue: an ordered list of Queue Entries mixing any Media Kind and Audio Source.
//!
//! Pure data structure; the UI and Player Core observe it through the app controller.

use crate::model::Track;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

/// Unique id of one position in the Queue; the same Track may be queued several times.
pub type EntryId = u64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEntry {
    pub id: EntryId,
    pub track: Track,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }
}

/// Why the queue is advancing: playback finished on its own, or the user asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advance {
    /// End of stream (Repeat One replays the same entry).
    Finished,
    /// User pressed Next (always moves on).
    User,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Queue {
    entries: Vec<QueueEntry>,
    current: Option<usize>,
    repeat: RepeatMode,
    /// Entry order before shuffle was enabled; `Some` while shuffled.
    unshuffled: Option<Vec<EntryId>>,
    next_id: EntryId,
    /// When the current entry is removed, the slot it occupied: Next / Play / Play Next
    /// continue from there instead of jumping to the top.
    #[serde(default)]
    removed_slot: Option<usize>,
}

impl Queue {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            ..Default::default()
        }
    }

    pub fn entries(&self) -> &[QueueEntry] {
        &self.entries
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }
    pub fn current(&self) -> Option<&QueueEntry> {
        self.current.and_then(|i| self.entries.get(i))
    }
    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }
    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }
    pub fn is_shuffled(&self) -> bool {
        self.unshuffled.is_some()
    }
    pub fn index_of(&self, id: EntryId) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }

    fn make_entry(&mut self, track: Track) -> QueueEntry {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        QueueEntry { id, track }
    }

    /// Append to the end (the default action everywhere in the app).
    pub fn append(&mut self, track: Track) -> EntryId {
        let e = self.make_entry(track);
        let id = e.id;
        if let Some(orig) = &mut self.unshuffled {
            orig.push(id);
        }
        self.entries.push(e);
        id
    }

    pub fn append_many(&mut self, tracks: impl IntoIterator<Item = Track>) -> Vec<EntryId> {
        tracks.into_iter().map(|t| self.append(t)).collect()
    }

    /// Insert right after the current entry (or at the front when nothing is current).
    pub fn play_next(&mut self, track: Track) -> EntryId {
        let pos = self
            .current
            .map_or(self.removed_slot.unwrap_or(0), |c| c + 1);
        self.insert(pos, track)
    }

    /// Insert at `pos` (clamped). Keeps the current entry current.
    pub fn insert(&mut self, pos: usize, track: Track) -> EntryId {
        let pos = pos.min(self.entries.len());
        let e = self.make_entry(track);
        let id = e.id;
        if let Some(orig) = &mut self.unshuffled {
            // Mirror the insertion relative to the neighbour in the unshuffled order.
            let after = pos.checked_sub(1).map(|p| self.entries[p].id);
            let at = after
                .and_then(|a| orig.iter().position(|x| *x == a).map(|p| p + 1))
                .unwrap_or(0);
            orig.insert(at, id);
        }
        self.entries.insert(pos, e);
        if let Some(c) = self.current {
            if pos <= c {
                self.current = Some(c + 1);
            }
        } else if let Some(slot) = self.removed_slot {
            // Play Next inserts at the slot itself, so the new entry is what comes next.
            if pos < slot {
                self.removed_slot = Some(slot + 1);
            }
        }
        id
    }

    /// Re-insert a previously removed entry (Undo) at its old index, keeping its id.
    pub fn restore(&mut self, pos: usize, entry: QueueEntry) {
        let pos = pos.min(self.entries.len());
        if let Some(orig) = &mut self.unshuffled {
            orig.push(entry.id);
        }
        self.next_id = self.next_id.max(entry.id + 1);
        self.entries.insert(pos, entry);
        if let Some(c) = self.current {
            if pos <= c {
                self.current = Some(c + 1);
            }
        }
    }

    /// Remove an entry; returns its former index and the entry (for Undo).
    pub fn remove(&mut self, id: EntryId) -> Option<(usize, QueueEntry)> {
        let idx = self.index_of(id)?;
        let e = self.entries.remove(idx);
        if let Some(orig) = &mut self.unshuffled {
            orig.retain(|x| *x != id);
        }
        match self.current {
            Some(c) if idx < c => self.current = Some(c - 1),
            Some(c) if idx == c => {
                // Nothing is current; the removed slot is where playback continues.
                self.current = None;
                self.removed_slot = Some(idx);
            }
            _ => {
                if let Some(slot) = self.removed_slot {
                    if idx < slot {
                        self.removed_slot = Some(slot - 1);
                    }
                }
            }
        }
        Some((idx, e))
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
        self.removed_slot = None;
        if let Some(orig) = &mut self.unshuffled {
            orig.clear();
        }
    }

    /// Move entry from `from` to `to` (indices in the current order). Current follows its entry.
    pub fn move_entry(&mut self, from: usize, to: usize) -> bool {
        if from >= self.entries.len() || to >= self.entries.len() || from == to {
            return false;
        }
        let cur_id = self.current().map(|e| e.id);
        let e = self.entries.remove(from);
        self.entries.insert(to, e);
        self.current = cur_id.and_then(|id| self.index_of(id));
        true
    }

    /// Update an entry's Track in place (metadata learned while resolving).
    pub fn update_track(&mut self, id: EntryId, f: impl FnOnce(&mut Track)) -> Option<&QueueEntry> {
        let idx = self.index_of(id)?;
        f(&mut self.entries[idx].track);
        self.entries.get(idx)
    }

    /// Make `index` the current entry.
    pub fn jump(&mut self, index: usize) -> Option<&QueueEntry> {
        if index < self.entries.len() {
            self.current = Some(index);
            self.removed_slot = None;
        }
        self.current()
    }

    fn next_index(&self, why: Advance) -> Option<usize> {
        let n = self.entries.len();
        if n == 0 {
            return None;
        }
        let Some(c) = self.current else {
            return match self.removed_slot {
                Some(slot) if slot < n => Some(slot),
                Some(_) if self.repeat != RepeatMode::Off => Some(0),
                Some(_) => None,
                None => Some(0),
            };
        };
        if why == Advance::Finished && self.repeat == RepeatMode::One {
            return Some(c.min(n - 1));
        }
        if c + 1 < n {
            Some(c + 1)
        } else if self.repeat != RepeatMode::Off {
            Some(0)
        } else {
            None
        }
    }

    /// The entry that `advance(why)` would move to, without moving (for pre-resolving streams).
    pub fn peek_next(&self, why: Advance) -> Option<&QueueEntry> {
        self.next_index(why).and_then(|i| self.entries.get(i))
    }

    /// Move to the next entry. Returns `None` at the end of the queue (current is kept).
    pub fn advance(&mut self, why: Advance) -> Option<&QueueEntry> {
        let i = self.next_index(why)?;
        self.current = Some(i);
        self.removed_slot = None;
        self.entries.get(i)
    }

    /// Move to the previous entry (wraps with Repeat All).
    pub fn previous(&mut self) -> Option<&QueueEntry> {
        let n = self.entries.len();
        if n == 0 {
            return None;
        }
        let i = match self.current {
            None => self
                .removed_slot
                .map_or(0, |s| s.saturating_sub(1))
                .min(n - 1),
            Some(0) if self.repeat == RepeatMode::All => n - 1,
            Some(0) => 0,
            Some(c) => (c - 1).min(n - 1),
        };
        self.current = Some(i);
        self.removed_slot = None;
        self.entries.get(i)
    }

    /// Where playback would continue if nothing is current (for Play).
    pub fn resume_index(&self) -> usize {
        self.current
            .or(self.removed_slot.filter(|s| *s < self.entries.len()))
            .unwrap_or(0)
    }

    /// Enable/disable shuffle. Enabling keeps the current entry first and shuffles the rest;
    /// disabling restores the pre-shuffle order (including entries added meanwhile).
    pub fn set_shuffle<R: rand::Rng + ?Sized>(&mut self, enabled: bool, rng: &mut R) {
        if enabled == self.is_shuffled() {
            return;
        }
        let cur_id = self.current().map(|e| e.id);
        if enabled {
            self.unshuffled = Some(self.entries.iter().map(|e| e.id).collect());
            let mut rest: Vec<QueueEntry> = Vec::with_capacity(self.entries.len());
            let mut first = None;
            for e in self.entries.drain(..) {
                if Some(e.id) == cur_id {
                    first = Some(e);
                } else {
                    rest.push(e);
                }
            }
            rest.shuffle(rng);
            if let Some(f) = first {
                self.entries.push(f);
                self.current = Some(0);
            }
            self.entries.extend(rest);
        } else if let Some(order) = self.unshuffled.take() {
            let mut by_id: std::collections::HashMap<EntryId, QueueEntry> =
                self.entries.drain(..).map(|e| (e.id, e)).collect();
            for id in order {
                if let Some(e) = by_id.remove(&id) {
                    self.entries.push(e);
                }
            }
            // Anything not tracked (should not happen) keeps a stable tail order.
            let mut leftover: Vec<_> = by_id.into_values().collect();
            leftover.sort_by_key(|e| e.id);
            self.entries.extend(leftover);
            self.current = cur_id.and_then(|id| self.index_of(id));
        }
    }
}
