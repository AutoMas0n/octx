use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

/// Task lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Completed,
    Cancelled,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Completed => "completed",
            Status::Cancelled => "cancelled",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Status::Pending),
            "completed" => Some(Status::Completed),
            "cancelled" => Some(Status::Cancelled),
            _ => None,
        }
    }
}

/// Task priority, ordered from most to least urgent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    High,
    Medium,
    Low,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Low => "low",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "high" => Some(Priority::High),
            "medium" => Some(Priority::Medium),
            "low" => Some(Priority::Low),
            _ => None,
        }
    }
}

/// Recurrence pattern applied when a task is completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Recurrence {
    Daily,
    Weekly,
    Monthly,
}

impl Recurrence {
    pub fn as_str(self) -> &'static str {
        match self {
            Recurrence::Daily => "daily",
            Recurrence::Weekly => "weekly",
            Recurrence::Monthly => "monthly",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "daily" => Some(Recurrence::Daily),
            "weekly" => Some(Recurrence::Weekly),
            "monthly" => Some(Recurrence::Monthly),
            _ => None,
        }
    }

    /// Advance a due date by one recurrence period, clamping to the last day of short months.
    pub fn advance(self, date: NaiveDate) -> NaiveDate {
        match self {
            Recurrence::Daily => date + chrono::Duration::days(1),
            Recurrence::Weekly => date + chrono::Duration::days(7),
            Recurrence::Monthly => add_one_month_clamped(date),
        }
    }
}

fn add_one_month_clamped(date: NaiveDate) -> NaiveDate {
    let (year, month) = if date.month() == 12 {
        (date.year() + 1, 1)
    } else {
        (date.year(), date.month() + 1)
    };
    let day = date.day();
    let last = days_in_month(year, month);
    NaiveDate::from_ymd_opt(year, month, day.min(last)).expect("clamped date is valid")
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => unreachable!("month is 1..=12"),
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// A stored task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub status: Status,
    pub priority: Priority,
    pub category: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub recurrence_pattern: Option<Recurrence>,
    pub parent_task_id: Option<String>,
    pub notified_at: Option<String>,
    pub completed_at: Option<String>,
    pub last_completed_at: Option<String>,
    pub decomposed_at: Option<String>,
    pub pinned: bool,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Fields supplied when creating a task.
#[derive(Debug, Clone, Default)]
pub struct NewTask {
    pub title: String,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub priority: Option<Priority>,
    pub category: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub recurrence_pattern: Option<Recurrence>,
    pub parent_task_id: Option<String>,
    pub pinned: bool,
    pub tags: Vec<String>,
}

/// Partial update. `Some` replaces a field; `None` leaves it unchanged.
#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub notes: Option<Option<String>>,
    pub status: Option<Status>,
    pub priority: Option<Priority>,
    pub category: Option<Option<String>>,
    pub due_date: Option<Option<NaiveDate>>,
    pub pinned: Option<bool>,
    pub decomposed_at: Option<Option<String>>,
}

/// Filters accepted by list queries.
#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub status: Option<Status>,
    pub category: Option<String>,
    pub parent_task_id: Option<String>,
    pub search: Option<String>,
    pub due_on_or_before: Option<NaiveDate>,
    pub limit: Option<usize>,
}

/// Aggregate counts for `stats`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    pub pending: i64,
    pub completed: i64,
    pub cancelled: i64,
    pub total: i64,
}

/// What the secretary did to a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionKind {
    Rescheduled,
    Skipped,
    Scheduled,
    Decomposed,
}

impl DecisionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DecisionKind::Rescheduled => "rescheduled",
            DecisionKind::Skipped => "skipped",
            DecisionKind::Scheduled => "scheduled",
            DecisionKind::Decomposed => "decomposed",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "rescheduled" => Some(DecisionKind::Rescheduled),
            "skipped" => Some(DecisionKind::Skipped),
            "scheduled" => Some(DecisionKind::Scheduled),
            "decomposed" => Some(DecisionKind::Decomposed),
            _ => None,
        }
    }
}

/// A recorded secretary decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub task_id: String,
    pub kind: DecisionKind,
    pub old_due_date: Option<NaiveDate>,
    pub new_due_date: Option<NaiveDate>,
    pub reason: String,
    /// JSON payload for subtask ids and similar structured data.
    pub details: Option<String>,
    pub created_at: String,
    pub undone_at: Option<String>,
    pub run_id: String,
}

impl Decision {
    /// True when the decision is still undoable.
    pub fn is_undone(&self) -> bool {
        self.undone_at.is_some()
    }
}

/// Fields supplied when recording a decision.
#[derive(Debug, Clone)]
pub struct NewDecision {
    pub task_id: String,
    pub kind: DecisionKind,
    pub old_due_date: Option<NaiveDate>,
    pub new_due_date: Option<NaiveDate>,
    pub reason: String,
    pub details: Option<String>,
    pub run_id: String,
}

/// A reschedule the policy layer proposes for one task.
///
/// Carried separately from [`NewDecision`] because applying one has to both
/// move the task and write the log row in the same transaction. `from` is the
/// due date the policy observed; the write is refused when the stored date no
/// longer matches, so a manual edit made mid-run is never silently clobbered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reschedule {
    pub task_id: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub reason: String,
}

/// A single secretary pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub decisions: i64,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_advances_one_day() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        assert_eq!(
            Recurrence::Daily.advance(d),
            NaiveDate::from_ymd_opt(2026, 11, 2).unwrap()
        );
    }

    #[test]
    fn weekly_advances_seven_days() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        assert_eq!(
            Recurrence::Weekly.advance(d),
            NaiveDate::from_ymd_opt(2026, 11, 8).unwrap()
        );
    }

    #[test]
    fn monthly_clamps_short_months() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
        assert_eq!(
            Recurrence::Monthly.advance(d),
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()
        );
    }

    #[test]
    fn monthly_clamps_leap_day() {
        let d = NaiveDate::from_ymd_opt(2024, 1, 31).unwrap();
        assert_eq!(
            Recurrence::Monthly.advance(d),
            NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()
        );
    }

    #[test]
    fn monthly_advances_year_boundary() {
        let d = NaiveDate::from_ymd_opt(2026, 12, 15).unwrap();
        assert_eq!(
            Recurrence::Monthly.advance(d),
            NaiveDate::from_ymd_opt(2027, 1, 15).unwrap()
        );
    }
}
