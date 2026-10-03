//! O(1) LRU cache with per-entry TTL.
//!
//! Entries live in a `Vec` arena and are linked into a recency list by
//! index (no `unsafe`, no `Rc`); a `HashMap` maps keys to arena slots and a
//! free list recycles slots. `get`, `put` and eviction are O(1).

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

const NIL: usize = usize::MAX;

#[derive(Debug)]
struct Entry<K, V> {
    key: K,
    value: V,
    expires_at: Instant,
    prev: usize,
    next: usize,
}

/// Least-recently-used cache with a fixed capacity and time-to-live.
#[derive(Debug)]
pub struct LruCache<K, V> {
    map: HashMap<K, usize>,
    slots: Vec<Option<Entry<K, V>>>,
    free: Vec<usize>,
    head: usize,
    tail: usize,
    capacity: usize,
    ttl: Duration,
}

impl<K: Clone + Eq + Hash, V> LruCache<K, V> {
    /// Cache holding at most `capacity` (≥ 1) entries for `ttl` each.
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        LruCache {
            map: HashMap::with_capacity(capacity),
            slots: Vec::with_capacity(capacity),
            free: Vec::new(),
            head: NIL,
            tail: NIL,
            capacity: capacity.max(1),
            ttl,
        }
    }

    /// Number of stored (possibly expired) entries.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// True when the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Returns the value for `key` and marks it most recently used.
    /// Expired entries are removed and reported as missing.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.get_at(key, Instant::now())
    }

    fn get_at(&mut self, key: &K, now: Instant) -> Option<&V> {
        let idx = *self.map.get(key)?;
        if self.entry(idx).expires_at <= now {
            self.remove_slot(idx);
            return None;
        }
        self.detach(idx);
        self.push_front(idx);
        Some(&self.entry(idx).value)
    }

    /// Inserts or replaces `key`, evicting the least recently used entry when full.
    pub fn put(&mut self, key: K, value: V) {
        self.put_at(key, value, Instant::now());
    }

    fn put_at(&mut self, key: K, value: V, now: Instant) {
        if let Some(&idx) = self.map.get(&key) {
            self.remove_slot(idx);
        }
        if self.map.len() >= self.capacity {
            self.remove_slot(self.tail);
        }
        let entry = Entry {
            key: key.clone(),
            value,
            expires_at: now + self.ttl,
            prev: NIL,
            next: NIL,
        };
        let idx = match self.free.pop() {
            Some(i) => {
                self.slots[i] = Some(entry);
                i
            }
            None => {
                self.slots.push(Some(entry));
                self.slots.len() - 1
            }
        };
        self.map.insert(key, idx);
        self.push_front(idx);
    }

    fn entry(&self, idx: usize) -> &Entry<K, V> {
        self.slots[idx].as_ref().expect("linked slot is occupied")
    }

    fn entry_mut(&mut self, idx: usize) -> &mut Entry<K, V> {
        self.slots[idx].as_mut().expect("linked slot is occupied")
    }

    fn detach(&mut self, idx: usize) {
        let (prev, next) = {
            let e = self.entry(idx);
            (e.prev, e.next)
        };
        if prev == NIL {
            self.head = next
        } else {
            self.entry_mut(prev).next = next
        }
        if next == NIL {
            self.tail = prev
        } else {
            self.entry_mut(next).prev = prev
        }
    }

    fn push_front(&mut self, idx: usize) {
        let old_head = self.head;
        {
            let e = self.entry_mut(idx);
            e.prev = NIL;
            e.next = old_head;
        }
        if old_head != NIL {
            self.entry_mut(old_head).prev = idx;
        }
        self.head = idx;
        if self.tail == NIL {
            self.tail = idx;
        }
    }

    fn remove_slot(&mut self, idx: usize) {
        self.detach(idx);
        if let Some(entry) = self.slots[idx].take() {
            self.map.remove(&entry.key);
        }
        self.free.push(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used() {
        let mut c = LruCache::new(2, Duration::from_secs(60));
        c.put("a", 1);
        c.put("b", 2);
        assert_eq!(c.get(&"a"), Some(&1));
        c.put("c", 3);
        assert_eq!(c.get(&"b"), None, "b was least recently used");
        assert_eq!(c.get(&"a"), Some(&1));
        assert_eq!(c.get(&"c"), Some(&3));
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn replaces_existing_keys() {
        let mut c = LruCache::new(2, Duration::from_secs(60));
        c.put("a", 1);
        c.put("a", 10);
        c.put("b", 2);
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(&"a"), Some(&10));
    }

    #[test]
    fn expires_entries() {
        let mut c = LruCache::new(4, Duration::from_secs(10));
        let t0 = Instant::now();
        c.put_at("a", 1, t0);
        assert_eq!(c.get_at(&"a", t0 + Duration::from_secs(5)), Some(&1));
        assert_eq!(c.get_at(&"a", t0 + Duration::from_secs(11)), None);
        assert!(c.is_empty());
    }

    #[test]
    fn slots_are_recycled() {
        let mut c = LruCache::new(3, Duration::from_secs(60));
        for i in 0..1000 {
            c.put(i, i);
        }
        assert_eq!(c.len(), 3);
        assert!(c.slots.len() <= 4);
        assert_eq!(c.get(&999), Some(&999));
        assert_eq!(c.get(&996), None);
    }
}
