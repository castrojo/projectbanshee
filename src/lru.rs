//! Weight-bounded LRU used by the Artwork Store (bytes) and the search memo (entries).

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::time::{Duration, Instant};

struct Slot<V> {
    value: V,
    weight: usize,
    tick: u64,
    inserted: Instant,
}

pub struct WeightedLru<K, V> {
    map: HashMap<K, Slot<V>>,
    by_tick: BTreeMap<u64, K>,
    tick: u64,
    total: usize,
    budget: usize,
    ttl: Option<Duration>,
}

impl<K: Hash + Eq + Clone, V> WeightedLru<K, V> {
    pub fn new(budget: usize) -> Self {
        Self {
            map: HashMap::new(),
            by_tick: BTreeMap::new(),
            tick: 0,
            total: 0,
            budget,
            ttl: None,
        }
    }

    /// Entries older than `ttl` are treated as absent and dropped on access or `evict_expired`.
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    pub fn total_weight(&self) -> usize {
        self.total
    }
    pub fn budget(&self) -> usize {
        self.budget
    }

    fn bump(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        let expired = match (self.ttl, self.map.get(key)) {
            (_, None) => return None,
            (Some(ttl), Some(s)) => s.inserted.elapsed() > ttl,
            (None, Some(_)) => false,
        };
        if expired {
            self.remove(key);
            return None;
        }
        let t = self.bump();
        let slot = self.map.get_mut(key)?;
        self.by_tick.remove(&slot.tick);
        slot.tick = t;
        self.by_tick.insert(t, key.clone());
        Some(&slot.value)
    }

    pub fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Insert, then evict least-recently-used entries until within budget. An item heavier
    /// than the whole budget is not retained. Returns evicted values.
    pub fn insert(&mut self, key: K, value: V, weight: usize) -> Vec<V> {
        let mut evicted = Vec::new();
        if let Some(old) = self.remove(&key) {
            evicted.push(old);
        }
        if weight > self.budget {
            evicted.push(value);
            return evicted;
        }
        let t = self.bump();
        self.by_tick.insert(t, key.clone());
        self.map.insert(
            key,
            Slot {
                value,
                weight,
                tick: t,
                inserted: Instant::now(),
            },
        );
        self.total += weight;
        evicted.extend(self.shrink_to(self.budget));
        evicted
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let slot = self.map.remove(key)?;
        self.by_tick.remove(&slot.tick);
        self.total -= slot.weight;
        Some(slot.value)
    }

    /// Evict LRU entries until total weight <= `target`.
    pub fn shrink_to(&mut self, target: usize) -> Vec<V> {
        let mut out = Vec::new();
        while self.total > target {
            let Some((_, k)) = self.by_tick.pop_first() else {
                break;
            };
            if let Some(slot) = self.map.remove(&k) {
                self.total -= slot.weight;
                out.push(slot.value);
            }
        }
        out
    }

    pub fn evict_expired(&mut self) -> usize {
        let Some(ttl) = self.ttl else { return 0 };
        let dead: Vec<K> = self
            .map
            .iter()
            .filter(|(_, s)| s.inserted.elapsed() > ttl)
            .map(|(k, _)| k.clone())
            .collect();
        let n = dead.len();
        for k in dead {
            self.remove(&k);
        }
        n
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.by_tick.clear();
        self.total = 0;
    }
}
