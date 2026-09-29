use std::{
    collections::HashMap,
    hash::{BuildHasher, RandomState},
    sync::Mutex,
    time::{Duration, Instant},
};

/// Fixed-window attempt counter for the credential endpoints.
///
/// In-process on purpose: it needs no extra infrastructure and bounds online guessing per
/// instance. Behind a reverse proxy the peer address is the proxy's, so deployments there
/// should also limit at the proxy.
///
/// Its own cost is bounded too, because its keys come from clients:
/// - keys are stored as keyed 64-bit hashes, so a longer login cannot make an entry larger;
/// - expired windows are swept at most once per window, so the sweep is amortised rather
///   than repeated on every call while the table holds many live keys;
/// - at most `max_keys` windows are tracked. A full table refuses keys it does not already
///   hold until the next sweep can free room: failing closed keeps the limiter a limit.
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    max_keys: usize,
    hasher: RandomState,
    state: Mutex<State>,
}

struct State {
    windows: HashMap<u64, (Instant, u32)>,
    last_sweep: Instant,
    #[cfg(test)]
    sweeps: usize,
}

impl State {
    fn sweep(&mut self, now: Instant, window: Duration) {
        self.windows
            .retain(|_, (started, _)| now.saturating_duration_since(*started) < window);
        self.last_sweep = now;
        #[cfg(test)]
        {
            self.sweeps += 1;
        }
    }
}

/// Below this many tracked keys, expired windows are left in place: they cost little.
const SWEEP_THRESHOLD: usize = 10_000;

impl RateLimiter {
    /// Enough for every client of a busy instance in one window, at a few MiB of memory.
    pub const DEFAULT_MAX_KEYS: usize = 100_000;

    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            max_keys: Self::DEFAULT_MAX_KEYS,
            hasher: RandomState::new(),
            state: Mutex::new(State {
                windows: HashMap::new(),
                last_sweep: Instant::now(),
                #[cfg(test)]
                sweeps: 0,
            }),
        }
    }

    pub fn with_max_keys(mut self, max_keys: usize) -> Self {
        self.max_keys = max_keys;
        self
    }

    /// Records an attempt for `key`. Returns the time until the window resets when the
    /// limit is already used up, or until room may free up when the table is full.
    pub fn check(&self, key: &str) -> Result<(), Duration> {
        self.check_at(key, Instant::now())
    }

    /// How many windows are currently held, expired ones included until the next sweep.
    pub fn tracked_keys(&self) -> usize {
        self.state.lock().expect("lock poisoned").windows.len()
    }

    fn check_at(&self, key: &str, now: Instant) -> Result<(), Duration> {
        let key = self.hasher.hash_one(key);
        let mut state = self.state.lock().expect("lock poisoned");

        let crowded = state.windows.len() > SWEEP_THRESHOLD || state.windows.len() >= self.max_keys;
        if crowded && now.saturating_duration_since(state.last_sweep) >= self.window {
            state.sweep(now, self.window);
        }

        if state.windows.len() >= self.max_keys && !state.windows.contains_key(&key) {
            let since_sweep = now.saturating_duration_since(state.last_sweep);
            return Err(self.window.saturating_sub(since_sweep));
        }

        let (started, count) = state.windows.entry(key).or_insert((now, 0));
        if now.saturating_duration_since(*started) >= self.window {
            *started = now;
            *count = 0;
        }

        if *count >= self.limit {
            return Err(self
                .window
                .saturating_sub(now.saturating_duration_since(*started)));
        }

        *count += 1;
        Ok(())
    }

    #[cfg(test)]
    fn sweeps(&self) -> usize {
        self.state.lock().expect("lock poisoned").sweeps
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

    /// Past the sweep threshold with every window still live, a sweep frees nothing; it
    /// must not be repeated on every call (it used to be, at ~1 ms per call under the lock).
    #[test]
    fn a_crowded_table_is_swept_at_most_once_per_window() {
        let limiter = RateLimiter::new(10, WINDOW);
        let start = Instant::now() + WINDOW;

        for i in 0..=SWEEP_THRESHOLD + 1 {
            limiter.check_at(&format!("ip:{i}"), start).unwrap();
        }
        assert_eq!(limiter.sweeps(), 1, "the first crowded call sweeps once");

        let later = start + Duration::from_secs(1);
        for i in 0..1_000 {
            limiter.check_at(&format!("new:{i}"), later).unwrap();
        }
        assert_eq!(limiter.sweeps(), 1, "no rescans within the same window");

        limiter.check_at("ip:after", start + WINDOW).unwrap();
        assert_eq!(limiter.sweeps(), 2);
        assert!(
            limiter.tracked_keys() < 1_100,
            "the next sweep drops the expired windows, got {}",
            limiter.tracked_keys()
        );
    }

    #[test]
    fn the_number_of_tracked_keys_never_exceeds_the_cap() {
        let limiter = RateLimiter::new(10, WINDOW).with_max_keys(3);
        let now = Instant::now();

        for i in 0..100 {
            let _ = limiter.check_at(&format!("ip:{i}"), now);
            assert!(limiter.tracked_keys() <= 3);
        }
    }

    #[test]
    fn a_full_table_refuses_new_keys_but_still_counts_known_ones() {
        let limiter = RateLimiter::new(10, WINDOW).with_max_keys(2);
        let now = Instant::now();
        limiter.check_at("ip:1", now).unwrap();
        limiter.check_at("ip:2", now).unwrap();

        assert!(
            limiter.check_at("ip:3", now).is_err(),
            "a full table fails closed"
        );
        assert!(
            limiter.check_at("ip:1", now).is_ok(),
            "known keys keep counting"
        );
    }

    #[test]
    fn a_full_table_frees_room_once_its_windows_expire() {
        let limiter = RateLimiter::new(10, WINDOW).with_max_keys(2);
        let now = Instant::now();
        limiter.check_at("ip:1", now).unwrap();
        limiter.check_at("ip:2", now).unwrap();

        let wait = limiter.check_at("ip:3", now).unwrap_err();
        assert!(wait > Duration::ZERO && wait <= WINDOW);

        assert!(limiter.check_at("ip:3", now + WINDOW).is_ok());
        assert_eq!(limiter.tracked_keys(), 1);
    }

    /// The stored key is a fixed-size hash, so a huge key costs no more than a short one
    /// and still maps to the same window every time.
    #[test]
    fn long_keys_are_counted_like_short_ones() {
        let limiter = RateLimiter::new(1, WINDOW);
        let now = Instant::now();
        let huge = format!("login:{}", "x".repeat(1 << 20));

        assert!(limiter.check_at(&huge, now).is_ok());
        assert!(limiter.check_at(&huge, now).is_err());
        assert_eq!(limiter.tracked_keys(), 1);
    }
}
