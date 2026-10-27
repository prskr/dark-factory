//! Domain status vocabularies (`TaskStatus`, `RunState`, `RunOutcome`) and `Principal`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ids::{PersonaId, RunId, TaskId, UserId};

/// Error parsing a domain enum from a string.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid {kind} `{value}`")]
pub struct ParseEnumError {
    pub kind: &'static str,
    pub value: String,
}

/// Coarse grouping of task statuses used by the task list filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusGroup {
    /// Queued, running or rate limited: the work is moving without a human.
    Active,
    /// Waiting or limit reached: a human has to act.
    Waiting,
    /// Completed or failed.
    Done,
}

/// The sets of statuses the task list, its live updates and the project counters select: the
/// coarse [`StatusGroup`]s plus two narrower sets that only links select (`queued`, `running`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusSet {
    /// Queued, running, rate limited.
    Active,
    /// Waiting, limit reached.
    Waiting,
    /// Completed, failed.
    Done,
    /// Queued only.
    Queued,
    /// Running and rate limited: the agent is working or about to continue on its own.
    Running,
}

impl StatusSet {
    /// The statuses of the set.
    pub const fn statuses(self) -> &'static [TaskStatus] {
        match self {
            Self::Active => &[
                TaskStatus::Queued,
                TaskStatus::Running,
                TaskStatus::RateLimited,
            ],
            Self::Waiting => &[
                TaskStatus::Waiting,
                TaskStatus::InReview,
                TaskStatus::LimitReached,
            ],
            Self::Done => &[TaskStatus::Completed, TaskStatus::Failed],
            Self::Queued => &[TaskStatus::Queued],
            Self::Running => &[TaskStatus::Running, TaskStatus::RateLimited],
        }
    }
}

impl From<StatusGroup> for StatusSet {
    fn from(group: StatusGroup) -> Self {
        match group {
            StatusGroup::Active => Self::Active,
            StatusGroup::Waiting => Self::Waiting,
            StatusGroup::Done => Self::Done,
        }
    }
}

/// Shared task status vocabulary (`tasks.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    Waiting,
    /// The agent called `complete_task`; the PR waits for review.
    InReview,
    /// A human marked the task done.
    Completed,
    RateLimited,
    LimitReached,
    Failed,
}

impl TaskStatus {
    pub const fn group(&self) -> StatusGroup {
        match self {
            Self::Queued | Self::Running | Self::RateLimited => StatusGroup::Active,
            Self::Waiting | Self::InReview | Self::LimitReached => StatusGroup::Waiting,
            Self::Completed | Self::Failed => StatusGroup::Done,
        }
    }

    /// Whether a board member may mark a task in this status done: not while a run works or is
    /// about to (`queued`, `running`) and not twice (`completed`).
    pub const fn can_mark_done(&self) -> bool {
        !matches!(self, Self::Queued | Self::Running | Self::Completed)
    }

    pub const ALL: &'static [Self] = &[
        Self::Queued,
        Self::Running,
        Self::Waiting,
        Self::InReview,
        Self::Completed,
        Self::RateLimited,
        Self::LimitReached,
        Self::Failed,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::InReview => "in_review",
            Self::Completed => "completed",
            Self::RateLimited => "rate_limited",
            Self::LimitReached => "limit_reached",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<String> for TaskStatus {
    type Error = ParseEnumError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl FromStr for TaskStatus {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "waiting" => Ok(Self::Waiting),
            "in_review" => Ok(Self::InReview),
            "completed" => Ok(Self::Completed),
            "rate_limited" => Ok(Self::RateLimited),
            "limit_reached" => Ok(Self::LimitReached),
            "failed" => Ok(Self::Failed),
            _ => Err(ParseEnumError {
                kind: "TaskStatus",
                value: s.to_string(),
            }),
        }
    }
}

/// Run lifecycle state (`runs.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Pending,
    Provisioning,
    Connecting,
    Preparing,
    Running,
    AwaitingInput,
    Finalizing,
    Succeeded,
    Blocked,
    Failed,
    Cancelled,
}

impl RunState {
    pub const ALL: &'static [Self] = &[
        Self::Pending,
        Self::Provisioning,
        Self::Connecting,
        Self::Preparing,
        Self::Running,
        Self::AwaitingInput,
        Self::Finalizing,
        Self::Succeeded,
        Self::Blocked,
        Self::Failed,
        Self::Cancelled,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Provisioning => "provisioning",
            Self::Connecting => "connecting",
            Self::Preparing => "preparing",
            Self::Running => "running",
            Self::AwaitingInput => "awaiting_input",
            Self::Finalizing => "finalizing",
            Self::Succeeded => "succeeded",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Returns `true` if the run has reached a terminal state.
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Blocked | Self::Failed | Self::Cancelled
        )
    }
}

impl fmt::Display for RunState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<String> for RunState {
    type Error = ParseEnumError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl FromStr for RunState {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "provisioning" => Ok(Self::Provisioning),
            "connecting" => Ok(Self::Connecting),
            "preparing" => Ok(Self::Preparing),
            "running" => Ok(Self::Running),
            "awaiting_input" => Ok(Self::AwaitingInput),
            "finalizing" => Ok(Self::Finalizing),
            "succeeded" => Ok(Self::Succeeded),
            "blocked" => Ok(Self::Blocked),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ParseEnumError {
                kind: "RunState",
                value: s.to_string(),
            }),
        }
    }
}

/// Run end outcome (`runs.outcome`), covering both runtime-reported outcomes
/// and control-plane-side failure outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Completed,
    EndTurn,
    Blocked,
    LimitReached,
    RateLimited,
    Cancelled,
    Failed,
    ProvisioningFailed,
    ConnectTimeout,
    SandboxLost,
}

impl RunOutcome {
    pub const ALL: &'static [Self] = &[
        Self::Completed,
        Self::EndTurn,
        Self::Blocked,
        Self::LimitReached,
        Self::RateLimited,
        Self::Cancelled,
        Self::Failed,
        Self::ProvisioningFailed,
        Self::ConnectTimeout,
        Self::SandboxLost,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::EndTurn => "end_turn",
            Self::Blocked => "blocked",
            Self::LimitReached => "limit_reached",
            Self::RateLimited => "rate_limited",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::ProvisioningFailed => "provisioning_failed",
            Self::ConnectTimeout => "connect_timeout",
            Self::SandboxLost => "sandbox_lost",
        }
    }
}

impl fmt::Display for RunOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RunOutcome {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "completed" => Ok(Self::Completed),
            "end_turn" => Ok(Self::EndTurn),
            "blocked" => Ok(Self::Blocked),
            "limit_reached" => Ok(Self::LimitReached),
            "rate_limited" => Ok(Self::RateLimited),
            "cancelled" => Ok(Self::Cancelled),
            "failed" => Ok(Self::Failed),
            "provisioning_failed" => Ok(Self::ProvisioningFailed),
            "connect_timeout" => Ok(Self::ConnectTimeout),
            "sandbox_lost" => Ok(Self::SandboxLost),
            _ => Err(ParseEnumError {
                kind: "RunOutcome",
                value: s.to_string(),
            }),
        }
    }
}

/// Identifies who performed an action or authored a prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Principal {
    Human {
        user_id: UserId,
        display_name: String,
    },
    Agent {
        persona_id: PersonaId,
        display_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        run_id: Option<RunId>,
        task_id: TaskId,
    },
    System {
        reason: String,
    },
}

impl Principal {
    /// Creates a human principal.
    pub fn human(user_id: UserId, display_name: impl Into<String>) -> Self {
        Self::Human {
            user_id,
            display_name: display_name.into(),
        }
    }

    /// Creates an agent principal.
    pub fn agent(
        persona_id: PersonaId,
        display_name: impl Into<String>,
        run_id: Option<RunId>,
        task_id: TaskId,
    ) -> Self {
        Self::Agent {
            persona_id,
            display_name: display_name.into(),
            run_id,
            task_id,
        }
    }

    /// Creates a system principal with a machine-readable reason (e.g. `"task.assigned"`).
    pub fn system(reason: impl Into<String>) -> Self {
        Self::System {
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_status_string_and_serde_roundtrip() {
        let expected = [
            (TaskStatus::Queued, "queued"),
            (TaskStatus::Running, "running"),
            (TaskStatus::Waiting, "waiting"),
            (TaskStatus::InReview, "in_review"),
            (TaskStatus::Completed, "completed"),
            (TaskStatus::RateLimited, "rate_limited"),
            (TaskStatus::LimitReached, "limit_reached"),
            (TaskStatus::Failed, "failed"),
        ];
        assert_eq!(expected.len(), TaskStatus::ALL.len());

        for (status, s) in expected {
            assert_eq!(status.as_str(), s);
            assert_eq!(status.to_string(), s);
            assert_eq!(TaskStatus::from_str(s).unwrap(), status);

            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, format!("\"{s}\""));
            let parsed: TaskStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }

        assert!(TaskStatus::from_str("unknown").is_err());
    }

    #[test]
    fn status_sets_select_the_statuses_of_the_filters_and_counters() {
        use TaskStatus::*;
        assert_eq!(StatusSet::Queued.statuses(), [Queued]);
        assert_eq!(StatusSet::Running.statuses(), [Running, RateLimited]);
        assert_eq!(
            StatusSet::Waiting.statuses(),
            [Waiting, InReview, LimitReached]
        );
        assert_eq!(StatusSet::Done.statuses(), [Completed, Failed]);
        // The groups are made of the statuses `TaskStatus::group` assigns to them.
        for group in [StatusGroup::Active, StatusGroup::Waiting, StatusGroup::Done] {
            let set = StatusSet::from(group);
            let by_group: Vec<_> = TaskStatus::ALL
                .iter()
                .filter(|s| s.group() == group)
                .collect();
            assert_eq!(set.statuses().len(), by_group.len(), "{group:?}");
            assert!(
                set.statuses().iter().all(|s| s.group() == group),
                "{group:?}"
            );
        }
    }

    #[test]
    fn only_idle_or_ended_statuses_can_be_marked_done() {
        use TaskStatus::*;
        for status in [Waiting, InReview, RateLimited, LimitReached, Failed] {
            assert!(status.can_mark_done(), "{status}");
        }
        for status in [Queued, Running, Completed] {
            assert!(!status.can_mark_done(), "{status}");
        }
    }

    #[test]
    fn every_status_has_a_group() {
        use StatusGroup::*;
        let expected = [
            (TaskStatus::Queued, Active),
            (TaskStatus::Running, Active),
            (TaskStatus::RateLimited, Active),
            (TaskStatus::Waiting, Waiting),
            (TaskStatus::InReview, Waiting),
            (TaskStatus::LimitReached, Waiting),
            (TaskStatus::Completed, Done),
            (TaskStatus::Failed, Done),
        ];
        assert_eq!(expected.len(), TaskStatus::ALL.len());
        for (status, group) in expected {
            assert_eq!(status.group(), group, "{status}");
        }
    }

    #[test]
    fn run_state_string_and_terminal_checks() {
        let expected = [
            (RunState::Pending, "pending", false),
            (RunState::Provisioning, "provisioning", false),
            (RunState::Connecting, "connecting", false),
            (RunState::Preparing, "preparing", false),
            (RunState::Running, "running", false),
            (RunState::AwaitingInput, "awaiting_input", false),
            (RunState::Finalizing, "finalizing", false),
            (RunState::Succeeded, "succeeded", true),
            (RunState::Blocked, "blocked", true),
            (RunState::Failed, "failed", true),
            (RunState::Cancelled, "cancelled", true),
        ];
        assert_eq!(expected.len(), RunState::ALL.len());

        for (state, s, terminal) in expected {
            assert_eq!(state.as_str(), s);
            assert_eq!(state.to_string(), s);
            assert_eq!(state.is_terminal(), terminal);
            assert_eq!(RunState::from_str(s).unwrap(), state);

            let json = serde_json::to_string(&state).unwrap();
            assert_eq!(json, format!("\"{s}\""));
            let parsed: RunState = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, state);
        }

        assert!(RunState::from_str("invalid").is_err());
    }

    #[test]
    fn run_outcome_string_and_serde_roundtrip() {
        let expected = [
            (RunOutcome::Completed, "completed"),
            (RunOutcome::EndTurn, "end_turn"),
            (RunOutcome::Blocked, "blocked"),
            (RunOutcome::LimitReached, "limit_reached"),
            (RunOutcome::RateLimited, "rate_limited"),
            (RunOutcome::Cancelled, "cancelled"),
            (RunOutcome::Failed, "failed"),
            (RunOutcome::ProvisioningFailed, "provisioning_failed"),
            (RunOutcome::ConnectTimeout, "connect_timeout"),
            (RunOutcome::SandboxLost, "sandbox_lost"),
        ];
        assert_eq!(expected.len(), RunOutcome::ALL.len());

        for (outcome, s) in expected {
            assert_eq!(outcome.as_str(), s);
            assert_eq!(outcome.to_string(), s);
            assert_eq!(RunOutcome::from_str(s).unwrap(), outcome);

            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(json, format!("\"{s}\""));
            let parsed: RunOutcome = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, outcome);
        }

        assert!(RunOutcome::from_str("bogus").is_err());
    }

    #[test]
    fn principal_json_roundtrip() {
        let principals = [
            Principal::human(UserId::new(), "Peter"),
            Principal::agent(PersonaId::new(), "Linus", Some(RunId::new()), TaskId::new()),
            Principal::system("task.assigned"),
        ];

        for p in principals {
            let json = serde_json::to_string(&p).unwrap();
            let decoded: Principal = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, p);
        }
    }
}
