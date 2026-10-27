//! Status badges and chips.

use df_core::{RunState, TaskStatus};
use maud::{Markup, html};

const BADGE: &str =
    "inline-flex shrink-0 items-center rounded-full px-2 py-0.5 text-xs font-medium";

/// Color family of a badge; each has a background and a foreground token pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Running,
    Waiting,
    Queued,
    Completed,
    Failed,
    /// Rate limited or limit reached.
    Limited,
    /// The agent is done and the PR waits for review.
    Review,
}

impl Tone {
    const fn class(self) -> &'static str {
        match self {
            Self::Running => "bg-status-running-bg text-status-running-fg",
            Self::Waiting => "bg-status-waiting-bg text-status-waiting-fg",
            Self::Queued => "bg-status-queued-bg text-status-queued-fg",
            Self::Completed => "bg-status-completed-bg text-status-completed-fg",
            Self::Failed => "bg-status-failed-bg text-status-failed-fg",
            Self::Limited => "bg-status-limited-bg text-status-limited-fg",
            Self::Review => "bg-status-review-bg text-status-review-fg",
        }
    }

    /// Background of a status dot: the foreground token of the tone.
    pub const fn dot_class(self) -> &'static str {
        match self {
            Self::Running => "bg-status-running-fg",
            Self::Waiting => "bg-status-waiting-fg",
            Self::Queued => "bg-status-queued-fg",
            Self::Completed => "bg-status-completed-fg",
            Self::Failed => "bg-status-failed-fg",
            Self::Limited => "bg-status-limited-fg",
            Self::Review => "bg-status-review-fg",
        }
    }

    /// Tone of a task status.
    pub const fn of_status(status: TaskStatus) -> Self {
        match status {
            TaskStatus::Queued => Self::Queued,
            TaskStatus::Running => Self::Running,
            TaskStatus::Waiting => Self::Waiting,
            TaskStatus::InReview => Self::Review,
            TaskStatus::Completed => Self::Completed,
            TaskStatus::RateLimited | TaskStatus::LimitReached => Self::Limited,
            TaskStatus::Failed => Self::Failed,
        }
    }

    /// Tone of a run state.
    pub const fn of_run(state: RunState) -> Self {
        match state {
            RunState::Running | RunState::Finalizing => Self::Running,
            RunState::Succeeded => Self::Completed,
            RunState::Failed => Self::Failed,
            RunState::AwaitingInput | RunState::Blocked => Self::Waiting,
            RunState::Pending
            | RunState::Provisioning
            | RunState::Connecting
            | RunState::Preparing
            | RunState::Cancelled => Self::Queued,
        }
    }
}

/// A badge with an arbitrary label.
pub fn tone_badge(tone: Tone, label: &str) -> Markup {
    html! { span class={(BADGE) " " (tone.class())} { (label) } }
}

/// The badge of a task status: the status name without underscores in the status's tone.
pub fn status_badge(status: TaskStatus) -> Markup {
    tone_badge(Tone::of_status(status), &status.as_str().replace('_', " "))
}

/// A harness by its product name, shown for a persona.
pub fn harness_label(harness: &str) -> &str {
    match harness {
        "claude-code" => "Claude Code",
        other => other,
    }
}

/// A neutral label, for example the project or persona of a task.
pub fn chip(label: &str) -> Markup {
    html! { span class="inline-flex items-center rounded-full bg-sunk px-2.5 py-0.5 text-xs text-muted" { (label) } }
}

/// A filter choice: a link that is marked as current when `active`.
pub fn filter_chip(label: &str, href: &str, active: bool) -> Markup {
    let class = if active {
        "bg-accent-soft text-accent-text border-accent-text font-medium"
    } else {
        "bg-panel text-muted border-line hover:bg-sunk"
    };
    html! {
        a href=(href) aria-current=[active.then_some("true")]
            class={"inline-flex min-h-11 items-center rounded-full border px-3 text-sm lg:min-h-8 " (class)} { (label) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_has_its_own_tone_colors() {
        let expected = [
            (TaskStatus::Queued, "status-queued"),
            (TaskStatus::Running, "status-running"),
            (TaskStatus::Waiting, "status-waiting"),
            (TaskStatus::InReview, "status-review"),
            (TaskStatus::Completed, "status-completed"),
            (TaskStatus::RateLimited, "status-limited"),
            (TaskStatus::LimitReached, "status-limited"),
            (TaskStatus::Failed, "status-failed"),
        ];
        assert_eq!(expected.len(), TaskStatus::ALL.len());
        for (status, token) in expected {
            let html = status_badge(status).into_string();
            assert!(
                html.contains(&format!("bg-{token}-bg text-{token}-fg")),
                "{html}"
            );
            assert!(html.contains(&format!(">{}<", status.as_str().replace('_', " "))));
        }
    }

    #[test]
    fn the_harness_is_shown_by_its_product_name() {
        assert_eq!(harness_label("claude-code"), "Claude Code");
        assert_eq!(harness_label("other"), "other");
    }

    #[test]
    fn run_states_map_to_tones() {
        assert_eq!(Tone::of_run(RunState::Succeeded), Tone::Completed);
        assert_eq!(Tone::of_run(RunState::Failed), Tone::Failed);
        assert_eq!(Tone::of_run(RunState::AwaitingInput), Tone::Waiting);
        assert_eq!(Tone::of_run(RunState::Cancelled), Tone::Queued);
    }

    #[test]
    fn filter_chip_marks_the_current_choice_and_is_touch_sized() {
        let on = filter_chip("Active", "/tasks", true).into_string();
        let off = filter_chip("Done", "/tasks?status=done", false).into_string();
        assert!(on.contains("aria-current=\"true\"") && !off.contains("aria-current"));
        assert!(on.contains("min-h-11"));
        assert!(chip("DF").into_string().contains(">DF<"));
    }
}
