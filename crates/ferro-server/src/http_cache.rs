//! In-memory cache for cacheable GET responses (per instance).
//!
//! Every write invalidates the whole cache (see [`HttpCache::invalidate`]): a generation number taken before a
//! request runs keeps a response computed before a concurrent write from being stored after it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const MAX_CACHED_BODY: usize = 512 * 1024;

#[derive(Clone)]
pub struct CachedHttp {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    pub etag: String,
    pub expires: Instant,
}

pub struct HttpCache {
    entries: Mutex<HashMap<String, CachedHttp>>,
    generation: AtomicU64,
    ttl: Duration,
    max_entries: usize,
}

impl HttpCache {
    pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
        HttpCache {
            entries: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
            ttl: Duration::from_secs(ttl_secs),
            max_entries,
        }
    }

    pub fn enabled(&self) -> bool {
        self.max_entries > 0 && self.ttl > Duration::ZERO
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn invalidate(&self) {
        if !self.enabled() {
            return;
        }
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        self.generation.fetch_add(1, Ordering::SeqCst);
        map.clear();
    }

    pub fn get(&self, key: &str) -> Option<CachedHttp> {
        if !self.enabled() {
            return None;
        }
        let map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        map.get(key).filter(|hit| Instant::now() < hit.expires).cloned()
    }

    /// Store a response computed while the cache was at `generation`; dropped if a write happened since.
    pub fn put(&self, key: String, generation: u64, status: u16, content_type: Option<String>, body: Vec<u8>, etag: String) {
        if !self.enabled() || body.len() > MAX_CACHED_BODY {
            return;
        }
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        if map.len() >= self.max_entries {
            let now = Instant::now();
            map.retain(|_, v| v.expires > now);
            if map.len() >= self.max_entries {
                map.clear();
            }
        }
        map.insert(key, CachedHttp { status, content_type, body, etag, expires: Instant::now() + self.ttl });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidate_drops_entries_and_stale_puts() {
        let c = HttpCache::new(60, 10);
        let g = c.generation();
        c.put("k".into(), g, 200, None, b"1".to_vec(), "\"e\"".into());
        assert!(c.get("k").is_some());
        let before_write = c.generation();
        c.invalidate();
        assert!(c.get("k").is_none());
        c.put("k".into(), before_write, 200, None, b"old".to_vec(), "\"e\"".into());
        assert!(c.get("k").is_none(), "a response computed before the write must not be cached");
        c.put("k".into(), c.generation(), 200, None, b"new".to_vec(), "\"e\"".into());
        assert_eq!(c.get("k").unwrap().body, b"new");
    }

    #[test]
    fn disabled_cache_stores_nothing() {
        let c = HttpCache::new(0, 10);
        c.put("k".into(), 0, 200, None, b"1".to_vec(), "\"e\"".into());
        assert!(c.get("k").is_none());
    }
}
