//! A small LRU cache bounded by total size in bytes.

use std::collections::HashMap;
use std::hash::Hash;

struct Entry<V> {
    value: V,
    size: usize,
    last_used: u64,
}

pub struct LruCache<K, V> {
    map: HashMap<K, Entry<V>>,
    budget: usize,
    used: usize,
    tick: u64,
}

impl<K: Eq + Hash + Clone, V> LruCache<K, V> {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            map: HashMap::new(),
            budget: budget_bytes,
            used: 0,
            tick: 0,
        }
    }

    /// Change the budget, evicting least-recently-used entries if over it.
    pub fn set_budget(&mut self, budget_bytes: usize) {
        self.budget = budget_bytes;
        self.evict_except(None);
    }

    /// Insert, then evict least-recently-used entries until within budget.
    /// The new entry itself is never evicted.
    pub fn insert(&mut self, key: K, value: V, size: usize) {
        self.tick += 1;
        if let Some(old) = self.map.insert(
            key.clone(),
            Entry {
                value,
                size,
                last_used: self.tick,
            },
        ) {
            self.used -= old.size;
        }
        self.used += size;
        self.evict_except(Some(&key));
    }

    fn evict_except(&mut self, keep: Option<&K>) {
        while self.used > self.budget && !self.map.is_empty() {
            let oldest = self
                .map
                .iter()
                .filter(|(k, _)| Some(*k) != keep)
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone());
            let Some(oldest) = oldest else { break };
            if let Some(e) = self.map.remove(&oldest) {
                self.used -= e.size;
            }
        }
    }

    /// Get and mark as recently used.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(key).map(|e| {
            e.last_used = tick;
            &e.value
        })
    }

    pub fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.map.keys()
    }

    pub fn used_bytes(&self) -> usize {
        self.used
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used_over_budget() {
        let mut c = LruCache::new(30);
        c.insert("a", 1, 10);
        c.insert("b", 2, 10);
        c.insert("c", 3, 10);
        assert_eq!(c.used_bytes(), 30);

        c.get(&"a");
        c.insert("d", 4, 10);
        assert!(c.contains(&"a"));
        assert!(!c.contains(&"b"));
        assert!(c.contains(&"c"));
        assert!(c.contains(&"d"));
        assert_eq!(c.used_bytes(), 30);
    }

    #[test]
    fn never_evicts_the_new_entry() {
        let mut c = LruCache::new(10);
        c.insert("a", 1, 5);
        c.insert("big", 2, 50);
        assert!(c.contains(&"big"));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn lowering_the_budget_evicts_the_oldest() {
        let mut c = LruCache::new(100);
        c.insert("a", 1, 40);
        c.insert("b", 2, 40);
        c.get(&"a");
        c.set_budget(50);
        assert!(c.contains(&"a"));
        assert!(!c.contains(&"b"));
        assert_eq!(c.used_bytes(), 40);
    }

    #[test]
    fn replacing_updates_size() {
        let mut c = LruCache::new(100);
        c.insert("a", 1, 40);
        c.insert("a", 2, 10);
        assert_eq!(c.used_bytes(), 10);
        assert_eq!(c.get(&"a"), Some(&2));
    }
}
