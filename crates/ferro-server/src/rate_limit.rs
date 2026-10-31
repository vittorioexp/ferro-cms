//! Per-instance HTTP rate limiting (fixed window).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    entries: Mutex<HashMap<String, Window>>,
    max: u32,
    window: Duration,
}

#[derive(Clone, Copy)]
struct Window {
    count: u32,
    start: Instant,
}

impl RateLimiter {
    /// `max` requests per `window`; `max == 0` disables limiting.
    pub fn new(max: u32, window: Duration) -> Self {
        RateLimiter { entries: Mutex::new(HashMap::new()), max, window }
    }

    pub fn disabled() -> Self {
        Self::new(0, Duration::from_secs(60))
    }

    pub fn enabled(&self) -> bool {
        self.max > 0
    }

    /// `Err(seconds)` until the window resets.
    pub fn check(&self, key: &str) -> Result<(), u64> {
        if self.max == 0 {
            return Ok(());
        }
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if map.len() > 20_000 {
            map.retain(|_, w| now.duration_since(w.start) < self.window);
        }
        let w = map.entry(key.to_string()).or_insert(Window { count: 0, start: now });
        if now.duration_since(w.start) >= self.window {
            *w = Window { count: 0, start: now };
        }
        if w.count >= self.max {
            let left = self.window.saturating_sub(now.duration_since(w.start));
            return Err(left.as_secs().max(1));
        }
        w.count += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_window_resets() {
        let lim = RateLimiter::new(2, Duration::from_millis(50));
        assert!(lim.check("k").is_ok());
        assert!(lim.check("k").is_ok());
        assert!(lim.check("k").is_err());
        std::thread::sleep(Duration::from_millis(60));
        assert!(lim.check("k").is_ok());
    }
}
