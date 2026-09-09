use chrono::{DateTime, NaiveDate, Utc};

/// Time as an injected dependency, so use cases stay deterministic under test.
pub trait Clock: Send + Sync {
    fn now_utc(&self) -> DateTime<Utc>;

    fn today(&self) -> NaiveDate {
        self.now_utc().date_naive()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Frozen clock for tests and for reproducing time-dependent bugs.
#[derive(Debug, Clone)]
pub struct FixedClock(pub DateTime<Utc>);

impl Clock for FixedClock {
    fn now_utc(&self) -> DateTime<Utc> {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_does_not_advance() {
        let instant = DateTime::parse_from_rfc3339("2026-09-05T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let clock = FixedClock(instant);
        assert_eq!(clock.now_utc(), instant);
        assert_eq!(clock.now_utc(), clock.now_utc());
        assert_eq!(clock.today(), NaiveDate::from_ymd_opt(2026, 9, 5).unwrap());
    }
}
