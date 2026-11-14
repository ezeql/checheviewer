use std::path::{Path, PathBuf};

use lru::LruCache;

/// Default budget of the image cache.
///
/// Measured in pixel bytes, not image count: ten 50 MP photos and a thousand
/// thumbnails take very different amounts of space, and what matters is
/// memory, not the count.
pub const DEFAULT_BUDGET: usize = 1024 * 1024 * 1024; // 1 GiB

/// Budget of the thumbnail cache.
///
/// A 256×256 RGBA thumbnail is 256 KiB, so this fits about a thousand cells:
/// well beyond what fits on screen, and a large folder can be scrolled
/// through without re-decoding what was already seen.
pub const THUMBNAIL_BUDGET: usize = 256 * 1024 * 1024; // 256 MiB

/// LRU cache bounded by bytes.
pub struct ByteCache<V> {
    entries: LruCache<PathBuf, (V, usize)>,
    bytes: usize,
    budget: usize,
}

impl<V> ByteCache<V> {
    pub fn new(budget: usize) -> Self {
        Self { entries: LruCache::unbounded(), bytes: 0, budget }
    }

    /// Returns the value and marks it as most recently used.
    pub fn get(&mut self, path: &Path) -> Option<&V> {
        self.entries.get(path).map(|(value, _)| value)
    }

    /// Is it in the cache? Doesn't change the usage order.
    pub fn contains(&self, path: &Path) -> bool {
        self.entries.peek(path).is_some()
    }

    /// Stores a value of `size` bytes and evicts the oldest if needed.
    pub fn insert(&mut self, path: PathBuf, value: V, size: usize) {
        if let Some((_, previous)) = self.entries.put(path, (value, size)) {
            self.bytes -= previous;
        }
        self.bytes += size;
        self.evict();
    }

    fn evict(&mut self) {
        // The last entry is never evicted: it's the one the user is looking
        // at, and a single photo can exceed the budget on its own.
        while self.bytes > self.budget && self.entries.len() > 1 {
            match self.entries.pop_lru() {
                Some((_, (_, size))) => self.bytes -= size,
                None => break,
            }
        }
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    #[test]
    fn stores_and_returns() {
        let mut cache: ByteCache<u32> = ByteCache::new(100);
        cache.insert(path("a"), 1, 10);
        assert_eq!(cache.get(&path("a")), Some(&1));
        assert_eq!(cache.bytes(), 10);
        assert!(cache.contains(&path("a")));
        assert!(!cache.contains(&path("b")));
    }

    #[test]
    fn evicts_oldest_when_over_budget() {
        let mut cache: ByteCache<u32> = ByteCache::new(100);
        cache.insert(path("a"), 1, 50);
        cache.insert(path("b"), 2, 50);
        cache.insert(path("c"), 3, 50); // over budget: "a" goes

        assert!(!cache.contains(&path("a")));
        assert!(cache.contains(&path("b")));
        assert!(cache.contains(&path("c")));
        assert_eq!(cache.bytes(), 100);
    }

    #[test]
    fn using_an_entry_saves_it_from_eviction() {
        let mut cache: ByteCache<u32> = ByteCache::new(100);
        cache.insert(path("a"), 1, 50);
        cache.insert(path("b"), 2, 50);
        cache.get(&path("a")); // "a" becomes the most recent
        cache.insert(path("c"), 3, 50);

        assert!(cache.contains(&path("a")), "the just-used entry should not be evicted");
        assert!(!cache.contains(&path("b")));
    }

    #[test]
    fn never_evicts_the_only_entry_even_if_it_does_not_fit() {
        let mut cache: ByteCache<u32> = ByteCache::new(100);
        cache.insert(path("huge"), 1, 5_000);

        assert!(cache.contains(&path("huge")), "the current image must survive");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn replacing_a_key_does_not_duplicate_bytes() {
        let mut cache: ByteCache<u32> = ByteCache::new(1_000);
        cache.insert(path("a"), 1, 100);
        cache.insert(path("a"), 2, 30);

        assert_eq!(cache.bytes(), 30);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&path("a")), Some(&2));
    }

    #[test]
    fn memory_use_does_not_grow_unbounded() {
        let mut cache: ByteCache<u32> = ByteCache::new(1_000);
        for i in 0..500 {
            cache.insert(path(&format!("img{i}")), i, 100);
            assert!(cache.bytes() <= 1_000, "cache went over budget at iteration {i}");
        }
        assert_eq!(cache.len(), 10);
    }

    #[test]
    fn clear_frees_everything() {
        let mut cache: ByteCache<u32> = ByteCache::new(1_000);
        cache.insert(path("a"), 1, 100);
        cache.clear();

        assert_eq!(cache.bytes(), 0);
        assert!(cache.is_empty());
    }
}
