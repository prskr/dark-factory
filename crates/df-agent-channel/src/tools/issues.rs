//! The issue tools of a chat: `list_issues` and `create_issue` (spec `factory-tools`, design
//! "`list_issues`" and "`create_issue` raises an approval"). Both act on the project's repository
//! only; `create_issue` posts nothing itself, it raises a `create-issue` approval request.

use df_core::{Issue, IssueFilter, IssueState, IssueStateFilter, Principal};
use df_db::create_issue::CREATE_ISSUE;
use df_db::requests::{Raise, Raised, RequestError, raise_approval};
use serde_json::{Value, json};

use super::{Outcome, forge_error, text_arg};
use crate::ChannelContext;
use crate::error::ChannelError;
use df_orchestrator::run_context::RunContext;

/// Issues per page unless the call says otherwise.
const DEFAULT_LIMIT: u32 = 30;
/// Characters of an issue's body that `list_issues` shows.
const BODY_CHARS: usize = 200;

/// Lists the issues of the project's repository, never pull requests.
pub(super) async fn list_issues(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let filter = IssueFilter {
        state: match text_arg(args, "state").as_deref() {
            Some("closed") => IssueStateFilter::Closed,
            Some("all") => IssueStateFilter::All,
            _ => IssueStateFilter::Open,
        },
        labels: strings(args, "labels"),
        query: text_arg(args, "query"),
    };
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|limit| u32::try_from(limit).ok())
        .unwrap_or(DEFAULT_LIMIT);
    let cursor = text_arg(args, "cursor");
    let page = match ctx
        .forge
        .list_issues(&rc.project.id, &filter, limit, cursor.as_deref())
        .await
    {
        Ok(page) => page,
        Err(e) => return Ok(forge_error(&e)),
    };
    let listing = if page.issues.is_empty() {
        "No issues found.\n".to_string()
    } else {
        page.issues.iter().map(issue_text).collect::<String>()
    };
    let text = format!(
        "{listing}next_cursor: {}\ntruncated: {}",
        page.next_cursor.as_deref().unwrap_or("none"),
        page.truncated
    );
    let issues: Vec<Value> = page.issues.iter().map(issue_json).collect();
    let mut outcome = Outcome::ok(text);
    outcome.structured = Some(json!({
        "issues": issues,
        "next_cursor": page.next_cursor,
        "truncated": page.truncated,
    }));
    Ok(outcome)
}

/// Raises a non-blocking `create-issue` approval request with exactly the given text.
pub(super) async fn create_issue(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let actor = Principal::agent(
        rc.persona.id,
        rc.persona.name.clone(),
        Some(rc.run.id),
        rc.task.id,
    );
    let from = Raise {
        task_id: rc.task.id,
        run_id: Some(rc.run.id),
        actor: &actor,
        blocking: false,
    };
    let payload = json!({
        "title": text_arg(args, "title").unwrap_or_default(),
        "body": text_arg(args, "body").unwrap_or_default(),
        "labels": strings(args, "labels"),
    });
    let raised = raise_approval(
        &ctx.pool,
        &ctx.approval_types,
        from,
        CREATE_ISSUE,
        payload,
        "",
    )
    .await;
    Ok(match raised {
        Ok(Raised::Request { request, .. }) => Outcome {
            structured: Some(json!({ "id": request.id, "state": request.state.as_str() })),
            ..Outcome::ok(format!(
                "Approval request {} raised; the issue is created when a member approves. \
                 Continue your work: the decision arrives as a message, and after an approval \
                 a second message names the issue number.",
                request.id
            ))
        },
        Ok(Raised::AlreadyGranted) => Outcome::error("the request was not stored"),
        Err(
            e @ (RequestError::Invalid(_)
            | RequestError::TooManyPending
            | RequestError::Refused(_)
            | RequestError::UnknownType(_)),
        ) => Outcome::error(e.to_string()),
        Err(e) => return Err(e.into()),
    })
}

fn strings(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn excerpt(body: &str) -> String {
    body.chars().take(BODY_CHARS).collect()
}

fn state_name(state: IssueState) -> &'static str {
    match state {
        IssueState::Open => "open",
        IssueState::Closed => "closed",
    }
}

fn issue_text(issue: &Issue) -> String {
    let labels = if issue.labels.is_empty() {
        String::new()
    } else {
        format!(" [{}]", issue.labels.join(", "))
    };
    let body = excerpt(&issue.body)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "#{} ({}) {}{labels}\n  {}\n  {body}\n",
        issue.id.number,
        state_name(issue.state),
        issue.title,
        issue.url
    )
}

fn issue_json(issue: &Issue) -> Value {
    json!({
        "number": issue.id.number,
        "state": state_name(issue.state),
        "title": issue.title,
        "labels": issue.labels,
        "url": issue.url,
        "body": excerpt(&issue.body),
    })
}

#[cfg(test)]
mod tests;
