//! In-memory cache for cacheable GET responses (per instance).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct CachedHttp {
    pub status: u16,
    pub body: Vec<u8>,
    pub etag: String,
    pub expires: Instant,
}

pub struct HttpCache {
    entries: Mutex<HashMap<String, CachedHttp>>,
    ttl: Duration,
    max_entries: usize,
}

impl HttpCache {
    pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
        HttpCache {
            entries: Mutex::new(HashMap::new()),
            ttl: if ttl_secs == 0 { Duration::ZERO } else { Duration::from_secs(ttl_secs) },
            max_entries,
        }
    }

    pub fn disabled() -> Self {
        Self::new(0, 0)
    }

    pub fn enabled(&self) -> bool {
        self.max_entries > 0 && self.ttl > Duration::ZERO
    }

    pub fn get(&self, key: &str) -> Option<CachedHttp> {
        if !self.enabled() {
            return None;
        }
        let map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let hit = map.get(key)?;
        if Instant::now() >= hit.expires {
            None
        } else {
            Some(hit.clone())
        }
    }

    pub fn put(&self, key: String, status: u16, body: Vec<u8>, etag: String) {
        if !self.enabled() || body.len() > 512 * 1024 {
            return;
        }
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= self.max_entries {
            let now = Instant::now();
            map.retain(|_, v| v.expires > now);
            if map.len() >= self.max_entries {
                map.clear();
            }
        }
        map.insert(
            key,
            CachedHttp {
                status,
                body,
                etag,
                expires: Instant::now() + self.ttl,
            },
        );
    }
}
