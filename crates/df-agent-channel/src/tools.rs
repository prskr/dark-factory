//! Factory tools v1: validation, scoping, exactly-once execution and the handlers
//! (`specs/factory-tools/spec.md`, design D6).

use std::collections::HashMap;
use std::sync::LazyLock;

use df_core::{IntegrationError, Principal, PullRequestUpdate, TitleSource, sanitize_title};
use df_db::create_issue::CREATE_ISSUE;
use df_db::requests::{
    Raise, Raised, RequestError, close_pending_for_task, raise_approval, raise_question,
};
use df_db::runs::set_completion_summary;
use df_db::task_updates::{set_pr_draft_fields, set_status_line};
use df_db::transcript::externalize_block;
use df_db::{
    NewAuditEntry, Notification, PgBlobStore, get_task, publish, set_task_title, tool_calls,
    write_audit_entry,
};
use df_proto::agent::v1::{ToolCallRequest, ToolCallResult};
use df_proto::transcript::v1::{BlobData, ContentBlock, TextContent, blob_data, content_block};
use jsonschema::Validator;
use serde_json::Value;

use crate::ChannelContext;
use crate::assign::{CHAT_ONLY_TOOLS, CHAT_TOOLS, tool_input_schemas};
use crate::error::ChannelError;
use df_orchestrator::OrchestratorError;
use df_orchestrator::run_context::RunContext;

mod artifacts;
mod ci;
mod delegation;
mod feedback;
mod issues;

static VALIDATORS: LazyLock<HashMap<&'static str, Validator>> = LazyLock::new(|| {
    tool_input_schemas()
        .map(|(name, schema)| {
            let validator = jsonschema::validator_for(schema)
                .expect("embedded tool schemas are valid JSON Schema");
            (name, validator)
        })
        .collect()
});

/// What a handler reports back to the model.
struct Outcome {
    is_error: bool,
    text: String,
    end_run_after_turn: bool,
    await_input_after_turn: bool,
    structured: Option<Value>,
    /// Content blocks replacing the single text block built from `text`.
    content: Option<Vec<ContentBlock>>,
}

impl Outcome {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            is_error: false,
            text: text.into(),
            end_run_after_turn: false,
            await_input_after_turn: false,
            structured: None,
            content: None,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            is_error: true,
            text: text.into(),
            end_run_after_turn: false,
            await_input_after_turn: false,
            structured: None,
            content: None,
        }
    }
}

/// Executes `request` for the run's task and returns the result to send to the runtime.
///
/// Unknown tools and invalid arguments produce an error result without executing anything. A
/// repeated `call_id` returns the stored result. Infrastructure failures are logged and reported
/// as a retryable tool error; the runtime re-sends the call with the same id.
pub async fn execute(
    ctx: &ChannelContext,
    rc: &RunContext,
    request: ToolCallRequest,
) -> ToolCallResult {
    let call_id = request.call_id.clone();
    match try_execute(ctx, rc, request).await {
        Ok(result) => result,
        Err(e) => {
            tracing::error!(error = %e, run_id = %rc.run.id, "tool call failed");
            result(
                &call_id,
                &Outcome::error("internal error; retry the call later"),
            )
        }
    }
}

async fn try_execute(
    ctx: &ChannelContext,
    rc: &RunContext,
    request: ToolCallRequest,
) -> Result<ToolCallResult, ChannelError> {
    let call_id = request.call_id.clone();
    let Ok(call_uuid) = request.call_id.parse::<uuid::Uuid>() else {
        return Ok(result(&call_id, &Outcome::error("call_id must be a UUID")));
    };
    let Some(validator) = VALIDATORS.get(request.tool_name.as_str()) else {
        return Ok(result(
            &call_id,
            &Outcome::error(format!("unknown tool `{}`", request.tool_name)),
        ));
    };
    if let Some(refused) = refused_for_run_kind(rc, &request.tool_name) {
        return Ok(result(&call_id, &Outcome::error(refused)));
    }
    let arguments: Value = if request.arguments_json.trim().is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        match serde_json::from_str(&request.arguments_json) {
            Ok(v) => v,
            Err(e) => {
                return Ok(result(
                    &call_id,
                    &Outcome::error(format!("arguments are not valid JSON: {e}")),
                ));
            }
        }
    };
    let problems: Vec<String> = validator
        .iter_errors(&arguments)
        .map(|e| e.to_string())
        .collect();
    if !problems.is_empty() {
        return Ok(result(
            &call_id,
            &Outcome::error(format!(
                "invalid arguments for `{}`: {}",
                request.tool_name,
                problems.join("; ")
            )),
        ));
    }

    // The bytes of an attachment stay out of the stored row; their hash identifies them.
    let mut stored_arguments = arguments.clone();
    if !request.attachment.is_empty()
        && let Some(object) = stored_arguments.as_object_mut()
    {
        object.insert(
            "attachment_sha256".into(),
            Value::String(df_db::sha256_hex(&request.attachment)),
        );
    }

    // Claim the call before executing it, in its own short transaction: no row lock or pool
    // connection is held while the forge is called, and a call is executed at most once.
    let claimed = tool_calls::claim(
        &ctx.pool,
        call_uuid,
        rc.run.id,
        &request.tool_name,
        &stored_arguments,
    )
    .await?;
    if !claimed {
        return answer_repeated_call(ctx, rc, &call_id, call_uuid).await;
    }

    let outcome = match run_handler(ctx, rc, &request, &arguments).await {
        Ok(outcome) => outcome,
        Err(e) => {
            tracing::error!(error = %e, run_id = %rc.run.id, tool = %request.tool_name, "tool handler failed");
            Outcome::error("internal error; the call may have been applied partially")
        }
    };
    let response = result(&call_id, &outcome);
    let blobs = PgBlobStore::new(ctx.pool.clone());
    let stored = externalize_images(&blobs, response.clone()).await?;
    let mut tx = ctx.pool.begin().await?;
    tool_calls::store_result(&mut *tx, call_uuid, &serde_json::to_value(&stored)?).await?;
    let actor = Principal::agent(
        rc.persona.id,
        rc.persona.display_name.clone(),
        Some(rc.run.id),
        rc.task.id,
    );
    let entry = NewAuditEntry::new(
        actor,
        Vec::new(),
        "tool_call.execute",
        "tool_call",
        request.call_id,
    )
    .with_details(&audit_details(rc, &request.tool_name, &arguments, &outcome))?;
    write_audit_entry(&mut *tx, &entry).await?;
    tx.commit().await?;
    Ok(response)
}

/// Why `tool` is not available in the kind of run `rc` is (a chat is offered [`CHAT_TOOLS`], a
/// task all but [`CHAT_ONLY_TOOLS`]), before any argument is looked at.
fn refused_for_run_kind(rc: &RunContext, tool: &str) -> Option<String> {
    if rc.is_chat() && !CHAT_TOOLS.contains(&tool) {
        Some(format!("`{tool}` is not available in a chat"))
    } else if !rc.is_chat() && CHAT_ONLY_TOOLS.contains(&tool) {
        Some(format!("`{tool}` is only available in a chat"))
    } else {
        None
    }
}

/// Answers a `call_id` that was claimed before: the stored result, or an error if the earlier
/// attempt is still running or died without storing one. Never executes the call again.
async fn answer_repeated_call(
    ctx: &ChannelContext,
    rc: &RunContext,
    call_id: &str,
    call_uuid: uuid::Uuid,
) -> Result<ToolCallResult, ChannelError> {
    let call = tool_calls::get(&ctx.pool, call_uuid).await?;
    if call.run_id != rc.run.id {
        return Ok(result(
            call_id,
            &Outcome::error("call_id is already used by another run"),
        ));
    }
    if let Some(stored) = call.result {
        let blobs = PgBlobStore::new(ctx.pool.clone());
        return rehydrate_images(&blobs, serde_json::from_value(stored)?).await;
    }
    if !call.abandoned {
        return Ok(result(
            call_id,
            &Outcome::error("this call is still in progress; retry later"),
        ));
    }
    // Close the claim so every retry gets the same answer instead of a second execution.
    let closed = result(
        call_id,
        &Outcome::error(
            "an earlier attempt of this call did not finish; check whether its effect exists before calling again",
        ),
    );
    tool_calls::store_result(&ctx.pool, call_uuid, &serde_json::to_value(&closed)?).await?;
    Ok(closed)
}

/// The details of the `tool_call.execute` audit entry. A call that names another task for an
/// artifact read records it as `target_task` and `artifact`, refused or not; a refusal also
/// records its error text.
fn audit_details(rc: &RunContext, tool: &str, arguments: &Value, outcome: &Outcome) -> Value {
    let mut details = serde_json::Map::from_iter([
        ("tool".to_string(), Value::from(tool)),
        ("run_id".to_string(), serde_json::json!(rc.run.id)),
        ("task_id".to_string(), serde_json::json!(rc.task.id)),
        ("is_error".to_string(), Value::from(outcome.is_error)),
    ]);
    let target = text_arg(arguments, "task").filter(|t| *t != rc.task.key.to_string());
    if let (Some(target), "list_artifacts" | "read_artifact") = (target, tool) {
        details.insert("target_task".into(), Value::String(target));
        if outcome.is_error {
            details.insert("error".into(), Value::String(outcome.text.clone()));
        }
        if let Some(name) = text_arg(arguments, "name").filter(|_| tool == "read_artifact") {
            details.insert("artifact".into(), Value::String(name));
        }
    }
    Value::Object(details)
}

/// Moves inline image data of `response` to the blob store; a stored result refers to it by hash.
async fn externalize_images(
    blobs: &PgBlobStore,
    mut response: ToolCallResult,
) -> Result<ToolCallResult, ChannelError> {
    for block in &mut response.content {
        externalize_block(blobs, block).await?;
    }
    Ok(response)
}

/// Puts the image data of a stored result back from the blob store.
async fn rehydrate_images(
    blobs: &PgBlobStore,
    mut response: ToolCallResult,
) -> Result<ToolCallResult, ChannelError> {
    for block in &mut response.content {
        let Some(content_block::Kind::Image(image)) = &mut block.kind else {
            continue;
        };
        let Some(blob_data::Source::Sha256(sha)) =
            image.data.as_ref().and_then(|d| d.source.clone())
        else {
            continue;
        };
        let blob = blobs.get(&sha).await?.ok_or_else(|| {
            OrchestratorError::InvalidData(format!("blob {sha} of a stored tool result is missing"))
        })?;
        image.data = Some(BlobData {
            source: Some(blob_data::Source::Inline(blob.data.into())),
        });
    }
    Ok(response)
}

/// The answer to a call that arrives after the run ended; nothing is executed.
pub fn run_ended(call_id: &str) -> ToolCallResult {
    result(call_id, &Outcome::error("the run has ended"))
}

/// A visible text block.
pub(crate) fn text_block(text: String) -> ContentBlock {
    ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent { text })),
        hidden: false,
    }
}

fn result(call_id: &str, outcome: &Outcome) -> ToolCallResult {
    ToolCallResult {
        call_id: call_id.to_string(),
        is_error: outcome.is_error,
        content: outcome
            .content
            .clone()
            .unwrap_or_else(|| vec![text_block(outcome.text.clone())]),
        structured_content_json: outcome
            .structured
            .as_ref()
            .map(Value::to_string)
            .unwrap_or_default(),
        end_run_after_turn: outcome.end_run_after_turn,
        await_input_after_turn: outcome.await_input_after_turn,
    }
}

/// Runs the tool. Database effects use short statements and forge calls run without any open
/// transaction.
async fn run_handler(
    ctx: &ChannelContext,
    rc: &RunContext,
    request: &ToolCallRequest,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    match request.tool_name.as_str() {
        "update_pull_request" => update_pull_request(ctx, rc, args).await,
        "comment_on_issue" => comment_on_issue(ctx, rc, args).await,
        "report_progress" => report_progress(ctx, rc, args).await,
        "set_task_title" => set_title(ctx, rc, args).await,
        "complete_task" => complete_task(ctx, rc, args).await,
        "ask_question" => ask_question(ctx, rc, args).await,
        "request_approval" => request_approval(ctx, rc, args).await,
        "read_pull_request_feedback" => feedback::read_feedback(ctx, rc, args).await,
        "comment_on_pull_request" => feedback::comment(ctx, rc, args).await,
        "create_task" => delegation::create_task(ctx, rc, args).await,
        "list_personas" => delegation::list_personas(ctx).await,
        "send_message" => delegation::send_message(ctx, rc, args).await,
        "write_artifact" => {
            artifacts::write_artifact(ctx, rc, args, &request.attachment, &request.call_id).await
        }
        "list_artifacts" => artifacts::list_artifacts(ctx, rc, args).await,
        "read_artifact" => artifacts::read_artifact(ctx, rc, args).await,
        "merge_subtask_pr" => delegation::merge_subtask_pr(ctx, rc, args).await,
        "list_pull_requests" => ci::list_pull_requests(ctx, rc, args).await,
        "read_ci_status" => ci::read_ci_status(ctx, rc, args).await,
        "read_ci_log" => ci::read_ci_log(ctx, rc, args).await,
        "list_issues" => issues::list_issues(ctx, rc, args).await,
        "create_issue" => issues::create_issue(ctx, rc, args).await,
        other => Ok(Outcome::error(format!("unknown tool `{other}`"))),
    }
}

/// The string argument `key`, if present.
fn text_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// Saves the pull request title and body on the task and, if the pull request exists, on the forge.
async fn update_pull_request(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let (title, body) = (text_arg(args, "title"), text_arg(args, "body"));
    let attached = get_task(&ctx.pool, rc.task.id)
        .await?
        .ok_or_else(|| OrchestratorError::InvalidData("task of the run is missing".into()))?
        .pr_attached;
    if attached {
        return Ok(Outcome::error(
            "the pull request of this task was opened by someone else, so you can't change its \
             title or body; comment on it instead",
        ));
    }
    let pr_number =
        set_pr_draft_fields(&ctx.pool, rc.task.id, title.as_deref(), body.as_deref()).await?;
    let Some(number) = pr_number.and_then(|n| u64::try_from(n).ok()) else {
        return Ok(Outcome::ok(
            "Saved. The pull request does not exist yet; it will use these values when it is opened.",
        ));
    };
    let update = PullRequestUpdate {
        title,
        body,
        draft: None,
    };
    Ok(
        match ctx
            .forge
            .update_pull_request(&rc.project.id, number, update)
            .await
        {
            Ok(_) => Outcome::ok("Pull request updated."),
            Err(e) => forge_error(&e),
        },
    )
}

/// Adds a comment to the issue linked to the task.
async fn comment_on_issue(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let Some(issue) = rc.task.issue_number.and_then(|n| u64::try_from(n).ok()) else {
        return Ok(Outcome::error("this task has no linked issue"));
    };
    let body = text_arg(args, "body").unwrap_or_default();
    Ok(
        match ctx
            .forge
            .add_issue_comment(&rc.project.id, issue, &body)
            .await
        {
            Ok(comment) => Outcome::ok(format!("Comment added: {}", comment.url)),
            Err(e) => forge_error(&e),
        },
    )
}

/// Stores the status line of the task.
async fn report_progress(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let line = text_arg(args, "summary").unwrap_or_default();
    set_status_line(&ctx.pool, rc.task.id, &line).await?;
    Ok(Outcome::ok("Progress recorded."))
}

/// Sets the title of the task unless it has one.
async fn set_title(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let Some(title) = text_arg(args, "title").as_deref().and_then(sanitize_title) else {
        return Ok(Outcome::error("the title is empty"));
    };
    let changed = set_task_title(&ctx.pool, rc.task.id, &title, TitleSource::Agent).await?;
    Ok(Outcome::ok(if changed {
        "Title set."
    } else {
        "The task already has a title; it is kept."
    }))
}

/// Records the completion summary, closes the task's pending requests and ends the run after the
/// turn.
async fn complete_task(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let task_id = rc.task.id;
    if let Some(blocked) = delegation::open_subtasks_outcome(ctx, rc).await? {
        return Ok(blocked);
    }
    let mut tx = ctx.pool.begin().await?;
    set_completion_summary(
        &mut *tx,
        rc.run.id,
        &text_arg(args, "summary").unwrap_or_default(),
    )
    .await?;
    close_pending_for_task(
        &mut tx,
        task_id,
        "task-completed",
        &Principal::agent(
            rc.persona.id,
            rc.persona.display_name.clone(),
            Some(rc.run.id),
            task_id,
        ),
    )
    .await?;
    publish(&mut *tx, &Notification::TaskChanged { task_id }).await?;
    tx.commit().await?;
    Ok(Outcome {
        end_run_after_turn: true,
        ..Outcome::ok("Task completion recorded. The run ends after this turn.")
    })
}

/// Raises a question; a blocking one makes the run wait for the answer after the turn.
async fn ask_question(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let task_id = rc.task.id;
    let answers: Vec<String> = args
        .get("suggested_answers")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let blocking = args
        .get("blocking")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let actor = Principal::agent(
        rc.persona.id,
        rc.persona.display_name.clone(),
        Some(rc.run.id),
        task_id,
    );
    let from = Raise {
        task_id,
        run_id: Some(rc.run.id),
        actor: &actor,
        blocking,
    };
    let raised = raise_question(
        &ctx.pool,
        from,
        &text_arg(args, "text").unwrap_or_default(),
        &answers,
    )
    .await;
    Ok(match raised {
        Ok((request, _)) => Outcome {
            await_input_after_turn: blocking,
            structured: Some(
                serde_json::json!({ "id": request.id, "state": request.state.as_str() }),
            ),
            ..Outcome::ok(if blocking {
                format!(
                    "Question {} raised. End your turn now; the answer arrives as a message and the run waits for it for a limited time.",
                    request.id
                )
            } else {
                format!(
                    "Question {} raised. Continue your work; the answer arrives later as a message.",
                    request.id
                )
            })
        },
        Err(
            e @ (RequestError::Invalid(_)
            | RequestError::TooManyPending
            | RequestError::Refused(_)
            | RequestError::UnknownType(_)),
        ) => Outcome::error(e.to_string()),
        Err(e) => return Err(e.into()),
    })
}

/// Raises an approval request of a registered type unless the run has what it asks for already;
/// a blocking one makes the run wait for the decision after the turn.
async fn request_approval(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let task_id = rc.task.id;
    if text_arg(args, "type").as_deref() == Some(CREATE_ISSUE) {
        return Ok(Outcome::error(
            "issues are requested with `create_issue`, not with `request_approval`",
        ));
    }
    let blocking = args
        .get("blocking")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let actor = Principal::agent(
        rc.persona.id,
        rc.persona.display_name.clone(),
        Some(rc.run.id),
        task_id,
    );
    let from = Raise {
        task_id,
        run_id: Some(rc.run.id),
        actor: &actor,
        blocking,
    };
    let raised = raise_approval(
        &ctx.pool,
        &ctx.approval_types,
        from,
        &text_arg(args, "type").unwrap_or_default(),
        args.get("payload").cloned().unwrap_or(Value::Null),
        &text_arg(args, "reason").unwrap_or_default(),
    )
    .await;
    Ok(match raised {
        Ok(Raised::AlreadyGranted) => Outcome {
            structured: Some(serde_json::json!({ "state": "already-allowed" })),
            ..Outcome::ok(
                "Your run is already allowed this; no request was stored. If a connection was denied, retry it.",
            )
        },
        Ok(Raised::Request { request, .. }) => Outcome {
            await_input_after_turn: blocking,
            structured: Some(
                serde_json::json!({ "id": request.id, "state": request.state.as_str() }),
            ),
            ..Outcome::ok(if blocking {
                format!(
                    "Approval request {} raised. End your turn now; the decision arrives as a message and the run waits for it for a limited time.",
                    request.id
                )
            } else {
                format!(
                    "Approval request {} raised. Continue your work; the decision arrives later as a message.",
                    request.id
                )
            })
        },
        Err(
            e @ (RequestError::Invalid(_)
            | RequestError::TooManyPending
            | RequestError::Refused(_)
            | RequestError::UnknownType(_)),
        ) => Outcome::error(e.to_string()),
        Err(e) => return Err(e.into()),
    })
}

/// Forge failures become results the model can act on; they never end the run.
fn forge_error(err: &IntegrationError) -> Outcome {
    Outcome::error(match err {
        IntegrationError::RateLimited(limit) => match limit.retry_after_seconds {
            Some(s) => format!("the forge rate-limited this request; retry in {s} seconds"),
            None => "the forge rate-limited this request; retry later".to_string(),
        },
        IntegrationError::NotFound(what) => format!("not found on the forge: {what}"),
        IntegrationError::InvalidInput(what) => format!("the forge rejected the request: {what}"),
        IntegrationError::Unauthorized(what) => format!(
            "the forge refused the request ({what}); ask a human to check the bot account's access \
             to the repository and the project's forge credential"
        ),
        IntegrationError::Unsupported(what) => format!("not supported by this forge: {what}"),
        other => format!("the forge request failed: {other}; retry later"),
    })
}

#[cfg(test)]
mod chat_tests;

#[cfg(test)]
mod tests;
