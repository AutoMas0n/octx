//! Deterministic scheduling policy for the secretary pass.
//!
//! The policy layer is deliberately pure: it takes already-loaded task data
//! and returns proposed decisions. Nothing here touches the database or the
//! network, so every rule is unit-testable without fixtures or mocks.

use std::collections::BTreeMap;

use chrono::NaiveDate;

use crate::model::{Status, Task};

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

/// Group open tasks by due date, returning only dates with two or more tasks.
///
/// Note on semantics: the design speaks of a "same-time collision", but the
/// store keeps a date and no time of day. Two tasks therefore "collide" when
/// they share a date. Resolving every such pair would cap each day at one
/// task, which is more disruptive than the cap the user chose, so collision
/// resolution is applied through the cap in [`tasks_to_move`] rather than on
/// its own. A genuine time-of-day collision needs a timestamp field that the
/// model does not have; adding one is a separate change.
pub fn collision_groups(tasks: &[Task]) -> BTreeMap<NaiveDate, Vec<&Task>> {
    let mut groups: BTreeMap<NaiveDate, Vec<&Task>> = BTreeMap::new();
    for task in tasks {
        if task.status != Status::Pending {
            continue;
        }
        if let Some(due) = task.due_date {
            groups.entry(due).or_default().push(task);
        }
    }
    groups.retain(|_, group| group.len() > 1);
    groups
}

/// Choose which tasks to move off a day so that at most `cap` remain.
///
/// Pinned tasks never move. Among movable tasks the highest priority stays;
/// equal priorities keep the earliest created, so the outcome is stable
/// across runs and never depends on query order. Returns tasks to move,
/// lowest priority and newest first.
pub fn tasks_to_move<'a>(tasks: &'a [Task], cap: usize) -> Vec<&'a Task> {
    let mut movable: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.status == Status::Pending && !task.pinned)
        .collect();
    if movable.len() <= cap {
        return Vec::new();
    }
    // Priority variants order High < Medium < Low, so an ascending sort keeps
    // the highest priority first and the oldest first among equals. The tail
    // is what moves; reversing it puts the least-established task first.
    movable.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
    let mut moved = movable.split_off(cap);
    moved.reverse();
    moved
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

    fn task(
        id: &str,
        priority: crate::model::Priority,
        created_at: &str,
        due: NaiveDate,
        pinned: bool,
    ) -> Task {
        Task {
            id: id.into(),
            title: id.into(),
            description: None,
            notes: None,
            status: Status::Pending,
            priority,
            category: None,
            due_date: Some(due),
            recurrence_pattern: None,
            parent_task_id: None,
            notified_at: None,
            completed_at: None,
            last_completed_at: None,
            decomposed_at: None,
            pinned,
            created_at: created_at.into(),
            updated_at: created_at.into(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn under_cap_moves_nothing() {
        let due = day(2026, 1, 5);
        let tasks = vec![
            task("a", crate::model::Priority::High, "2026-01-01", due, false),
            task("b", crate::model::Priority::Low, "2026-01-02", due, false),
        ];
        assert!(tasks_to_move(&tasks, 3).is_empty());
    }

    #[test]
    fn over_cap_moves_the_excess() {
        let due = day(2026, 1, 5);
        let tasks = vec![
            task("a", crate::model::Priority::High, "2026-01-01", due, false),
            task(
                "b",
                crate::model::Priority::Medium,
                "2026-01-02",
                due,
                false,
            ),
            task(
                "c",
                crate::model::Priority::Medium,
                "2026-01-03",
                due,
                false,
            ),
            task("d", crate::model::Priority::Low, "2026-01-04", due, false),
        ];
        let moved = tasks_to_move(&tasks, 3);
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].id, "d");
    }

    #[test]
    fn tie_break_moves_lowest_priority_first() {
        let due = day(2026, 1, 5);
        let tasks = vec![
            task(
                "high",
                crate::model::Priority::High,
                "2026-01-04",
                due,
                false,
            ),
            task("low", crate::model::Priority::Low, "2026-01-01", due, false),
            task(
                "med",
                crate::model::Priority::Medium,
                "2026-01-02",
                due,
                false,
            ),
            task(
                "high2",
                crate::model::Priority::High,
                "2026-01-03",
                due,
                false,
            ),
        ];
        let moved = tasks_to_move(&tasks, 2);
        assert_eq!(moved.len(), 2);
        // Lowest priority moves first, even though it was created earliest.
        assert_eq!(moved[0].id, "low");
        assert_eq!(moved[1].id, "med");
    }

    #[test]
    fn equal_priority_moves_newest_first() {
        let due = day(2026, 1, 5);
        let tasks = vec![
            task(
                "old",
                crate::model::Priority::Medium,
                "2026-01-01",
                due,
                false,
            ),
            task(
                "new",
                crate::model::Priority::Medium,
                "2026-01-09",
                due,
                false,
            ),
            task(
                "mid",
                crate::model::Priority::Medium,
                "2026-01-05",
                due,
                false,
            ),
        ];
        let moved = tasks_to_move(&tasks, 1);
        assert_eq!(moved.len(), 2);
        // Newest moves first among equal priorities; oldest stays.
        assert_eq!(moved[0].id, "new");
        assert_eq!(moved[1].id, "mid");
    }

    #[test]
    fn pinned_tasks_never_move() {
        let due = day(2026, 1, 5);
        let tasks = vec![
            task(
                "pinned",
                crate::model::Priority::Low,
                "2026-01-01",
                due,
                true,
            ),
            task("a", crate::model::Priority::High, "2026-01-02", due, false),
            task("b", crate::model::Priority::High, "2026-01-03", due, false),
        ];
        // Pinned tasks are excluded from the movable set entirely.
        let moved = tasks_to_move(&tasks, 1);
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].id, "b");
    }

    #[test]
    fn collision_groups_only_returns_shared_dates() {
        let d1 = day(2026, 1, 5);
        let d2 = day(2026, 1, 6);
        let mut tasks = vec![
            task("a", crate::model::Priority::High, "2026-01-01", d1, false),
            task("b", crate::model::Priority::Low, "2026-01-02", d1, false),
            task("c", crate::model::Priority::Medium, "2026-01-03", d2, false),
        ];
        let groups = collision_groups(&tasks);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups.get(&d1).unwrap().len(), 2);
        assert!(!groups.contains_key(&d2));

        // Completed tasks are not part of a collision.
        tasks[1].status = Status::Completed;
        let groups = collision_groups(&tasks);
        assert!(groups.is_empty());
    }
}
