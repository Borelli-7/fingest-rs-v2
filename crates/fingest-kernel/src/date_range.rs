use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// An inclusive `[start, end]` span of days.
///
/// Absent bounds widen to the v1 sentinels (`0001-01-01` / `9999-12-31`) rather than
/// becoming `None`, so "no filter" and "full range" stay the same query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl DateRange {
    pub const MIN_DATE: &'static str = "0001-01-01";
    pub const MAX_DATE: &'static str = "9999-12-31";

    pub fn min_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(1, 1, 1).expect("MIN_DATE is a valid date")
    }

    pub fn max_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(9999, 12, 31).expect("MAX_DATE is a valid date")
    }

    pub fn new(start: Option<NaiveDate>, end: Option<NaiveDate>) -> Self {
        Self {
            start: start.unwrap_or_else(Self::min_date),
            end: end.unwrap_or_else(Self::max_date),
        }
    }

    /// Empty strings widen to the sentinels, matching v1's query-parameter handling.
    pub fn from_string(start: Option<&str>, end: Option<&str>) -> Result<Self, chrono::ParseError> {
        let start_date = match start {
            Some(s) if !s.is_empty() => NaiveDate::parse_from_str(s, "%Y-%m-%d")?,
            _ => Self::min_date(),
        };
        let end_date = match end {
            Some(e) if !e.is_empty() => NaiveDate::parse_from_str(e, "%Y-%m-%d")?,
            _ => Self::max_date(),
        };
        Ok(Self {
            start: start_date,
            end: end_date,
        })
    }

    pub fn contains_date(&self, date: NaiveDate) -> bool {
        date >= self.start && date <= self.end
    }

    pub fn contains_date_str(&self, date_str: &str) -> Result<bool, chrono::ParseError> {
        Ok(self.contains_date(NaiveDate::parse_from_str(date_str, "%Y-%m-%d")?))
    }

    pub fn is_valid(&self) -> bool {
        self.start <= self.end
    }

    pub fn require_valid(&self) -> Result<(), DomainError> {
        if !self.is_valid() {
            return Err(DomainError::InvalidDateRange {
                start: self.start.to_string(),
                end: self.end.to_string(),
            });
        }
        Ok(())
    }
}

impl Default for DateRange {
    fn default() -> Self {
        let today = Utc::now().naive_utc().date();
        Self {
            start: today,
            end: today,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    // --- ported from v1 src/models/date_range.rs ---

    #[test]
    fn new_with_both_bounds() {
        let range = DateRange::new(Some(d(2023, 1, 1)), Some(d(2023, 12, 31)));
        assert_eq!(range.start, d(2023, 1, 1));
        assert_eq!(range.end, d(2023, 12, 31));
    }

    #[test]
    fn new_without_bounds_uses_sentinels() {
        let range = DateRange::new(None, None);
        assert_eq!(range.start, DateRange::min_date());
        assert_eq!(range.end, DateRange::max_date());
    }

    #[test]
    fn sentinels_match_v1_strings() {
        assert_eq!(DateRange::min_date().to_string(), DateRange::MIN_DATE);
        assert_eq!(DateRange::max_date().to_string(), DateRange::MAX_DATE);
    }

    #[test]
    fn from_string_parses() {
        let range = DateRange::from_string(Some("2023-01-01"), Some("2023-12-31")).unwrap();
        assert_eq!(range.start, d(2023, 1, 1));
        assert_eq!(range.end, d(2023, 12, 31));
    }

    #[test]
    fn from_string_treats_empty_as_absent() {
        let range = DateRange::from_string(Some(""), Some("")).unwrap();
        assert_eq!(range.start, DateRange::min_date());
        assert_eq!(range.end, DateRange::max_date());
    }

    #[test]
    fn from_string_rejects_malformed() {
        assert!(DateRange::from_string(Some("01-01-2023"), None).is_err());
    }

    #[test]
    fn contains_date_is_inclusive() {
        let range = DateRange::new(Some(d(2023, 1, 1)), Some(d(2023, 12, 31)));
        assert!(range.contains_date(d(2023, 6, 15)));
        assert!(range.contains_date(d(2023, 1, 1)));
        assert!(range.contains_date(d(2023, 12, 31)));
        assert!(!range.contains_date(d(2024, 1, 1)));
    }

    #[test]
    fn contains_date_str() {
        let range = DateRange::new(Some(d(2023, 1, 1)), Some(d(2023, 12, 31)));
        assert!(range.contains_date_str("2023-06-15").unwrap());
        assert!(range.contains_date_str("2024-06-15").is_ok_and(|c| !c));
    }

    #[test]
    fn validity() {
        assert!(DateRange::new(Some(d(2023, 1, 1)), Some(d(2023, 12, 31))).is_valid());
        assert!(!DateRange::new(Some(d(2023, 12, 31)), Some(d(2023, 1, 1))).is_valid());
    }

    #[test]
    fn require_valid_reports_the_bounds() {
        let inverted = DateRange::new(Some(d(2023, 12, 31)), Some(d(2023, 1, 1)));
        assert_eq!(
            inverted.require_valid().unwrap_err(),
            DomainError::InvalidDateRange {
                start: "2023-12-31".into(),
                end: "2023-01-01".into()
            }
        );
    }
}
