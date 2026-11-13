//! Test helpers shared by the task tests and the webhook tests.

use chrono::{TimeDelta, Utc};
use df_core::task_transition::TaskEvent;
use df_core::{Principal, RunId, TaskId};
use sqlx::PgPool;

use super::{list_undelivered_messages, plain_text};

/// A `RunEnded` event that reports `run_id` as completed.
pub(crate) fn completed_run_ended(run_id: RunId) -> TaskEvent {
    TaskEvent::RunEnded(df_core::task_transition::RunEnded {
        run_id,
        outcome: df_core::RunOutcome::Completed,
        detail: String::new(),
        retry_after: None,
        task_branch_pushed: false,
        stop_requested_at: None,
        max_auto_resumes: 3,
        resume_backoff: TimeDelta::minutes(2),
        now: Utc::now(),
    })
}

/// The `task.child_ended` messages queued on `task`, as plain text.
pub(crate) async fn child_ended_messages(pool: &PgPool, task: TaskId) -> Vec<String> {
    list_undelivered_messages(pool, task)
        .await
        .unwrap()
        .into_iter()
        .filter(|m| m.author == Principal::system("task.child_ended"))
        .map(|m| plain_text(serde_json::to_value(&m.content).unwrap()).unwrap())
        .collect()
}
