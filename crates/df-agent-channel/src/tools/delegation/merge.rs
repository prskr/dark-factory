//! `merge_subtask_pr`: the parent's agent merges the pull request of a subtask that builds on its
//! branch (`task-tree` "Merging subtask PRs", "Merge outcome"; design D7).

use df_core::{IntegrationError, SubtaskMerge, TaskKey, TaskStatus};
use df_db::git_pushes::last_accepted_head;
use df_db::tasks::{PR_SYNC_KIND, PrSyncJob};
use df_db::webhooks::complete_on_merge_as_tx;
use df_db::{lock_task_status, subtask_to_merge};
use serde_json::{Value, json};
use sqlx::PgPool;

use super::{Outcome, agent, text_arg};
use crate::ChannelContext;
use crate::error::ChannelError;
use crate::tools::forge_error;
use df_orchestrator::run_context::RunContext;

/// What the checks found out about the subtask: its PR and the head to merge.
pub(super) struct Mergeable {
    pub(super) task: df_core::TaskId,
    pub(super) number: u64,
    pub(super) url: String,
    pub(super) head: String,
}

pub(in crate::tools) async fn merge_subtask_pr(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let key = text_arg(args, "task").unwrap_or_default();
    let mergeable = match check(&ctx.pool, rc, &key).await? {
        Ok(mergeable) => mergeable,
        Err(problem) => return Ok(Outcome::error(problem)),
    };
    // The forge call runs without a transaction or row lock.
    if let Err(error) = ctx
        .forge
        .merge_pull_request(&rc.project.id, mergeable.number, &mergeable.head)
        .await
    {
        return Ok(match error {
            IntegrationError::Conflict(reason) | IntegrationError::InvalidInput(reason) => {
                Outcome::error(format!(
                    "the forge did not merge the pull request of {key}: {reason}; nothing changed"
                ))
            }
            other => forge_error(&other),
        });
    }
    let not_done = record_merge(ctx, rc, &mergeable).await?;
    let mut outcome = Outcome::ok(merged_text(&key, &mergeable.url, not_done));
    outcome.structured = Some(json!({ "task": key, "url": mergeable.url }));
    Ok(outcome)
}

/// The result text of a merge; `not_done` is the final status the subtask had when the merge was
/// recorded, if it could not be marked done.
pub(super) fn merged_text(key: &str, url: &str, not_done: Option<TaskStatus>) -> String {
    match not_done {
        None => format!("Merged {key}: {url}. {key} is done."),
        Some(TaskStatus::Cancelled) => {
            format!("Merged {key}: {url}. {key} was cancelled meanwhile and stays cancelled.")
        }
        Some(_) => format!("Merged {key}: {url}. {key} is already done."),
    }
}

/// The subtask's PR and head if the call is allowed; otherwise the one condition that fails.
async fn check(
    pool: &PgPool,
    rc: &RunContext,
    key: &str,
) -> Result<Result<Mergeable, String>, ChannelError> {
    let not_a_subtask = || {
        Err(format!(
            "`{key}` is not a subtask of this task; you can merge only the pull requests of tasks you created"
        ))
    };
    let Ok(parsed) = key.parse::<TaskKey>() else {
        return Ok(not_a_subtask());
    };
    let me = rc.task.id;
    let found = subtask_to_merge(pool, rc.project.id, &parsed).await?;
    let Some(subtask) = found.filter(|s| s.parent_id == Some(me)) else {
        return Ok(not_a_subtask());
    };
    if subtask.policy != SubtaskMerge::ParentAgent {
        return Ok(Err(
            "merging subtask pull requests is not allowed in this project: its `subtask_merge` setting is `human`; a board member merges them"
                .into(),
        ));
    }
    if subtask.base_task_id != Some(me) {
        return Ok(Err(format!(
            "`{key}` is not based on this task: only subtasks whose base task is this task (iterations on your branch) can be merged by you"
        )));
    }
    if subtask.status != TaskStatus::InReview {
        return Ok(Err(format!(
            "`{key}` is {}, not in_review: wait until its agent finished (you are woken when it ends)",
            subtask.status
        )));
    }
    let Some(number) = subtask.pr_number else {
        return Ok(Err(format!(
            "`{key}` has no pull request to merge: the control plane opens it shortly after the subtask's run ended, so call merge_subtask_pr again in a moment; a subtask that pushed nothing has none"
        )));
    };
    let Some(head) = last_accepted_head(pool, subtask.id).await? else {
        return Ok(Err(format!(
            "no push of `{key}` was recorded, so there is no commit to merge"
        )));
    };
    Ok(Ok(Mergeable {
        task: subtask.id,
        number,
        url: subtask.pr_url.unwrap_or_default(),
        head,
    }))
}

/// Records the merge as the webhook would (`pr_merged_at`, done, dependents released, the caller
/// woken) with the calling agent as actor, and asks for the parent's own draft PR: its branch is
/// ahead of the base now. A forge failure of that PR is retried by the outbox, never reported
/// here.
///
/// The run rows and the subtask row are locked (in this order) and its status read again: the
/// checks ran before the forge call. If the subtask got `completed` or `cancelled` meanwhile it is
/// left as it is (a completed one is done already, a cancelled one stays cancelled) and the result
/// is its status.
pub(super) async fn record_merge(
    ctx: &ChannelContext,
    rc: &RunContext,
    merged: &Mergeable,
) -> Result<Option<TaskStatus>, ChannelError> {
    let mut tx = ctx.pool.begin().await?;
    let actor = agent(rc);
    let status = lock_task_status(&mut tx, merged.task).await?;
    let not_done = status.filter(|s| TaskStatus::CLOSED.contains(s));
    if not_done.is_none() {
        complete_on_merge_as_tx(&mut tx, merged.task, &rc.persona.name, &actor).await?;
    }
    let job = PrSyncJob {
        task_id: rc.task.id,
        run_id: Some(rc.run.id),
        open: true,
        ready: None,
    };
    df_db::outbox::enqueue(&mut tx, PR_SYNC_KIND, &serde_json::to_value(job)?).await?;
    tx.commit().await?;
    Ok(not_done)
}
