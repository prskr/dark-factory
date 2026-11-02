//! Resume time and count of a task that waits for its next automatic run: the "Now" line, the
//! "Resumes" row of the details and the subtitle of the task row.

use chrono::{DateTime, Utc};
use df_core::{CAPACITY_DETAIL, RunOutcome, TaskStatus};
use df_db::task_views::{RunView, TaskDetail};

/// What the details panel shows about an automatic resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResumeNote {
    /// When the scheduler may start the next run; `None` if the task does not wait for one.
    pub at: Option<DateTime<Utc>>,
    /// Why the task waits, for example "rate limit".
    pub reason: &'static str,
    /// Consecutive automatic resumes after infrastructure failures.
    pub count: i32,
    /// `runs.maxAutoResumes`.
    pub max: u32,
}

impl ResumeNote {
    pub(crate) fn new(task: &TaskDetail, runs: &[RunView], max: u32) -> Self {
        // Runs are newest first.
        let last = runs.first();
        let outcome = last.and_then(|r| r.outcome.as_deref()?.parse::<RunOutcome>().ok());
        let reason = match (task.status, outcome) {
            (TaskStatus::RateLimited, _) => "rate limit",
            (_, Some(RunOutcome::SandboxLost)) => "sandbox lost",
            (_, Some(RunOutcome::ConnectTimeout)) => "sandbox did not connect",
            (_, Some(RunOutcome::ProvisioningFailed))
                if last.is_some_and(|r| r.detail == CAPACITY_DETAIL) =>
            {
                "no capacity"
            }
            (_, Some(RunOutcome::ProvisioningFailed)) => "provisioning failed",
            _ => "automatic resume",
        };
        Self {
            at: task.resume_at.filter(|_| task.status.waits_for_resume()),
            reason,
            count: task.auto_resumes,
            max,
        }
    }
}

/// A time of day, with the date when it is not on the day of `now` (UTC, like all times).
pub(super) fn clock_label(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    if at.date_naive() == now.date_naive() {
        at.format("%H:%M").to_string()
    } else {
        at.format("%b %d %H:%M").to_string()
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn the_date_is_shown_only_for_another_day() {
        let now = Utc.with_ymd_and_hms(2026, 11, 2, 23, 0, 0).unwrap();
        let same_day = Utc.with_ymd_and_hms(2026, 11, 2, 23, 30, 0).unwrap();
        let next_day = Utc.with_ymd_and_hms(2026, 11, 3, 4, 30, 0).unwrap();
        assert_eq!(clock_label(same_day, now), "23:30");
        assert_eq!(clock_label(next_day, now), "Nov 03 04:30");
    }
}
