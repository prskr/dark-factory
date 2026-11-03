//! `read_pull_request_feedback` and `comment_on_pull_request` on the task's own pull request.

use chrono::{DateTime, FixedOffset};
use df_core::{Feedback, ReviewThread};
use serde_json::Value;

use super::{Outcome, forge_error, text_arg};
use crate::ChannelContext;
use crate::assign::RunContext;
use crate::error::ChannelError;

/// Upper bound of the serialized feedback result.
/// `ponytail:` fixed; make it configurable if agents with small context windows need less.
const FEEDBACK_BUDGET_BYTES: usize = 48 * 1024;

const NO_PULL_REQUEST: &str =
    "no pull request opened yet; it is opened after your first commits are pushed";

/// Returns the feedback on the task's pull request, newest items within [`FEEDBACK_BUDGET_BYTES`].
pub(super) async fn read_feedback(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let since = text_arg(args, "since");
    if since
        .as_deref()
        .is_some_and(|s| DateTime::parse_from_rfc3339(s).is_err())
    {
        return Ok(Outcome::error(
            "since must be an RFC 3339 timestamp such as 2026-11-03T08:00:00Z",
        ));
    }
    let Some(number) = pull_request_number(ctx, rc).await? else {
        return Ok(Outcome::error(NO_PULL_REQUEST));
    };
    let feedback = match ctx
        .forge
        .list_pull_request_feedback(&rc.project.id, number, since.as_deref())
        .await
    {
        Ok(feedback) => feedback,
        Err(e) => return Ok(forge_error(&e)),
    };
    let result = fit_to_budget(feedback, FEEDBACK_BUDGET_BYTES)?;
    Ok(Outcome {
        structured: Some(result.clone()),
        ..Outcome::ok(result.to_string())
    })
}

/// Posts a PR comment or, with `thread`, a reply in that review thread.
pub(super) async fn comment(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let Some(number) = pull_request_number(ctx, rc).await? else {
        return Ok(Outcome::error(NO_PULL_REQUEST));
    };
    let body = text_arg(args, "body").unwrap_or_default();
    // Top-level PR comments are issue comments on the PR number on every supported forge.
    let posted = match text_arg(args, "thread") {
        Some(thread) => {
            ctx.forge
                .reply_to_review_thread(&rc.project.id, number, &thread, &body)
                .await
        }
        None => {
            ctx.forge
                .add_issue_comment(&rc.project.id, number, &body)
                .await
        }
    };
    Ok(match posted {
        Ok(comment) => Outcome {
            structured: Some(serde_json::json!({ "id": comment.id, "url": comment.url })),
            ..Outcome::ok(format!("Comment added: {}", comment.url))
        },
        Err(e) => forge_error(&e),
    })
}

/// The task's recorded pull request, read fresh because it may be opened during the run.
async fn pull_request_number(
    ctx: &ChannelContext,
    rc: &RunContext,
) -> Result<Option<u64>, ChannelError> {
    let number: Option<i64> = sqlx::query_scalar("SELECT pr_number FROM tasks WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .fetch_one(&ctx.pool)
        .await?;
    Ok(number.and_then(|n| u64::try_from(n).ok()))
}

/// Which list of the result an item belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Comment,
    Review,
    Thread,
}

/// A PR comment, review or whole review thread, already encoded.
struct Item {
    kind: Kind,
    /// Position in the plugin's result, which the output keeps.
    position: usize,
    activity: Option<DateTime<FixedOffset>>,
    value: Value,
}

/// `feedback` as JSON, without its oldest items while it exceeds `budget` bytes.
fn fit_to_budget(feedback: Feedback, budget: usize) -> Result<Value, serde_json::Error> {
    let plugin_truncated = feedback.truncated;
    let mut items = Vec::new();
    let mut push = |kind, activity, value| {
        let position = items.len();
        items.push(Item {
            kind,
            position,
            activity,
            value,
        });
    };
    for c in feedback.comments {
        let activity = latest([&c.created_at, &c.updated_at]);
        push(Kind::Comment, activity, serde_json::to_value(c)?);
    }
    for r in feedback.reviews {
        let activity = latest([&r.submitted_at]);
        push(Kind::Review, activity, serde_json::to_value(r)?);
    }
    for t in feedback.threads {
        let activity = thread_activity(&t);
        push(Kind::Thread, activity, serde_json::to_value(t)?);
    }
    // Oldest first; unparsable timestamps (`None`) count as oldest.
    items.sort_by_key(|i| i.activity);

    let mut dropped = 0;
    loop {
        let kept = items.get(dropped..).unwrap_or_default();
        let result = assemble(kept, plugin_truncated || dropped > 0);
        if dropped >= items.len() || result.to_string().len() <= budget {
            return Ok(result);
        }
        dropped += 1;
    }
}

/// The result of the `kept` items, in the plugin's order.
fn assemble(kept: &[Item], truncated: bool) -> Value {
    let kept = |kind: Kind| -> Vec<Value> {
        let mut of_kind: Vec<&Item> = kept.iter().filter(|i| i.kind == kind).collect();
        of_kind.sort_by_key(|i| i.position);
        of_kind.into_iter().map(|i| i.value.clone()).collect()
    };
    serde_json::json!({
        "comments": kept(Kind::Comment),
        "reviews": kept(Kind::Review),
        "threads": kept(Kind::Thread),
        "truncated": truncated,
    })
}

/// Latest of RFC 3339 `timestamps` as an instant; unparsable ones are ignored.
fn latest<'a>(timestamps: impl IntoIterator<Item = &'a String>) -> Option<DateTime<FixedOffset>> {
    timestamps
        .into_iter()
        .filter_map(|t| DateTime::parse_from_rfc3339(t).ok())
        .max()
}

fn thread_activity(thread: &ReviewThread) -> Option<DateTime<FixedOffset>> {
    latest(
        thread
            .comments
            .iter()
            .flat_map(|c| [&c.created_at, &c.updated_at]),
    )
}

#[cfg(test)]
mod tests;
