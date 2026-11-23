//! Resume time and count of a task that waits for its next automatic run: the "Now" line, the
//! "Resumes" row of the details and the subtitle of the task row.

use df_core::{CAPACITY_DETAIL, RunOutcome, TaskStatus};
use df_db::task_views::{RunView, TaskDetail};
use jiff::Timestamp;
use jiff::tz::TimeZone;

/// What the details panel shows about an automatic resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResumeNote {
    /// When the scheduler may start the next run; `None` if the task does not wait for one.
    pub at: Option<Timestamp>,
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
pub(super) fn clock_label(at: Timestamp, now: Timestamp) -> String {
    if at.to_zoned_datetime(TimeZone::UTC).date() == now.to_zoned_datetime(TimeZone::UTC).date() {
        at.strftime("%H:%M").to_string()
    } else {
        at.strftime("%b %d %H:%M").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_date_is_shown_only_for_another_day() {
        let now: Timestamp = "2026-11-02T23:00:00Z".parse().unwrap();
        let same_day: Timestamp = "2026-11-02T23:30:00Z".parse().unwrap();
        let next_day: Timestamp = "2026-11-03T04:30:00Z".parse().unwrap();
        assert_eq!(clock_label(same_day, now), "23:30");
        assert_eq!(clock_label(next_day, now), "Nov 03 04:30");
    }
}
