//! Deterministic scheduling policy for the secretary pass.
//!
//! The policy layer is deliberately pure: it takes already-loaded task data
//! and returns proposed decisions. Nothing here touches the database or the
//! network, so every rule is unit-testable without fixtures or mocks.

use std::collections::BTreeMap;

use chrono::NaiveDate;

/// A day whose pending-task count exceeds the configured cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayLoad {
    pub date: NaiveDate,
    pub count: usize,
}

impl DayLoad {
    /// How many tasks must move off this day to reach the cap.
    pub fn excess(&self, daily_cap: usize) -> usize {
        self.count.saturating_sub(daily_cap)
    }
}

/// Return days whose load is strictly greater than the cap, earliest first.
///
/// A day sitting exactly at the cap is not overloaded: the cap is the maximum
/// allowed, not a trigger threshold. Only `count > daily_cap` moves tasks.
pub fn overloaded_days(loads: &BTreeMap<NaiveDate, usize>, daily_cap: usize) -> Vec<DayLoad> {
    let mut overloaded = Vec::new();
    for (&date, &count) in loads {
        if count > daily_cap {
            overloaded.push(DayLoad { date, count });
        }
    }
    overloaded
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn loads(entries: &[(NaiveDate, usize)]) -> BTreeMap<NaiveDate, usize> {
        entries.iter().copied().collect()
    }

    #[test]
    fn under_cap_day_is_not_overloaded() {
        let map = loads(&[(day(2026, 1, 5), 2)]);
        assert!(overloaded_days(&map, 3).is_empty());
    }

    #[test]
    fn day_exactly_at_cap_is_not_overloaded() {
        let map = loads(&[(day(2026, 1, 5), 3)]);
        assert!(overloaded_days(&map, 3).is_empty());
    }

    #[test]
    fn over_cap_day_is_overloaded_with_excess() {
        let map = loads(&[(day(2026, 1, 5), 5)]);
        let overloaded = overloaded_days(&map, 3);
        assert_eq!(
            overloaded,
            vec![DayLoad {
                date: day(2026, 1, 5),
                count: 5,
            }]
        );
        assert_eq!(overloaded[0].excess(3), 2);
    }

    #[test]
    fn only_overloaded_days_are_returned_in_date_order() {
        let map = loads(&[
            (day(2026, 1, 7), 4),
            (day(2026, 1, 5), 1),
            (day(2026, 1, 6), 3),
            (day(2026, 1, 8), 6),
        ]);
        let overloaded = overloaded_days(&map, 3);
        assert_eq!(
            overloaded.iter().map(|d| d.date).collect::<Vec<_>>(),
            vec![day(2026, 1, 7), day(2026, 1, 8)]
        );
    }

    #[test]
    fn zero_cap_marks_every_nonempty_day() {
        let map = loads(&[(day(2026, 1, 5), 1)]);
        assert_eq!(overloaded_days(&map, 0).len(), 1);
    }

    #[test]
    fn empty_map_has_no_overloads() {
        assert!(overloaded_days(&BTreeMap::new(), 3).is_empty());
    }
}
