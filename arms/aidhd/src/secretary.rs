//! Deterministic scheduling policy for the secretary pass.
//!
//! The policy layer is deliberately pure: it takes already-loaded task data
//! and returns proposed decisions. Nothing here touches the database or the
//! network, so every rule is unit-testable without fixtures or mocks.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, NaiveDate, NaiveTime};

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

/// Group open tasks by their exact due timestamp, returning only timestamps
/// shared by two or more tasks.
///
/// A task's due timestamp is its `due_date` plus its optional `due_time`.
/// Tasks that name only a date group under `None`, so two undated-time tasks
/// on the same day collide with each other but never with a task that names a
/// specific time. That is the "same-time collision" the design describes: a
/// real clash of moments, not merely a busy day. A busy day is handled by the
/// daily cap in [`tasks_to_move`].
pub fn collision_groups(tasks: &[Task]) -> BTreeMap<(NaiveDate, Option<NaiveTime>), Vec<&Task>> {
    let mut groups: BTreeMap<(NaiveDate, Option<NaiveTime>), Vec<&Task>> = BTreeMap::new();
    for task in tasks {
        if task.status != Status::Pending {
            continue;
        }
        if let Some(due) = task.due_date {
            groups.entry((due, task.due_time)).or_default().push(task);
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
pub fn tasks_to_move(tasks: &[Task], cap: usize) -> Vec<&Task> {
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

/// Find the next day at or after `from` that has room for one more task.
///
/// `counts` maps a date to its current pending load. A day is open when its
/// count is below `cap`. The search never looks before `today`, so a task is
/// never moved into the past, and never past `today + horizon_days`, so a run
/// cannot push work indefinitely far out. Returns `None` when the horizon is
/// exhausted, which the caller records as a `skipped` decision.
pub fn next_open_day(
    from: NaiveDate,
    today: NaiveDate,
    counts: &BTreeMap<NaiveDate, usize>,
    cap: usize,
    horizon_days: i64,
) -> Option<NaiveDate> {
    let start = from.max(today);
    let limit = today + Duration::days(horizon_days);
    let mut day = start;
    while day <= limit {
        let load = counts.get(&day).copied().unwrap_or(0);
        if load < cap {
            return Some(day);
        }
        day += Duration::days(1);
    }
    None
}

/// Select overdue tasks the pass may reschedule.
///
/// Excluded, each for a distinct reason:
/// - pinned tasks are immovable by user intent;
/// - recurring tasks advance by their own recurrence path and must not be
///   treated as overdue one-offs;
/// - non-pending tasks are already closed;
/// - tasks without a due date are handled by undated fill instead.
pub fn reschedule_candidates(tasks: &[Task], today: NaiveDate) -> Vec<&Task> {
    tasks
        .iter()
        .filter(|task| {
            task.status == Status::Pending
                && !task.pinned
                && task.decomposed_at.is_none()
                && task.recurrence_pattern.is_none()
                && task.due_date.is_some_and(|due| due < today)
        })
        .collect()
}

/// A task the secretary may break into subtasks.
///
/// A task is stuck when it is still open, the user has not pinned it, it is
/// not itself a step, it has not already been broken down, and nothing else
/// is waiting on it as a parent. Recurring tasks are excluded because their
/// overdue state belongs to the recurrence path, not to decomposition.
///
/// At least one trigger must hold:
/// - the due date is more than `stuck_after_days` days in the past;
/// - there is no due date and the task is older than `stuck_after_days`;
/// - it has been rescheduled `stuck_after_moves` or more times since it was
///   last completed. `reschedule_counts` supplies that count per task so the
///   policy stays pure; the caller derives it from the decision log.
///
/// Results are ordered longest-neglected first, then by priority, so a capped
/// run always picks the tasks most in need of breaking down.
pub fn stuck_candidates<'a>(
    tasks: &'a [Task],
    reschedule_counts: &BTreeMap<String, usize>,
    today: NaiveDate,
    stuck_after_days: i64,
    stuck_after_moves: usize,
) -> Vec<&'a Task> {
    let parents_with_children: BTreeSet<&str> = tasks
        .iter()
        .filter_map(|task| task.parent_task_id.as_deref())
        .collect();
    let mut stuck: Vec<&Task> = tasks
        .iter()
        .filter(|task| {
            is_stuck(
                task,
                &parents_with_children,
                reschedule_counts,
                today,
                stuck_after_days,
                stuck_after_moves,
            )
        })
        .collect();
    stuck.sort_by(|a, b| {
        neglect(b, today)
            .cmp(&neglect(a, today))
            .then_with(|| a.priority.cmp(&b.priority))
    });
    stuck
}

/// Days since a task was last touched: overdue days when dated, age when not.
fn neglect(task: &Task, today: NaiveDate) -> i64 {
    match task.due_date {
        Some(due) => (today - due).num_days(),
        None => created_date(task)
            .map(|created| (today - created).num_days())
            .unwrap_or(0),
    }
}

/// The date part of a task's `created_at` timestamp, if it parses.
fn created_date(task: &Task) -> Option<NaiveDate> {
    let head = task.created_at.get(..10)?;
    NaiveDate::parse_from_str(head, "%Y-%m-%d").ok()
}

fn is_stuck(
    task: &Task,
    parents_with_children: &BTreeSet<&str>,
    reschedule_counts: &BTreeMap<String, usize>,
    today: NaiveDate,
    stuck_after_days: i64,
    stuck_after_moves: usize,
) -> bool {
    if task.status != Status::Pending
        || task.pinned
        || task.decomposed_at.is_some()
        || task.parent_task_id.is_some()
        || task.recurrence_pattern.is_some()
        || parents_with_children.contains(task.id.as_str())
    {
        return false;
    }
    let overdue = task
        .due_date
        .is_some_and(|due| (today - due).num_days() > stuck_after_days);
    let never_dated = task.due_date.is_none()
        && created_date(task)
            .is_some_and(|created| (today - created).num_days() > stuck_after_days);
    let moved = reschedule_counts.get(&task.id).copied().unwrap_or(0) >= stuck_after_moves;
    overdue || never_dated || moved
}

/// A deterministic split used when no LLM is available.
///
/// Splits on commas and the word " and " into at most three non-empty,
/// trimmed parts. Fewer than two parts means the title has no natural seam,
/// and the caller records a `skipped` decision instead of forcing a split:
/// inventing steps for a single task would be worse than leaving it alone.
pub fn fallback_split(title: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for chunk in title.split(',') {
        for piece in chunk.split(" and ") {
            let part = piece.trim();
            if !part.is_empty() {
                parts.push(part.to_string());
            }
        }
    }
    if parts.len() < 2 {
        return Vec::new();
    }
    parts.truncate(3);
    parts
}

/// True when a recent `skipped` decomposition should suppress a retry.
///
/// Without this, one unsplittable title would write a skip row on every run
/// forever. The backoff is measured from the skip's date, so a stale skip
/// does not block a task whose title the user has since edited.
pub fn skip_blocks_retry(last_skipped_at: Option<&str>, today: NaiveDate, days: i64) -> bool {
    let Some(stamp) = last_skipped_at else {
        return false;
    };
    let Some(head) = stamp.get(..10) else {
        return false;
    };
    NaiveDate::parse_from_str(head, "%Y-%m-%d")
        .is_ok_and(|skipped| (today - skipped).num_days() < days)
}

/// The seam between decomposition and whatever performs the completion.
///
/// Keeping this a trait lets the pass be tested with no network, and lets a
/// missing credential surface as a plain error the caller turns into a
/// deterministic fallback. Only `LlmClient` implements it in production.
pub trait Completer {
    fn complete(&self, system: &str, user: &str) -> std::result::Result<String, String>;
}

/// Most subtasks one parent may be broken into.
pub const MAX_SUBTASKS: usize = 5;
/// Longest subtask title the secretary will accept.
pub const MAX_SUBTASK_CHARS: usize = 120;

/// Build the one batched decomposition request for a whole run.
///
/// Every candidate goes into a single prompt, so a run with several stuck
/// tasks still costs one request. The reply contract is an object mapping
/// task id to an array of step titles.
pub fn decomposition_prompt(candidates: &[&Task]) -> (String, String) {
    let system = "You break big, neglected tasks into small concrete steps.         Reply with only a JSON object mapping each task id to an array of at         most 5 step titles. Each title must be non-empty, distinct, and at         most 120 characters. Steps must be directly actionable and must not         invent unrelated work. Do not add commentary."
        .to_string();
    let listing: Vec<serde_json::Value> = candidates
        .iter()
        .map(|task| {
            serde_json::json!({
                "task_id": task.id,
                "title": task.title,
                "category": task.category,
            })
        })
        .collect();
    let user = format!(
        "Break these tasks into steps:\n{}",
        serde_json::to_string_pretty(&listing).unwrap_or_default()
    );
    (system, user)
}

/// Strip a surrounding Markdown code fence, if the model added one.
fn strip_code_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    rest.trim_start().trim_end_matches("```").trim()
}

/// Validate one parent's proposed steps.
///
/// Returns `None` when the proposal is unusable: empty, more than
/// [`MAX_SUBTASKS`], a blank title, a title over [`MAX_SUBTASK_CHARS`], or a
/// duplicate. A rejected parent falls back on its own; it never fails the run.
pub fn validate_subtasks(titles: &[String]) -> Option<Vec<String>> {
    if titles.is_empty() || titles.len() > MAX_SUBTASKS {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut steps = Vec::with_capacity(titles.len());
    for raw in titles {
        let title = raw.trim();
        if title.is_empty() || title.chars().count() > MAX_SUBTASK_CHARS {
            return None;
        }
        if !seen.insert(title.to_lowercase()) {
            return None;
        }
        steps.push(title.to_string());
    }
    Some(steps)
}

/// Ask the model to decompose every candidate in one request.
///
/// The reply is filtered down to ids that were actually asked about, and
/// each parent's steps are validated independently. Invalid parents are
/// simply absent from the result, so the caller can fall back per parent.
pub fn propose_decompositions<C: Completer>(
    completer: &C,
    candidates: &[&Task],
) -> std::result::Result<BTreeMap<String, Vec<String>>, String> {
    if candidates.is_empty() {
        return Ok(BTreeMap::new());
    }
    let (system, user) = decomposition_prompt(candidates);
    let text = completer.complete(&system, &user)?;
    let raw: BTreeMap<String, Vec<String>> = serde_json::from_str(strip_code_fence(&text))
        .map_err(|error| format!("invalid decomposition reply: {error}"))?;
    let mut accepted = BTreeMap::new();
    for (task_id, titles) in raw {
        if !candidates.iter().any(|task| task.id == task_id) {
            continue;
        }
        if let Some(steps) = validate_subtasks(&titles) {
            accepted.insert(task_id, steps);
        }
    }
    Ok(accepted)
}

/// Select undated tasks the fill stage may schedule.
///
/// Pinned tasks are left alone because filling a date is a move. Recurring
/// tasks derive their own next occurrence. Decomposed parents have already
/// handed their scheduling to their children and must not be re-dated.
pub fn undated_fill_candidates(tasks: &[Task]) -> Vec<&Task> {
    tasks
        .iter()
        .filter(|task| {
            task.status == Status::Pending
                && !task.pinned
                && task.decomposed_at.is_none()
                && task.recurrence_pattern.is_none()
                && task.due_date.is_none()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Priority, Recurrence};
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
            due_time: None,
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

    fn counts(entries: &[(NaiveDate, usize)]) -> BTreeMap<NaiveDate, usize> {
        entries.iter().copied().collect()
    }

    #[test]
    fn next_open_day_skips_full_days() {
        let today = day(2026, 1, 5);
        let map = counts(&[(day(2026, 1, 5), 3), (day(2026, 1, 6), 3)]);
        let next = next_open_day(today, today, &map, 3, 14);
        assert_eq!(next, Some(day(2026, 1, 7)));
    }

    #[test]
    fn next_open_day_respects_horizon_exhaustion() {
        let today = day(2026, 1, 5);
        // Every day through the horizon is full.
        let mut map = BTreeMap::new();
        for offset in 0..=3 {
            map.insert(today + Duration::days(offset), 3);
        }
        let next = next_open_day(today, today, &map, 3, 3);
        assert_eq!(next, None);
    }

    #[test]
    fn next_open_day_never_moves_into_the_past() {
        let today = day(2026, 1, 10);
        // A stale day in the past is full; the search starts at today instead.
        let map = counts(&[(day(2026, 1, 1), 3)]);
        let next = next_open_day(day(2026, 1, 1), today, &map, 3, 14);
        assert_eq!(next, Some(today));
    }

    #[test]
    fn next_open_day_finds_capacity_after_gap() {
        let today = day(2026, 1, 5);
        let map = counts(&[(day(2026, 1, 5), 3)]);
        // Day 6 has no entry, so it is open (zero load).
        let next = next_open_day(today, today, &map, 3, 14);
        assert_eq!(next, Some(day(2026, 1, 6)));
    }

    #[allow(clippy::too_many_arguments)]
    fn candidate_task(
        id: &str,
        due: Option<NaiveDate>,
        pinned: bool,
        recurrence: Option<Recurrence>,
        status: Status,
    ) -> Task {
        Task {
            id: id.into(),
            title: id.into(),
            description: None,
            notes: None,
            status,
            priority: Priority::Medium,
            category: None,
            due_date: due,
            due_time: None,
            recurrence_pattern: recurrence,
            parent_task_id: None,
            notified_at: None,
            completed_at: None,
            last_completed_at: None,
            decomposed_at: None,
            pinned,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            tags: vec![],
        }
    }

    #[test]
    fn overdue_pending_task_is_a_candidate() {
        let today = day(2026, 1, 10);
        let tasks = vec![candidate_task(
            "t1",
            Some(day(2026, 1, 9)),
            false,
            None,
            Status::Pending,
        )];
        assert_eq!(reschedule_candidates(&tasks, today).len(), 1);
    }

    #[test]
    fn pinned_task_is_not_a_candidate() {
        let today = day(2026, 1, 10);
        let tasks = vec![candidate_task(
            "t1",
            Some(day(2026, 1, 9)),
            true,
            None,
            Status::Pending,
        )];
        assert!(reschedule_candidates(&tasks, today).is_empty());
    }

    #[test]
    fn recurring_task_is_not_a_candidate() {
        let today = day(2026, 1, 10);
        let tasks = vec![candidate_task(
            "t1",
            Some(day(2026, 1, 9)),
            false,
            Some(Recurrence::Daily),
            Status::Pending,
        )];
        assert!(reschedule_candidates(&tasks, today).is_empty());
    }

    #[test]
    fn future_task_is_not_a_candidate() {
        let today = day(2026, 1, 10);
        let tasks = vec![candidate_task(
            "t1",
            Some(day(2026, 1, 11)),
            false,
            None,
            Status::Pending,
        )];
        assert!(reschedule_candidates(&tasks, today).is_empty());
    }

    #[test]
    fn completed_task_is_not_a_candidate() {
        let today = day(2026, 1, 10);
        let tasks = vec![candidate_task(
            "t1",
            Some(day(2026, 1, 9)),
            false,
            None,
            Status::Completed,
        )];
        assert!(reschedule_candidates(&tasks, today).is_empty());
    }

    #[test]
    fn undated_task_is_not_a_reschedule_candidate() {
        let today = day(2026, 1, 10);
        let tasks = vec![candidate_task("t1", None, false, None, Status::Pending)];
        assert!(reschedule_candidates(&tasks, today).is_empty());
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
        assert_eq!(groups.get(&(d1, None)).unwrap().len(), 2);
        assert!(!groups.contains_key(&(d2, None)));

        // Completed tasks are not part of a collision.
        tasks[1].status = Status::Completed;
        let groups = collision_groups(&tasks);
        assert!(groups.is_empty());
    }

    #[test]
    fn collision_groups_separates_tasks_by_time_of_day() {
        let d1 = day(2026, 1, 5);
        let nine = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        let ten = NaiveTime::from_hms_opt(10, 0, 0).unwrap();
        let mut a = task("a", crate::model::Priority::High, "2026-01-01", d1, false);
        a.due_time = Some(nine);
        let mut b = task("b", crate::model::Priority::Low, "2026-01-02", d1, false);
        b.due_time = Some(nine);
        let mut c = task("c", crate::model::Priority::Medium, "2026-01-03", d1, false);
        c.due_time = Some(ten);
        let tasks = vec![a.clone(), b.clone(), c];

        // a and b share 09:00; c is at 10:00 on the same day and must not join.
        let groups = collision_groups(&tasks);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups.get(&(d1, Some(nine))).unwrap().len(), 2);
        assert!(!groups.contains_key(&(d1, Some(ten))));

        // A date-only task is its own group: it does not join a timed clash.
        let mut d = task("d", crate::model::Priority::Medium, "2026-01-04", d1, false);
        d.due_time = None;
        let tasks = vec![a, b, d];
        let groups = collision_groups(&tasks);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups.get(&(d1, Some(nine))).unwrap().len(), 2);
        assert!(!groups.contains_key(&(d1, None)));
    }

    // --- stuck detection (5a.2) ---

    fn base_task(id: &str) -> Task {
        task(
            id,
            crate::model::Priority::Medium,
            "2026-01-01T00:00:00Z",
            day(2026, 1, 1),
            false,
        )
    }

    fn stuck(tasks: &[Task], counts: &BTreeMap<String, usize>, today: NaiveDate) -> Vec<String> {
        stuck_candidates(tasks, counts, today, 2, 2)
            .into_iter()
            .map(|t| t.id.clone())
            .collect()
    }

    #[test]
    fn overdue_task_is_stuck() {
        let today = day(2026, 1, 10);
        let mut t = base_task("a");
        t.due_date = Some(day(2026, 1, 1));
        assert_eq!(stuck(&[t], &BTreeMap::new(), today), vec!["a"]);
    }

    #[test]
    fn recently_overdue_task_is_not_stuck() {
        let today = day(2026, 1, 10);
        let mut t = base_task("a");
        t.due_date = Some(day(2026, 1, 9));
        assert!(stuck(&[t], &BTreeMap::new(), today).is_empty());
    }

    #[test]
    fn old_undated_task_is_stuck() {
        let today = day(2026, 1, 10);
        let mut t = base_task("a");
        t.due_date = None;
        t.created_at = "2026-01-01T00:00:00Z".into();
        assert_eq!(stuck(&[t], &BTreeMap::new(), today), vec!["a"]);
    }

    #[test]
    fn recent_undated_task_is_not_stuck() {
        let today = day(2026, 1, 10);
        let mut t = base_task("a");
        t.due_date = None;
        t.created_at = "2026-01-09T00:00:00Z".into();
        assert!(stuck(&[t], &BTreeMap::new(), today).is_empty());
    }

    #[test]
    fn repeatedly_rescheduled_task_is_stuck() {
        let today = day(2026, 1, 10);
        let mut t = base_task("a");
        t.due_date = Some(day(2026, 1, 9)); // not overdue on its own
        let mut counts = BTreeMap::new();
        counts.insert("a".to_string(), 2usize);
        assert_eq!(stuck(&[t], &counts, today), vec!["a"]);
    }

    #[test]
    fn one_reschedule_is_not_stuck() {
        let today = day(2026, 1, 10);
        let mut t = base_task("a");
        t.due_date = Some(day(2026, 1, 9));
        let mut counts = BTreeMap::new();
        counts.insert("a".to_string(), 1usize);
        assert!(stuck(&[t], &counts, today).is_empty());
    }

    #[test]
    fn pinned_task_is_never_stuck() {
        let today = day(2026, 1, 10);
        let mut t = base_task("a");
        t.pinned = true;
        assert!(stuck(&[t], &BTreeMap::new(), today).is_empty());
    }

    #[test]
    fn task_with_children_is_not_stuck() {
        let today = day(2026, 1, 10);
        let parent = base_task("a");
        let mut child = base_task("c");
        child.parent_task_id = Some("a".into());
        assert!(stuck(&[parent, child], &BTreeMap::new(), today).is_empty());
    }

    #[test]
    fn decomposed_task_is_not_stuck_again() {
        let today = day(2026, 1, 10);
        let mut t = base_task("a");
        t.decomposed_at = Some("2026-01-05T00:00:00Z".into());
        assert!(stuck(&[t], &BTreeMap::new(), today).is_empty());
    }

    #[test]
    fn child_task_is_not_stuck() {
        let today = day(2026, 1, 10);
        let mut t = base_task("c");
        t.parent_task_id = Some("a".into());
        assert!(stuck(&[t], &BTreeMap::new(), today).is_empty());
    }

    #[test]
    fn recurring_task_is_not_stuck() {
        let today = day(2026, 1, 10);
        let mut t = base_task("a");
        t.recurrence_pattern = Some(crate::model::Recurrence::Daily);
        assert!(stuck(&[t], &BTreeMap::new(), today).is_empty());
    }

    #[test]
    fn completed_task_is_not_stuck() {
        let today = day(2026, 1, 10);
        let mut t = base_task("a");
        t.status = Status::Completed;
        assert!(stuck(&[t], &BTreeMap::new(), today).is_empty());
    }

    #[test]
    fn reopened_task_ignores_reschedules_before_completion() {
        // The caller only counts reschedules after last_completed_at, so a
        // reopened task with no post-completion moves is not stuck.
        let today = day(2026, 1, 10);
        let mut t = base_task("a");
        t.due_date = Some(day(2026, 1, 9));
        t.last_completed_at = Some("2026-01-05T00:00:00Z".into());
        let counts = BTreeMap::new();
        assert!(stuck(&[t], &counts, today).is_empty());
    }

    #[test]
    fn stuck_candidates_order_longest_neglected_then_priority() {
        let today = day(2026, 1, 10);
        let mut older = base_task("older");
        older.due_date = Some(day(2025, 12, 1));
        older.priority = crate::model::Priority::Low;
        let mut newer = base_task("newer");
        newer.due_date = Some(day(2026, 1, 1));
        newer.priority = crate::model::Priority::High;
        let mut same_day_low = base_task("same-day-low");
        same_day_low.due_date = Some(day(2026, 1, 1));
        same_day_low.priority = crate::model::Priority::Low;
        let order = stuck(&[same_day_low, newer, older], &BTreeMap::new(), today);
        assert_eq!(order, vec!["older", "newer", "same-day-low"]);
    }

    // --- deterministic fallback (5a.4) ---

    #[test]
    fn fallback_splits_on_and() {
        assert_eq!(
            fallback_split("Clean kitchen and take out bins"),
            vec!["Clean kitchen", "take out bins"]
        );
    }

    #[test]
    fn fallback_splits_on_commas() {
        assert_eq!(
            fallback_split("Draft email, book room, send invite"),
            vec!["Draft email", "book room", "send invite"]
        );
    }

    #[test]
    fn fallback_truncates_to_three_parts() {
        let parts = fallback_split("a, b, c, d, e");
        assert_eq!(parts, vec!["a", "b", "c"]);
    }

    #[test]
    fn fallback_refuses_a_title_with_no_seam() {
        assert!(fallback_split("Write the annual report").is_empty());
    }

    #[test]
    fn fallback_drops_empty_parts() {
        assert_eq!(fallback_split("one, , and two"), vec!["one", "two"]);
    }

    #[test]
    fn skip_backoff_blocks_only_recent_skips() {
        let today = day(2026, 1, 10);
        assert!(skip_blocks_retry(Some("2026-01-08T00:00:00Z"), today, 7));
        assert!(!skip_blocks_retry(Some("2026-01-01T00:00:00Z"), today, 7));
        assert!(!skip_blocks_retry(None, today, 7));
    }

    // --- batched LLM decomposition (5a.3) ---

    struct FakeCompleter {
        reply: String,
        calls: std::cell::Cell<usize>,
    }

    impl FakeCompleter {
        fn new(reply: &str) -> Self {
            Self {
                reply: reply.to_string(),
                calls: std::cell::Cell::new(0),
            }
        }
    }

    impl Completer for FakeCompleter {
        fn complete(&self, _system: &str, _user: &str) -> std::result::Result<String, String> {
            self.calls.set(self.calls.get() + 1);
            Ok(self.reply.clone())
        }
    }

    #[test]
    fn several_candidates_make_one_request() {
        let a = base_task("a");
        let b = base_task("b");
        let fake = FakeCompleter::new(r#"{"a":["one","two"],"b":["three"]}"#);
        let result = propose_decompositions(&fake, &[&a, &b]).unwrap();
        assert_eq!(fake.calls.get(), 1);
        assert_eq!(result.get("a").unwrap(), &vec!["one", "two"]);
        assert_eq!(result.get("b").unwrap(), &vec!["three"]);
    }

    #[test]
    fn no_candidates_means_no_request() {
        let fake = FakeCompleter::new("{}");
        let result = propose_decompositions(&fake, &[]).unwrap();
        assert!(result.is_empty());
        assert_eq!(fake.calls.get(), 0);
    }

    #[test]
    fn duplicate_titles_reject_that_parent_only() {
        let a = base_task("a");
        let b = base_task("b");
        let fake = FakeCompleter::new(r#"{"a":["step","STEP"],"b":["ok"]}"#);
        let result = propose_decompositions(&fake, &[&a, &b]).unwrap();
        assert!(!result.contains_key("a"));
        assert!(result.contains_key("b"));
    }

    #[test]
    fn oversized_title_rejects_that_parent() {
        let a = base_task("a");
        let long = "x".repeat(121);
        let fake = FakeCompleter::new(&format!(r#"{{"a":["ok","{long}"]}}"#));
        let result = propose_decompositions(&fake, &[&a]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn too_many_subtasks_rejects_that_parent() {
        let a = base_task("a");
        let fake = FakeCompleter::new(r#"{"a":["1","2","3","4","5","6"]}"#);
        let result = propose_decompositions(&fake, &[&a]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let a = base_task("a");
        let fake = FakeCompleter::new(r#"{"ghost":["x"],"a":["real"]}"#);
        let result = propose_decompositions(&fake, &[&a]).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("a"));
    }

    #[test]
    fn markdown_fence_is_tolerated() {
        let a = base_task("a");
        let fake = FakeCompleter::new("```json\n{\"a\":[\"one\"]}\n```");
        let result = propose_decompositions(&fake, &[&a]).unwrap();
        assert_eq!(result.get("a").unwrap(), &vec!["one"]);
    }

    // --- decomposed-parent exclusions (5a.7) ---

    #[test]
    fn decomposed_parent_is_not_rescheduled() {
        let today = day(2026, 1, 10);
        let mut t = candidate_task("p", Some(day(2026, 1, 1)), false, None, Status::Pending);
        t.decomposed_at = Some("2026-01-02T00:00:00Z".into());
        assert!(reschedule_candidates(&[t], today).is_empty());
    }

    #[test]
    fn decomposed_parent_is_not_filled() {
        let mut t = candidate_task("p", None, false, None, Status::Pending);
        t.decomposed_at = Some("2026-01-02T00:00:00Z".into());
        assert!(undated_fill_candidates(&[t]).is_empty());
    }

    #[test]
    fn open_undated_task_is_filled() {
        let t = candidate_task("u", None, false, None, Status::Pending);
        assert_eq!(undated_fill_candidates(&[t]).len(), 1);
    }
}
