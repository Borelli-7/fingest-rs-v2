use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

/// Fixed-window attempt counter for the credential endpoints.
///
/// In-process on purpose: it needs no extra infrastructure and bounds online guessing per
/// instance. Behind a reverse proxy the peer address is the proxy's, so deployments there
/// should also limit at the proxy.
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    windows: Mutex<HashMap<String, (Instant, u32)>>,
}

/// Above this many tracked keys, expired windows are pruned before counting.
const PRUNE_THRESHOLD: usize = 10_000;

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Records an attempt for `key`. Returns the time until the window resets when the
    /// limit is already used up.
    pub fn check(&self, key: &str) -> Result<(), Duration> {
        self.check_at(key, Instant::now())
    }

    fn check_at(&self, key: &str, now: Instant) -> Result<(), Duration> {
        let mut windows = self.windows.lock().expect("lock poisoned");

        if windows.len() > PRUNE_THRESHOLD {
            windows.retain(|_, (started, _)| now.duration_since(*started) < self.window);
        }

        let (started, count) = windows.entry(key.to_owned()).or_insert((now, 0));
        if now.duration_since(*started) >= self.window {
            *started = now;
            *count = 0;
        }

        if *count >= self.limit {
            return Err(self.window.saturating_sub(now.duration_since(*started)));
        }

        *count += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_secs(60);

    #[test]
    fn allows_up_to_the_limit_then_refuses() {
        let limiter = RateLimiter::new(3, WINDOW);
        let now = Instant::now();

        for _ in 0..3 {
            assert!(limiter.check_at("ip:1", now).is_ok());
        }
        assert!(limiter.check_at("ip:1", now).is_err());
    }

    #[test]
    fn keys_are_counted_independently() {
        let limiter = RateLimiter::new(1, WINDOW);
        let now = Instant::now();

        assert!(limiter.check_at("ip:1", now).is_ok());
        assert!(limiter.check_at("ip:2", now).is_ok());
        assert!(limiter.check_at("ip:1", now).is_err());
    }

    #[test]
    fn the_window_resets() {
        let limiter = RateLimiter::new(1, WINDOW);
        let now = Instant::now();

        assert!(limiter.check_at("ip:1", now).is_ok());
        assert!(limiter.check_at("ip:1", now).is_err());
        assert!(limiter.check_at("ip:1", now + WINDOW).is_ok());
    }

    #[test]
    fn a_refusal_reports_the_remaining_wait() {
        let limiter = RateLimiter::new(1, WINDOW);
        let now = Instant::now();
        limiter.check_at("ip:1", now).unwrap();

        let wait = limiter
            .check_at("ip:1", now + Duration::from_secs(20))
            .unwrap_err();

        assert_eq!(wait, Duration::from_secs(40));
    }
}
