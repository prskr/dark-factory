//! `read_pull_request_feedback` and `comment_on_pull_request` on the task's own pull request.

use chrono::{DateTime, Duration, FixedOffset, SecondsFormat, Utc};
use df_core::{Feedback, ReviewThread};
use serde_json::Value;

use super::{Outcome, forge_error, text_arg};
use crate::ChannelContext;
use crate::assign::RunContext;
use crate::error::ChannelError;

/// Upper bound of the serialized feedback result.
/// `ponytail:` fixed; make it configurable if agents with small context windows need less.
const FEEDBACK_BUDGET_BYTES: usize = 48 * 1024;

/// `next_since` lies this far before the newest activity returned. The plugin lists comments,
/// reviews and review comments in separate requests, so an item can appear between two of them. A
/// call is bounded by `plugins.<name>.callTimeoutSeconds`, which is at most 300 s.
const CURSOR_OVERLAP: Duration = Duration::minutes(5);

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
    let result = fit_to_budget(feedback, since.as_deref(), FEEDBACK_BUDGET_BYTES)?;
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

/// Which list of the result an item belongs to; the value indexes per-kind counts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Comment = 0,
    Review = 1,
    Thread = 2,
}

/// A PR comment, review or whole review thread, already encoded.
struct Item {
    kind: Kind,
    /// Position in the plugin's result; items with the same activity keep it.
    position: usize,
    activity: Option<DateTime<FixedOffset>>,
    value: Value,
    /// Encoded length plus the separating comma.
    size: usize,
}

/// `feedback` as JSON, newest activity first and without its oldest items while it exceeds `budget`
/// bytes, with the `next_since` cursor for a read that passed `since`.
fn fit_to_budget(
    feedback: Feedback,
    since: Option<&str>,
    budget: usize,
) -> Result<Value, serde_json::Error> {
    let plugin_truncated = feedback.truncated;
    let mut items = Vec::new();
    let mut push = |kind, activity, value: Value| {
        let position = items.len();
        let size = value.to_string().len() + 1;
        items.push(Item {
            kind,
            position,
            activity,
            value,
            size,
        });
    };
    for c in feedback.comments {
        let activity = latest([&c.created_at, &c.updated_at]);
        push(Kind::Comment, activity, serde_json::to_value(c)?);
    }
    for r in feedback.reviews {
        let activity = latest([&r.submitted_at, &r.updated_at]);
        push(Kind::Review, activity, serde_json::to_value(r)?);
    }
    for t in feedback.threads {
        let activity = thread_activity(&t);
        push(Kind::Thread, activity, serde_json::to_value(t)?);
    }
    // Oldest first; unparsable timestamps (`None`) count as oldest.
    items.sort_by_key(|i| i.activity);

    // The encoded length is the envelope plus each item and its comma, less one comma per
    // non-empty list. Drop the oldest items until that fits, then encode once. The newest kept
    // item is the last one, so the cursor only changes when everything is dropped.
    let cursor = |dropped: usize| {
        let newest = items.get(dropped..).and_then(<[Item]>::last);
        next_since(newest.and_then(|i| i.activity), since)
    };
    let envelope = |truncated, dropped| assemble(&[], truncated, cursor(dropped)).to_string().len();
    let mut size: usize = items.iter().map(|i| i.size).sum();
    let mut per_kind = [Kind::Comment, Kind::Review, Kind::Thread]
        .map(|kind| items.iter().filter(|i| i.kind == kind).count());
    let encoded = |size: usize, per_kind: &[usize; 3], truncated, dropped| {
        envelope(truncated, dropped) + size - per_kind.iter().filter(|n| **n > 0).count()
    };
    let mut dropped = 0;
    while let Some(oldest) = items.get(dropped) {
        if encoded(size, &per_kind, plugin_truncated || dropped > 0, dropped) <= budget {
            break;
        }
        size -= oldest.size;
        if let Some(n) = per_kind.get_mut(oldest.kind as usize) {
            *n -= 1;
        }
        dropped += 1;
    }
    let kept = items.get(dropped..).unwrap_or_default();
    Ok(assemble(
        kept,
        plugin_truncated || dropped > 0,
        cursor(dropped),
    ))
}

/// The result of the `kept` items, each list newest activity first (unparsable timestamps last).
fn assemble(kept: &[Item], truncated: bool, cursor: Option<String>) -> Value {
    let of_kind = |kind: Kind| -> Vec<Value> {
        let mut items: Vec<&Item> = kept.iter().filter(|i| i.kind == kind).collect();
        items.sort_by(|a, b| {
            b.activity
                .cmp(&a.activity)
                .then(a.position.cmp(&b.position))
        });
        items.into_iter().map(|i| i.value.clone()).collect()
    };
    serde_json::json!({
        "comments": of_kind(Kind::Comment),
        "reviews": of_kind(Kind::Review),
        "threads": of_kind(Kind::Thread),
        "truncated": truncated,
        "next_since": cursor,
    })
}

/// The cursor for the next read, in UTC: `newest` minus [`CURSOR_OVERLAP`], but never before
/// `since`. Without a newest activity it is `since`, or `None`.
fn next_since(newest: Option<DateTime<FixedOffset>>, since: Option<&str>) -> Option<String> {
    let cursor = newest.map(|n| n.with_timezone(&Utc) - CURSOR_OVERLAP);
    let since_utc = since
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|s| s.with_timezone(&Utc));
    // `AutoSi` keeps fractional seconds, so the cursor is never rounded below `since`.
    cursor
        .max(since_utc)
        .map(|c| c.to_rfc3339_opts(SecondsFormat::AutoSi, true))
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
