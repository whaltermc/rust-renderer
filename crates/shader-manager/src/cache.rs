//! Shader source and bytecode cache with content-addressed keys.
//!
//! Uses xxh3-64 for fast, collision-resistant hashing.

use std::collections::hash_map::Entry;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use parking_lot::RwLock;
use xxhash_rust::xxh3::xxh3_64;

use super::ShaderKey;

/// Cached shader entry.
#[derive(Debug, Clone)]
pub struct ShaderEntry {
    pub key: ShaderKey,
    pub translated_source: String,
    pub compiled: bool,
    pub last_used: Instant,
    pub use_count: u64,
    pub size_bytes: usize,
}

impl ShaderEntry {
    pub fn new(key: ShaderKey, translated_source: String) -> Self {
        let size_bytes = translated_source.len();
        Self {
            key,
            translated_source,
            compiled: false,
            last_used: Instant::now(),
            use_count: 0,
            size_bytes,
        }
    }
}

/// Shader cache for translated/compiled shader sources.
pub struct ShaderCache {
    entries: DashMap<u64, ShaderEntry>,
    by_key: DashMap<ShaderKey, u64>,
    max_bytes: u64,
    total_bytes: AtomicU64,
}

impl ShaderCache {
    pub fn new(max_bytes: u64) -> Self {
        Self {
            entries: DashMap::new(),
            by_key: DashMap::new(),
            max_bytes,
            total_bytes: AtomicU64::new(0),
        }
    }

    /// Look up a cache entry by source hash.
    pub fn get(&self, hash: &u64) -> Option<ShaderEntry> {
        self.entries.get_mut(hash).map(|mut e| {
            e.last_used = Instant::now();
            e.use_count += 1;
            e.clone()
        })
    }

    /// Look up by full shader key (hash + backend + stage).
    pub fn get_by_key(&self, key: &ShaderKey) -> Option<ShaderEntry> {
        let hash = self.by_key.get(key)?;
        self.get(&hash)
    }

    /// Insert a new cache entry.
    pub fn insert(&self, entry: ShaderEntry) {
        let hash = entry.key.hash;
        let size = entry.size_bytes as u64;

        self.evict_if_needed(size);

        self.entries.insert(hash, entry.clone());
        self.by_key.insert(entry.key, hash);
        self.total_bytes.fetch_add(size, Ordering::Relaxed);
    }

    /// Invalidate an entry by hash.
    pub fn invalidate(&self, hash: &u64) {
        if let Some((_, entry)) = self.entries.remove(hash) {
            self.by_key.remove(&entry.key);
            self.total_bytes.fetch_sub(entry.size_bytes as u64, Ordering::Relaxed);
        }
    }

    /// Clear the entire cache.
    pub fn clear(&self) {
        self.entries.clear();
        self.by_key.clear();
        self.total_bytes.store(0, Ordering::Relaxed);
    }

    /// Current cache size in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.total_bytes.load(Ordering::Relaxed)
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn evict_if_needed(&self, needed: u64) {
        if self.max_bytes == 0 {
            return;
        }
        let current = self.total_bytes.load(Ordering::Relaxed);
        if current + needed <= self.max_bytes {
            return;
        }

        let mut entries: Vec<_> = self.entries
            .iter()
            .map(|e| (e.last_used, e.use_count, e.key.hash, e.size_bytes))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        let target = self.max_bytes.saturating_sub(needed / 2);
        let mut freed = 0u64;
        for (_, _, hash, size) in entries {
            if current - freed <= target {
                break;
            }
            freed += size as u64;
            self.invalidate(&hash);
        }
    }
}

/// Compute a content hash for shader source.
pub fn hash_source(source: &str) -> u64 {
    xxh3_64(source.as_bytes())
}

/// Compute a content hash for a file.
pub fn hash_file(path: &Path) -> std::io::Result<u64> {
    let data = std::fs::read(path)?;
    Ok(xxh3_64(&data))
}

use super::ShaderStage;

/// Shader cache entry kind (for metrics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderKind {
    Translated,
    Compiled,
    Failed,
}
