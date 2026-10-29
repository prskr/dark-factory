//! Factory tools v1: validation, scoping, exactly-once execution and the handlers
//! (`specs/factory-tools/spec.md`, design D6).

use std::collections::HashMap;
use std::sync::LazyLock;

use df_core::{IntegrationError, Principal, PullRequestUpdate, TitleSource, sanitize_title};
use df_db::requests::{Raise, RequestError, close_pending_for_task, raise_question};
use df_db::{NewAuditEntry, Notification, publish, set_task_title, write_audit_entry};
use df_proto::agent::v1::{ToolCallRequest, ToolCallResult};
use df_proto::transcript::v1::{ContentBlock, TextContent, content_block};
use jsonschema::Validator;
use serde_json::Value;
use sqlx::{Postgres, Transaction};

use crate::ChannelContext;
use crate::assign::{RunContext, tool_input_schemas};
use crate::error::ChannelError;

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
}

impl Outcome {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            is_error: false,
            text: text.into(),
            end_run_after_turn: false,
            await_input_after_turn: false,
            structured: None,
        }
    }

    fn error(text: impl Into<String>) -> Self {
        Self {
            is_error: true,
            text: text.into(),
            end_run_after_turn: false,
            await_input_after_turn: false,
            structured: None,
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

    // Claim the call before executing it, in its own short transaction: no row lock or pool
    // connection is held while the forge is called, and a call is executed at most once.
    let claimed = sqlx::query(
        "INSERT INTO tool_calls (call_id, run_id, tool_name, arguments)
         VALUES ($1, $2, $3, $4) ON CONFLICT (call_id) DO NOTHING",
    )
    .bind(call_uuid)
    .bind(rc.run.id.as_uuid())
    .bind(&request.tool_name)
    .bind(&arguments)
    .execute(&ctx.pool)
    .await?
    .rows_affected()
        == 1;
    if !claimed {
        return answer_repeated_call(ctx, rc, &call_id, call_uuid).await;
    }

    let outcome = match run_handler(ctx, rc, &request.tool_name, &arguments).await {
        Ok(outcome) => outcome,
        Err(e) => {
            tracing::error!(error = %e, run_id = %rc.run.id, tool = %request.tool_name, "tool handler failed");
            Outcome::error("internal error; the call may have been applied partially")
        }
    };
    let response = result(&call_id, &outcome);
    let mut tx = ctx.pool.begin().await?;
    store_result(&mut tx, call_uuid, &response).await?;
    let actor = Principal::agent(
        rc.persona.id,
        rc.persona.name.clone(),
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
    .with_details(&serde_json::json!({
        "tool": request.tool_name,
        "run_id": rc.run.id,
        "task_id": rc.task.id,
        "is_error": outcome.is_error,
    }))?;
    write_audit_entry(&mut *tx, &entry).await?;
    tx.commit().await?;
    Ok(response)
}

/// Answers a `call_id` that was claimed before: the stored result, or an error if the earlier
/// attempt is still running or died without storing one. Never executes the call again.
async fn answer_repeated_call(
    ctx: &ChannelContext,
    rc: &RunContext,
    call_id: &str,
    call_uuid: uuid::Uuid,
) -> Result<ToolCallResult, ChannelError> {
    let (owner, stored, abandoned): (uuid::Uuid, Option<Value>, bool) = sqlx::query_as(
        "SELECT run_id, result, created_at < now() - interval '5 minutes'
         FROM tool_calls WHERE call_id = $1",
    )
    .bind(call_uuid)
    .fetch_one(&ctx.pool)
    .await?;
    if owner != rc.run.id.as_uuid() {
        return Ok(result(
            call_id,
            &Outcome::error("call_id is already used by another run"),
        ));
    }
    if let Some(stored) = stored {
        return Ok(serde_json::from_value(stored)?);
    }
    if !abandoned {
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
    let mut tx = ctx.pool.begin().await?;
    store_result(&mut tx, call_uuid, &closed).await?;
    tx.commit().await?;
    Ok(closed)
}

/// Stores `response` unless a result is already there.
async fn store_result(
    tx: &mut Transaction<'_, Postgres>,
    call_uuid: uuid::Uuid,
    response: &ToolCallResult,
) -> Result<(), ChannelError> {
    sqlx::query(
        "UPDATE tool_calls SET result = $2, completed_at = now() WHERE call_id = $1 AND result IS NULL",
    )
    .bind(call_uuid)
    .bind(serde_json::to_value(response)?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// The answer to a call that arrives after the run ended; nothing is executed.
pub fn run_ended(call_id: &str) -> ToolCallResult {
    result(call_id, &Outcome::error("the run has ended"))
}

fn result(call_id: &str, outcome: &Outcome) -> ToolCallResult {
    ToolCallResult {
        call_id: call_id.to_string(),
        is_error: outcome.is_error,
        content: vec![ContentBlock {
            kind: Some(content_block::Kind::Text(TextContent {
                text: outcome.text.clone(),
            })),
            hidden: false,
        }],
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
    tool: &str,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    match tool {
        "update_pull_request" => update_pull_request(ctx, rc, args).await,
        "comment_on_issue" => comment_on_issue(ctx, rc, args).await,
        "report_progress" => report_progress(ctx, rc, args).await,
        "set_task_title" => set_title(ctx, rc, args).await,
        "complete_task" => complete_task(ctx, rc, args).await,
        "ask_question" => ask_question(ctx, rc, args).await,
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
    let pr_number: Option<i64> = sqlx::query_scalar(
        "UPDATE tasks SET pr_title = COALESCE($2, pr_title), pr_body = COALESCE($3, pr_body),
             updated_at = now()
         WHERE id = $1 RETURNING pr_number",
    )
    .bind(rc.task.id.as_uuid())
    .bind(&title)
    .bind(&body)
    .fetch_one(&ctx.pool)
    .await?;
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
    let task_id = rc.task.id;
    let mut tx = ctx.pool.begin().await?;
    sqlx::query("UPDATE tasks SET status_line = $2, updated_at = now() WHERE id = $1")
        .bind(task_id.as_uuid())
        .bind(text_arg(args, "summary").unwrap_or_default())
        .execute(&mut *tx)
        .await?;
    publish(&mut *tx, &Notification::TaskChanged { task_id }).await?;
    tx.commit().await?;
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
    let mut tx = ctx.pool.begin().await?;
    sqlx::query("UPDATE tasks SET completion_summary = $2, updated_at = now() WHERE id = $1")
        .bind(task_id.as_uuid())
        .bind(text_arg(args, "summary").unwrap_or_default())
        .execute(&mut *tx)
        .await?;
    close_pending_for_task(
        &mut tx,
        task_id,
        "task-completed",
        &Principal::agent(
            rc.persona.id,
            rc.persona.name.clone(),
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
        rc.persona.name.clone(),
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
        IntegrationError::Unauthorized(_) => {
            "the forge rejected the credentials; ask a human to check the project's forge credential".to_string()
        }
        other => format!("the forge request failed: {other}; retry later"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assign::load_run_context;
    use crate::runs::get_run;
    use crate::testing::Env;
    use df_core::testing::ForgeCall;
    use df_core::{Comment, ForgeClient, RateLimit};
    use sqlx::PgPool;

    async fn setup(pool: PgPool) -> (Env, RunContext) {
        let env = Env::new(pool.clone()).await;
        let run = env.insert_run(env.task.id, 1, "running").await;
        let rc = load_run_context(&pool, get_run(&pool, run).await.unwrap().unwrap())
            .await
            .unwrap();
        (env, rc)
    }

    fn call(tool: &str, args: Value) -> ToolCallRequest {
        ToolCallRequest {
            call_id: uuid::Uuid::now_v7().to_string(),
            tool_name: tool.into(),
            arguments_json: args.to_string(),
        }
    }

    fn message(result: &ToolCallResult) -> String {
        match &result.content[0].kind {
            Some(content_block::Kind::Text(t)) => t.text.clone(),
            other => panic!("unexpected content {other:?}"),
        }
    }

    #[test]
    fn embedded_schemas_compile() {
        assert_eq!(VALIDATORS.len(), 6);
    }

    #[tokio::test]
    async fn unknown_tool_executes_nothing() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let res = execute(&env.ctx, &rc, call("delete_project", serde_json::json!({}))).await;
        assert!(res.is_error);
        assert!(message(&res).contains("delete_project"));
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM tool_calls")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[tokio::test]
    async fn schema_violation_names_the_missing_field() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool).await;
        let res = execute(
            &env.ctx,
            &rc,
            call("report_progress", serde_json::json!({})),
        )
        .await;
        assert!(res.is_error);
        assert!(message(&res).contains("summary"), "{}", message(&res));
    }

    #[tokio::test]
    async fn retry_with_the_same_call_id_executes_once() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let request = call("comment_on_issue", serde_json::json!({"body": "findings"}));

        let first = execute(&env.ctx, &rc, request.clone()).await;
        let second = execute(&env.ctx, &rc, request).await;

        assert!(!first.is_error);
        assert_eq!(first, second);
        let comments = env
            .forge
            .calls()
            .into_iter()
            .filter(|c| matches!(c, ForgeCall::AddIssueComment { number: 42, .. }))
            .count();
        assert_eq!(comments, 1);
        let audited: i64 =
            sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'tool_call.execute'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(audited, 1);
    }

    #[tokio::test]
    async fn a_claimed_call_that_is_still_running_is_not_executed_again() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let request = call("comment_on_issue", serde_json::json!({"body": "x"}));
        // Another attempt claimed the call and has not stored a result yet.
        sqlx::query("INSERT INTO tool_calls (call_id, run_id, tool_name, arguments) VALUES ($1, $2, 'comment_on_issue', '{}')")
            .bind(request.call_id.parse::<uuid::Uuid>().unwrap())
            .bind(rc.run.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();

        let res = execute(&env.ctx, &rc, request).await;

        assert!(res.is_error);
        assert!(message(&res).contains("in progress"), "{}", message(&res));
        assert!(env.forge.calls().is_empty());
    }

    #[tokio::test]
    async fn an_abandoned_claim_is_closed_with_an_error_and_never_executed() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let request = call("comment_on_issue", serde_json::json!({"body": "x"}));
        // The claiming replica died long ago without storing a result.
        sqlx::query("INSERT INTO tool_calls (call_id, run_id, tool_name, arguments, created_at) VALUES ($1, $2, 'comment_on_issue', '{}', now() - interval '1 hour')")
            .bind(request.call_id.parse::<uuid::Uuid>().unwrap())
            .bind(rc.run.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();

        let first = execute(&env.ctx, &rc, request.clone()).await;
        let second = execute(&env.ctx, &rc, request).await;

        assert!(first.is_error);
        assert!(
            message(&first).contains("did not finish"),
            "{}",
            message(&first)
        );
        assert_eq!(
            first, second,
            "the closed claim keeps answering the same way"
        );
        assert!(
            env.forge.calls().is_empty(),
            "at most once: never executed after the claim"
        );
    }

    #[tokio::test]
    async fn the_forge_call_does_not_hold_a_database_transaction() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let request = call("comment_on_issue", serde_json::json!({"body": "x"}));
        // While the forge call runs, the claim is committed: visible to other connections.
        let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
        let forge = ObservingForge {
            pool: pool.clone(),
            call_id: request.call_id.clone(),
            seen: observed.clone(),
        };
        let mut ctx = env.ctx.clone();
        ctx.forge = std::sync::Arc::new(forge);

        execute(&ctx, &rc, request).await;

        assert_eq!(
            *observed.lock().unwrap(),
            Some(1),
            "claim row visible during the forge call"
        );
    }

    /// Records how many `tool_calls` rows with the call id other connections see during a call.
    struct ObservingForge {
        pool: PgPool,
        call_id: String,
        seen: std::sync::Arc<std::sync::Mutex<Option<i64>>>,
    }

    #[async_trait::async_trait]
    impl ForgeClient for ObservingForge {
        async fn create_pull_request(
            &self,
            _: &df_core::ProjectId,
            _: df_core::NewPullRequest,
        ) -> Result<df_core::PullRequest, IntegrationError> {
            unreachable!("not used")
        }
        async fn find_open_pull_request(
            &self,
            _: &df_core::ProjectId,
            _: &str,
            _: &str,
        ) -> Result<Option<df_core::PullRequest>, IntegrationError> {
            unreachable!("not used")
        }
        async fn update_pull_request(
            &self,
            _: &df_core::ProjectId,
            _: u64,
            _: PullRequestUpdate,
        ) -> Result<df_core::PullRequest, IntegrationError> {
            unreachable!("not used")
        }
        async fn get_issue(
            &self,
            _: &df_core::ProjectId,
            _: u64,
        ) -> Result<df_core::Issue, IntegrationError> {
            unreachable!("not used")
        }
        async fn add_issue_comment(
            &self,
            _: &df_core::ProjectId,
            _: u64,
            _: &str,
        ) -> Result<Comment, IntegrationError> {
            let rows: i64 =
                sqlx::query_scalar("SELECT count(*) FROM tool_calls WHERE call_id = $1")
                    .bind(self.call_id.parse::<uuid::Uuid>().unwrap())
                    .fetch_one(&self.pool)
                    .await
                    .unwrap();
            *self.seen.lock().unwrap() = Some(rows);
            Ok(Comment {
                id: "1".into(),
                url: "u".into(),
            })
        }
    }

    #[tokio::test]
    async fn update_pull_request_before_the_pr_exists_stores_the_values() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let res = execute(
            &env.ctx,
            &rc,
            call(
                "update_pull_request",
                serde_json::json!({"title": "New title"}),
            ),
        )
        .await;
        assert!(!res.is_error);
        let title: Option<String> = sqlx::query_scalar("SELECT pr_title FROM tasks WHERE id = $1")
            .bind(rc.task.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(title.as_deref(), Some("New title"));
        assert!(env.forge.calls().is_empty());
    }

    #[tokio::test]
    async fn update_pull_request_updates_an_existing_pr() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let pr = env
            .forge
            .create_pull_request(
                &rc.project.id,
                df_core::NewPullRequest {
                    repo: crate::pr::repo_ref(&rc.project.repo_url).unwrap(),
                    head: "factory/task/DF-1".into(),
                    base: "main".into(),
                    title: "t".into(),
                    body: "b".into(),
                    draft: true,
                },
            )
            .await
            .unwrap();
        sqlx::query("UPDATE tasks SET pr_number = $2 WHERE id = $1")
            .bind(rc.task.id.as_uuid())
            .bind(i64::try_from(pr.id.number).unwrap())
            .execute(&pool)
            .await
            .unwrap();
        let res = execute(
            &env.ctx,
            &rc,
            call(
                "update_pull_request",
                serde_json::json!({"body": "Details"}),
            ),
        )
        .await;
        assert!(!res.is_error, "{}", message(&res));
        assert!(env.forge.calls().iter().any(|c| matches!(c,
            ForgeCall::UpdatePullRequest { number: 1, changes, .. } if changes.body.as_deref() == Some("Details"))));
    }

    #[tokio::test]
    async fn comment_without_linked_issue_is_an_error() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, mut rc) = setup(pool).await;
        rc.task.issue_number = None;
        let res = execute(
            &env.ctx,
            &rc,
            call("comment_on_issue", serde_json::json!({"body": "x"})),
        )
        .await;
        assert!(res.is_error);
        assert!(message(&res).contains("this task has no linked issue"));
    }

    #[tokio::test]
    async fn report_progress_sets_the_status_line() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let res = execute(
            &env.ctx,
            &rc,
            call(
                "report_progress",
                serde_json::json!({"summary": "tests pass"}),
            ),
        )
        .await;
        assert!(!res.is_error);
        let line: String = sqlx::query_scalar("SELECT status_line FROM tasks WHERE id = $1")
            .bind(rc.task.id.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(line, "tests pass");
    }

    async fn title_of(pool: &PgPool, rc: &RunContext) -> (String, String) {
        sqlx::query_as("SELECT title, title_source FROM tasks WHERE id = $1")
            .bind(rc.task.id.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn make_placeholder(pool: &PgPool, rc: &RunContext) {
        sqlx::query("UPDATE tasks SET title_source = 'placeholder' WHERE id = $1")
            .bind(rc.task.id.as_uuid())
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn set_task_title_sets_a_sanitized_agent_title_once() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        make_placeholder(&pool, &rc).await;
        let mut events = env.ctx.notifier.subscribe();

        let res = execute(
            &env.ctx,
            &rc,
            call(
                "set_task_title",
                serde_json::json!({"title": "Stop duplicate\nnightly backups"}),
            ),
        )
        .await;

        assert!(!res.is_error);
        assert_eq!(
            title_of(&pool, &rc).await,
            ("Stop duplicate nightly backups".into(), "agent".into())
        );
        let changed = tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            changed,
            Notification::TaskChanged {
                task_id: rc.task.id
            }
        );

        let again = execute(
            &env.ctx,
            &rc,
            call("set_task_title", serde_json::json!({"title": "Other"})),
        )
        .await;
        assert!(!again.is_error);
        assert!(message(&again).contains("kept"));
        assert_eq!(
            title_of(&pool, &rc).await.0,
            "Stop duplicate nightly backups"
        );
    }

    #[tokio::test]
    async fn set_task_title_keeps_the_title_of_a_person() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;

        let res = execute(
            &env.ctx,
            &rc,
            call(
                "set_task_title",
                serde_json::json!({"title": "Agent title"}),
            ),
        )
        .await;

        assert!(!res.is_error);
        assert!(message(&res).contains("kept"));
        assert_eq!(
            title_of(&pool, &rc).await,
            ("Fix login".into(), "user".into())
        );
    }

    #[tokio::test]
    async fn set_task_title_rejects_a_blank_title() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        make_placeholder(&pool, &rc).await;

        let res = execute(
            &env.ctx,
            &rc,
            call("set_task_title", serde_json::json!({"title": " \n "})),
        )
        .await;

        assert!(res.is_error);
        assert_eq!(
            title_of(&pool, &rc).await,
            ("Fix login".into(), "placeholder".into())
        );
    }

    #[tokio::test]
    async fn complete_task_stores_the_summary_and_ends_the_run_after_the_turn() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let res = execute(
            &env.ctx,
            &rc,
            call("complete_task", serde_json::json!({"summary": "Done."})),
        )
        .await;
        assert!(res.end_run_after_turn);
        let stored: String =
            sqlx::query_scalar("SELECT completion_summary FROM tasks WHERE id = $1")
                .bind(rc.task.id.as_uuid())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, "Done.");
    }

    #[tokio::test]
    async fn forge_errors_are_tool_errors_with_actionable_text() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool).await;
        env.forge
            .push_add_issue_comment_result(Err(IntegrationError::RateLimited(RateLimit {
                retry_after_seconds: Some(30),
                message: "slow down".into(),
            })));
        let res = execute(
            &env.ctx,
            &rc,
            call("comment_on_issue", serde_json::json!({"body": "x"})),
        )
        .await;
        assert!(res.is_error);
        assert!(message(&res).contains("retry in 30 seconds"));

        env.forge.push_add_issue_comment_result(Ok(Comment {
            id: "1".into(),
            url: "u".into(),
        }));
        let ok = execute(
            &env.ctx,
            &rc,
            call("comment_on_issue", serde_json::json!({"body": "x"})),
        )
        .await;
        assert!(
            !ok.is_error,
            "the run's tools keep working after a forge error"
        );
    }

    async fn pending_requests(pool: &PgPool) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM agent_requests WHERE state = 'pending'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn non_blocking_question_returns_its_id_and_does_not_pause_the_run() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let res = execute(
            &env.ctx,
            &rc,
            call(
                "ask_question",
                serde_json::json!({"text": "Should I also update the docs?", "suggested_answers": ["yes", "no"]}),
            ),
        )
        .await;
        assert!(!res.is_error, "{}", message(&res));
        assert!(!res.await_input_after_turn && !res.end_run_after_turn);
        let structured: Value = serde_json::from_str(&res.structured_content_json).unwrap();
        assert_eq!(structured["state"], "pending");
        assert!(structured["id"].is_string());
        assert_eq!(pending_requests(&pool).await, 1);
    }

    #[tokio::test]
    async fn blocking_question_awaits_input_after_the_turn() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        let res = execute(
            &env.ctx,
            &rc,
            call(
                "ask_question",
                serde_json::json!({"text": "Which?", "blocking": true}),
            ),
        )
        .await;
        assert!(res.await_input_after_turn);
        assert!(!res.end_run_after_turn);
    }

    #[tokio::test]
    async fn too_many_questions_is_an_error_the_agent_can_read() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        for i in 0..21 {
            let res = execute(
                &env.ctx,
                &rc,
                call("ask_question", serde_json::json!({"text": format!("q{i}")})),
            )
            .await;
            assert_eq!(res.is_error, i == 20, "{}", message(&res));
            if res.is_error {
                assert!(message(&res).contains("too many pending requests"));
            }
        }
    }

    #[tokio::test]
    async fn completing_the_task_closes_its_pending_questions() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let (env, rc) = setup(pool.clone()).await;
        execute(
            &env.ctx,
            &rc,
            call("ask_question", serde_json::json!({"text": "Which?"})),
        )
        .await;
        execute(
            &env.ctx,
            &rc,
            call("complete_task", serde_json::json!({"summary": "Done."})),
        )
        .await;
        assert_eq!(pending_requests(&pool).await, 0);
        let reason: String = sqlx::query_scalar("SELECT close_reason FROM agent_requests")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(reason, "task-completed");
    }
}
