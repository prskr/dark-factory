//! Writes demo tasks: the task with its dependencies and pull request through `df_db`, the rest
//! (runs, transcript, token usage, request, artifacts) with plain queries, because the `df_db`
//! writers stamp the current time and this profile needs times in the recent past. All times are
//! computed in SQL from `now()` of the transaction, so a task is consistent in itself.

use std::str::FromStr;

use anyhow::{Context as _, Result, bail};
use df_core::artifacts::{ArtifactKind, check_artifact};
use df_core::config::ArtifactsConfig;
use df_core::{
    DependencyKind, PersonaId, Principal, ProjectId, RunId, RunOutcome, TaskId, TaskKey,
    TaskStatus, UserId,
};
use df_db::tasks::{create_task_on_tx, text_block};
use df_db::transcript::{append_control_event, factory_event, forge_event, lock_run_and_task};
use df_db::{AttachedPr, NewDependency, NewTask, PgPool, TaskMessageRow, TaskPlacement};
use df_proto::transcript::v1::{
    self as pb, ArtifactWritten, BranchPushed, Event, FactoryActivity, ForgeActivity,
    PullRequestOpened, StopReason, ToolCallStatus, event, factory_activity, forge_activity,
};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::data::{
    Artifact, Content, Creator, DemoRun, DemoTask, GPT, Step, ToolKind, Usage, pr_url,
};
use super::{Ids, Report};

/// A demo task that exists in the database.
#[derive(Clone)]
pub(super) struct Written {
    pub(super) id: Uuid,
    pub(super) key: String,
    pub(super) persona: Uuid,
    pub(super) persona_name: &'static str,
}

/// Seconds as the `float8` the SQL below turns into an interval.
type Secs = f64;

/// Writes `task` unless it exists (identified by project key and title).
pub(super) async fn ensure_task(
    pool: &PgPool,
    ctx: &mut Ids,
    task: &DemoTask,
    report: &mut Report,
) -> Result<()> {
    let project = *ctx.projects.get(task.project).context("demo project")?;
    let existing: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, key FROM tasks WHERE project_id = $1 AND title = $2")
            .bind(project)
            .bind(task.title)
            .fetch_optional(pool)
            .await?;
    let persona = *ctx.personas.get(task.persona).context("demo persona")?;
    let written = if let Some((id, key)) = existing {
        refresh_resume_time(pool, id, &key, task, report).await?;
        Written {
            id,
            key,
            persona,
            persona_name: task.persona,
        }
    } else {
        let mut tx = pool.begin().await?;
        let written = insert_task(&mut tx, ctx, task, project, persona)
            .await
            .with_context(|| format!("demo task {}", task.title))?;
        tx.commit().await?;
        written
    };
    ctx.tasks.insert((task.project, task.title), written);
    Ok(())
}

/// The one update of an existing task: a resume time that has passed moves forward again, as long
/// as no run started since the seeding.
async fn refresh_resume_time(
    pool: &PgPool,
    id: Uuid,
    key: &str,
    task: &DemoTask,
    report: &mut Report,
) -> Result<()> {
    let Some(resume_in) = task.resume_in else {
        return Ok(());
    };
    let runs = i64::try_from(task.runs.len())?;
    let moved = sqlx::query(
        "UPDATE tasks SET resume_at = now() + make_interval(secs => $2)
         WHERE id = $1 AND status = $3 AND resume_at < now()
           AND (SELECT count(*) FROM runs WHERE task_id = $1) = $4",
    )
    .bind(id)
    .bind(secs(resume_in))
    .bind(task.status.as_str())
    .bind(runs)
    .execute(pool)
    .await?
    .rows_affected();
    if moved > 0 {
        sqlx::query(
            "UPDATE runs SET retry_after = now() + make_interval(secs => $2)
             WHERE task_id = $1 AND number = $3 AND retry_after IS NOT NULL",
        )
        .bind(id)
        .bind(secs(resume_in))
        .bind(runs)
        .execute(pool)
        .await?;
        return Ok(());
    }
    let (status, due, started): (String, bool, i64) = sqlx::query_as(
        "SELECT status, COALESCE(resume_at < now(), true),
                (SELECT count(*) FROM runs WHERE task_id = $1)
         FROM tasks WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    if due && (status != task.status.as_str() || started != runs) {
        report.hints.push(format!(
            "{key} is no longer in the state it was seeded in ({status}, {started} runs): a control plane with a cluster started it. Seed a fresh database to get the demo state back."
        ));
    }
    Ok(())
}

async fn insert_task(
    tx: &mut Transaction<'_, Postgres>,
    ctx: &Ids,
    task: &DemoTask,
    project: Uuid,
    persona: Uuid,
) -> Result<Written> {
    let parent = task
        .parent
        .map(|title| ctx.tasks.get(&(task.project, title)).context("parent task"))
        .transpose()?;
    let created_by = match task.created_by {
        Creator::Human(name) => Principal::human(human_id(name), name),
        Creator::Agent => {
            let parent = parent.context("an agent creates a subtask of its task")?;
            Principal::agent(
                PersonaId::from_uuid(parent.persona),
                parent.persona_name,
                None,
                TaskId::from_uuid(parent.id),
            )
        }
    };
    let depends_on = task
        .depends_on
        .iter()
        .map(|title| {
            let dependency = ctx
                .tasks
                .get(&(task.project, *title))
                .context("dependency")?;
            Ok(NewDependency {
                key: TaskKey::from_str(&dependency.key)?,
                kind: DependencyKind::Done,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let pr_url = task
        .pr
        .map(|pr| pr_url(task.project, pr.number).context("project of the task"))
        .transpose()?;
    let placement = match (task.pr, &pr_url) {
        (Some(pr), Some(url)) if pr.attached_branch.is_some() => TaskPlacement {
            branch: Some(df_core::ExistingBranch::parse(
                pr.attached_branch.unwrap_or_default(),
                "main",
            )?),
            attached_pr: Some(AttachedPr {
                number: pr.number,
                url: url.clone(),
                base: "main".into(),
            }),
        },
        _ => TaskPlacement::default(),
    };
    let (row, message) = create_task_on_tx(
        tx,
        NewTask {
            project_id: ProjectId::from_uuid(project),
            persona_id: PersonaId::from_uuid(persona),
            title: Some(task.title.to_owned()),
            description: task.description.to_owned(),
            issue_number: task.issue,
            created_by,
            parent_id: parent.map(|p| TaskId::from_uuid(p.id)),
            depends_on,
        },
        placement,
    )
    .await?;
    let written = Written {
        id: row.id.as_uuid(),
        key: row.key.to_string(),
        persona,
        persona_name: task.persona,
    };
    write_runs(tx, &written, task, &message).await?;
    set_task_fields(tx, &written, task, pr_url.as_deref()).await?;
    if let Some(question) = &task.question {
        write_question(tx, &written, task, question).await?;
    }
    Ok(written)
}

/// The task row as the runs left it: status, times, pull request.
async fn set_task_fields(
    tx: &mut Transaction<'_, Postgres>,
    task: &Written,
    spec: &DemoTask,
    pr_url: Option<&str>,
) -> Result<()> {
    let last_end = spec.runs.last().map(|run| run.start_ago - run.minutes * 60);
    let updated_ago = spec.done_ago.or(last_end).unwrap_or(spec.created_ago);
    let completed = matches!(spec.status, TaskStatus::InReview | TaskStatus::Completed)
        .then_some(last_end)
        .flatten();
    // A completed task with a pull request was done by merging it.
    let merged = (spec.status == TaskStatus::Completed && spec.pr.is_some())
        .then_some(spec.done_ago)
        .flatten();
    sqlx::query(
        "UPDATE tasks SET status = $2, status_line = $3,
             created_at = now() - make_interval(secs => $4),
             updated_at = now() - make_interval(secs => $5),
             resume_at = now() + make_interval(secs => $6),
             auto_resumes = $7,
             first_completed_at = now() - make_interval(secs => $8),
             first_done_at = now() - make_interval(secs => $9),
             pr_number = $10, pr_url = $11, pr_title = $12, pr_body = $13,
             pr_merged_at = now() - make_interval(secs => $14)
         WHERE id = $1",
    )
    .bind(task.id)
    .bind(spec.status.as_str())
    .bind(spec.status_line)
    .bind(secs(spec.created_ago))
    .bind(secs(updated_ago))
    .bind(spec.resume_in.map(secs))
    .bind(spec.auto_resumes)
    .bind(completed.map(secs))
    .bind(spec.done_ago.map(secs))
    .bind(spec.pr.map(|pr| pr.number))
    .bind(pr_url)
    .bind(spec.pr.map(|pr| pr.title))
    .bind(spec.pr.map(|pr| pr.body))
    .bind(merged.map(secs))
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE task_messages SET queued_at = now() - make_interval(secs => $2) WHERE task_id = $1",
    )
    .bind(task.id)
    .bind(secs(spec.created_ago))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn write_runs(
    tx: &mut Transaction<'_, Postgres>,
    task: &Written,
    spec: &DemoTask,
    assignment: &TaskMessageRow,
) -> Result<()> {
    for (index, run) in spec.runs.iter().enumerate() {
        let number = i64::try_from(index)? + 1;
        let run_id = Uuid::now_v7();
        insert_run(tx, task, run, run_id, number).await?;
        let last = index + 1 == spec.runs.len();
        let mut writer = Transcript {
            task: task.id,
            run: run_id,
            started_ago: run.start_ago,
            length: run.minutes * 60,
            seq: 0,
        };
        let prompt = if index == 0 {
            Prompt::Assignment(assignment)
        } else {
            match run.steps.first() {
                Some(Step::Message { author, text }) => Prompt::Message { author, text },
                _ => bail!("run {number} of {} starts without a message", spec.title),
            }
        };
        let (rest, turn) = write_prompt(tx, &mut writer, task, run, number, prompt).await?;
        write_steps(tx, &mut writer, &turn, rest).await?;
        write_turn_end(tx, &mut writer, &turn, run).await?;
        insert_usage(tx, run_id, run).await?;
        if run.outcome == RunOutcome::Completed {
            write_pull_request_events(tx, task, spec, run_id, run).await?;
        }
        if last {
            write_artifacts(tx, task, run_id, run, &spec.artifacts).await?;
        }
    }
    Ok(())
}

async fn insert_run(
    tx: &mut Transaction<'_, Postgres>,
    task: &Written,
    run: &DemoRun,
    run_id: Uuid,
    number: i64,
) -> Result<()> {
    let state = run.outcome.run_state().as_str();
    sqlx::query(
        "INSERT INTO runs (id, task_id, number, state, outcome, detail, retry_after,
                           started_at, connected_at, ended_at, completion_summary)
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(secs => $7),
                 now() - make_interval(secs => $8),
                 now() - make_interval(secs => $8) + interval '20 seconds',
                 now() - make_interval(secs => $9), $10)",
    )
    .bind(run_id)
    .bind(task.id)
    .bind(number)
    .bind(state)
    .bind(run.outcome.as_str())
    .bind(run.detail)
    .bind(run.retry_in.map(secs))
    .bind(secs(run.start_ago))
    .bind(secs(run.start_ago - run.minutes * 60))
    .bind(run.summary)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// What the run starts with.
enum Prompt<'a> {
    /// The assignment of the task.
    Assignment(&'a TaskMessageRow),
    /// A follow-up message of a board member.
    Message {
        author: &'static str,
        text: &'static str,
    },
}

/// Writes the turn start and the user message, marks the message delivered and returns the steps
/// after it and the turn id.
async fn write_prompt<'s>(
    tx: &mut Transaction<'_, Postgres>,
    writer: &mut Transcript,
    task: &Written,
    run: &'s DemoRun,
    number: i64,
    prompt: Prompt<'_>,
) -> Result<(&'s [Step], String)> {
    let turn = format!("{}-turn-{number}", task.key);
    let (prompt_id, author, content, rest) = match prompt {
        Prompt::Assignment(message) => (
            message.id.as_uuid(),
            pb_principal_system("task.assigned"),
            serde_json::to_value(&message.content)?,
            run.steps.as_slice(),
        ),
        Prompt::Message { author, text } => {
            let id = Uuid::now_v7();
            let blocks = vec![text_block(text.to_owned(), false)];
            sqlx::query(
                "INSERT INTO task_messages (id, task_id, author, content, queued_at)
                 VALUES ($1, $2, $3, $4, now() - make_interval(secs => $5))",
            )
            .bind(id)
            .bind(task.id)
            .bind(serde_json::to_value(Principal::human(
                human_id(author),
                author,
            ))?)
            .bind(serde_json::to_value(&blocks)?)
            .bind(secs(run.start_ago + 120))
            .execute(&mut **tx)
            .await?;
            (
                id,
                pb_principal_human(author),
                serde_json::to_value(&blocks)?,
                run.steps.get(1..).unwrap_or_default(),
            )
        }
    };
    sqlx::query(
        "UPDATE task_messages SET delivered_run_id = (SELECT id FROM runs WHERE id = $2),
             delivered_at = now() - make_interval(secs => $3),
             forwarded_at = now() - make_interval(secs => $3)
         WHERE id = $1",
    )
    .bind(prompt_id)
    .bind(writer.run)
    .bind(secs(run.start_ago - 20))
    .execute(&mut **tx)
    .await?;
    writer
        .push(
            tx,
            "turn_started",
            event_of(
                &turn,
                event::Kind::TurnStarted(pb::TurnStarted {
                    prompt_ids: vec![prompt_id.to_string()],
                }),
            ),
        )
        .await?;
    let content: Vec<pb::ContentBlock> = serde_json::from_value(content)?;
    writer
        .push(
            tx,
            "user_message",
            event_of(
                &turn,
                event::Kind::UserMessage(pb::UserMessage {
                    prompt_id: prompt_id.to_string(),
                    author: Some(author),
                    content,
                }),
            ),
        )
        .await?;
    Ok((rest, turn))
}

async fn write_steps(
    tx: &mut Transaction<'_, Postgres>,
    writer: &mut Transcript,
    turn: &str,
    steps: &[Step],
) -> Result<()> {
    for (index, step) in steps.iter().enumerate() {
        let (kind, kind_event) = match step {
            Step::Agent(text) => (
                "agent_message_chunk",
                event::Kind::AgentMessageChunk(pb::AgentMessageChunk {
                    message_id: format!("{turn}-message-{index}"),
                    content: Some(text_block((*text).to_owned(), false)),
                }),
            ),
            Step::Tool {
                title,
                kind,
                input,
                output,
            } => (
                "tool_call",
                event::Kind::ToolCall(pb::ToolCall {
                    tool_call_id: format!("{turn}-tool-{index}"),
                    title: (*title).to_owned(),
                    kind: tool_kind(*kind).into(),
                    status: ToolCallStatus::Completed.into(),
                    raw_input_json: (*input).to_owned(),
                    raw_output_json: serde_json::to_string(output)?,
                    origin: pb::ToolOrigin::Harness.into(),
                    ..pb::ToolCall::default()
                }),
            ),
            Step::Message { .. } => bail!("a follow-up message is only allowed first in a run"),
        };
        writer.push(tx, kind, event_of(turn, kind_event)).await?;
    }
    Ok(())
}

async fn write_turn_end(
    tx: &mut Transaction<'_, Postgres>,
    writer: &mut Transcript,
    turn: &str,
    run: &DemoRun,
) -> Result<()> {
    // The sandbox is gone: the runtime never reported the end of the turn.
    let stop_reason = match run.outcome {
        RunOutcome::SandboxLost | RunOutcome::ConnectTimeout | RunOutcome::ProvisioningFailed => {
            return Ok(());
        }
        RunOutcome::RateLimited | RunOutcome::LimitReached => StopReason::RateLimited,
        RunOutcome::Failed => StopReason::Error,
        RunOutcome::Cancelled => StopReason::Cancelled,
        RunOutcome::Completed | RunOutcome::EndTurn | RunOutcome::Blocked => StopReason::EndTurn,
    };
    let ended = pb::TurnEnded {
        stop_reason: stop_reason.into(),
        detail: run.detail.to_owned(),
        interrupted_by: String::new(),
    };
    writer
        .push(
            tx,
            "turn_ended",
            event_of(turn, event::Kind::TurnEnded(ended)),
        )
        .await
}

/// The events of the branch push and the pull request of a run that completed the task.
async fn write_pull_request_events(
    tx: &mut Transaction<'_, Postgres>,
    task: &Written,
    spec: &DemoTask,
    run_id: Uuid,
    run: &DemoRun,
) -> Result<()> {
    let Some(pr) = spec.pr.filter(|pr| pr.attached_branch.is_none()) else {
        return Ok(());
    };
    let url = pr_url(spec.project, pr.number).context("project of the task")?;
    let lock = lock_run_and_task(tx, RunId::from_uuid(run_id), TaskId::from_uuid(task.id)).await?;
    let events = [
        (
            "branch-pushed",
            forge_event(ForgeActivity {
                kind: Some(forge_activity::Kind::BranchPushed(BranchPushed {
                    r#ref: format!("refs/heads/factory/task/{}", task.key),
                    old_sha: "0".repeat(40),
                    new_sha: fake_sha(&task.key),
                    verdict: pb::PushVerdict::Accepted.into(),
                    reason: String::new(),
                })),
            }),
            run.minutes * 60 - 60,
        ),
        (
            "pull-request-opened",
            forge_event(ForgeActivity {
                kind: Some(forge_activity::Kind::PullRequestOpened(PullRequestOpened {
                    number: u64::try_from(pr.number)?,
                    url,
                    draft: false,
                })),
            }),
            run.minutes * 60 - 30,
        ),
    ];
    for (key, event, offset) in events {
        let id = append_control_event(tx, &lock, key, &event).await?;
        set_event_time(tx, id, run.start_ago - offset).await?;
    }
    Ok(())
}

/// Stores the artifacts of the task and the events that say a run wrote them.
async fn write_artifacts(
    tx: &mut Transaction<'_, Postgres>,
    task: &Written,
    run_id: Uuid,
    run: &DemoRun,
    artifacts: &[Artifact],
) -> Result<()> {
    if artifacts.is_empty() {
        return Ok(());
    }
    let lock = lock_run_and_task(tx, RunId::from_uuid(run_id), TaskId::from_uuid(task.id)).await?;
    for (index, artifact) in artifacts.iter().enumerate() {
        let bytes: &[u8] = match artifact.content {
            Content::Text(text) => text.as_bytes(),
            Content::Image(bytes) => bytes,
        };
        let sniffed = check_artifact(artifact.name, bytes, &ArtifactsConfig::default())?;
        let (width, height) = sniffed.dimensions.map_or((None, None), |(w, h)| {
            (i32::try_from(w).ok(), i32::try_from(h).ok())
        });
        let written_ago = run.start_ago - run.minutes * 60 + 30 + i64::try_from(index)? * 20;
        sqlx::query(
            "INSERT INTO task_artifacts (task_id, name, kind, mime_type, size, width, height,
                                        content, sha256, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9,
                     now() - make_interval(secs => $10), now() - make_interval(secs => $10))",
        )
        .bind(task.id)
        .bind(artifact.name)
        .bind(sniffed.kind.as_str())
        .bind(sniffed.mime_type)
        .bind(i64::try_from(bytes.len())?)
        .bind(width)
        .bind(height)
        .bind(bytes)
        .bind(df_db::sha256_hex(bytes))
        .bind(secs(written_ago))
        .execute(&mut **tx)
        .await?;
        let event = factory_event(FactoryActivity {
            kind: Some(factory_activity::Kind::ArtifactWritten(ArtifactWritten {
                name: artifact.name.to_owned(),
                kind: match sniffed.kind {
                    ArtifactKind::Text => pb::ArtifactKind::Text,
                    ArtifactKind::Image => pb::ArtifactKind::Image,
                }
                .into(),
                mime_type: sniffed.mime_type.to_owned(),
                size: bytes.len() as u64,
            })),
        });
        let id =
            append_control_event(tx, &lock, &format!("artifact:{}", artifact.name), &event).await?;
        set_event_time(tx, id, written_ago).await?;
    }
    Ok(())
}

async fn set_event_time(
    tx: &mut Transaction<'_, Postgres>,
    id: Option<i64>,
    ago: i64,
) -> Result<()> {
    sqlx::query("UPDATE transcript_events SET occurred_at = now() - make_interval(secs => $2) WHERE id = $1")
        .bind(id)
        .bind(secs(ago))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// The model requests of a run: the totals of each [`Usage`] spread evenly over its requests.
async fn insert_usage(
    tx: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
    run: &DemoRun,
) -> Result<()> {
    for usage in &run.usage {
        let Usage {
            model,
            requests,
            input,
            output,
            cache_read,
            cache_write,
        } = *usage;
        let wire_api = if model == GPT {
            "openai-chat"
        } else {
            "anthropic-messages"
        };
        for request in 0..requests {
            // The last request takes the remainder.
            let share = |total: i64| {
                if request + 1 == requests {
                    total - total / requests * (requests - 1)
                } else {
                    total / requests
                }
            };
            let at = run.start_ago - run.minutes * 60 * (request + 1) / (requests + 1);
            sqlx::query(
                "INSERT INTO model_usage (request_id, run_id, recorded_at, model, wire_api,
                     upstream_status, input_tokens, output_tokens, cache_read_tokens,
                     cache_write_tokens, usage_known)
                 VALUES ($1, $2, now() - make_interval(secs => $3), $4, $5, 200, $6, $7, $8, $9, true)",
            )
            .bind(Uuid::now_v7())
            .bind(run_id)
            .bind(secs(at))
            .bind(model)
            .bind(wire_api)
            .bind(share(input))
            .bind(share(output))
            .bind(share(cache_read))
            .bind(share(cache_write))
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

/// The pending question of a blocked task, raised by its last run.
async fn write_question(
    tx: &mut Transaction<'_, Postgres>,
    task: &Written,
    spec: &DemoTask,
    question: &super::data::Question,
) -> Result<()> {
    let run = spec.runs.last().context("a blocked task has a run")?;
    sqlx::query(
        "INSERT INTO agent_requests (id, task_id, run_id, kind, payload, blocking, fingerprint,
                                     state, created_at)
         VALUES ($1, $2, (SELECT id FROM runs WHERE task_id = $2 AND number = $3), 'question', $4,
                 true, encode(sha256(convert_to($5, 'UTF8')), 'hex'), 'pending',
                 now() - make_interval(secs => $6))",
    )
    .bind(Uuid::now_v7())
    .bind(task.id)
    .bind(i64::try_from(spec.runs.len())?)
    .bind(serde_json::json!({
        "text": question.text,
        "suggested_answers": question.suggested_answers,
    }))
    .bind(question.text.to_lowercase())
    .bind(secs(run.start_ago - run.minutes * 60 + 60))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Appends the events of one run, spread evenly over its length.
struct Transcript {
    task: Uuid,
    run: Uuid,
    /// Seconds before the seeding that the run started.
    started_ago: i64,
    /// Seconds the run lasts.
    length: i64,
    seq: i64,
}

impl Transcript {
    async fn push(
        &mut self,
        tx: &mut Transaction<'_, Postgres>,
        kind: &str,
        event: Event,
    ) -> Result<()> {
        self.seq += 1;
        // Events of a run happen in its first 90 %, one every 20 seconds at most.
        let offset = (self.length * 9 / 10).min(self.seq * 20);
        sqlx::query(
            "INSERT INTO transcript_events (task_id, run_id, seq, schema_version, occurred_at, kind, payload)
             VALUES ($1, $2, $3, 1, now() - make_interval(secs => $4), $5, $6)",
        )
        .bind(self.task)
        .bind(self.run)
        .bind(self.seq)
        .bind(secs(self.started_ago - offset))
        .bind(kind)
        .bind(serde_json::to_value(&event)?)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}

fn event_of(turn: &str, kind: event::Kind) -> Event {
    Event {
        schema_version: 1,
        occurred_at: None,
        turn_id: turn.to_owned(),
        kind: Some(kind),
    }
}

fn pb_principal_system(reason: &str) -> pb::Principal {
    pb::Principal {
        kind: Some(pb::principal::Kind::System(pb::SystemPrincipal {
            reason: reason.to_owned(),
        })),
    }
}

fn pb_principal_human(name: &str) -> pb::Principal {
    pb::Principal {
        kind: Some(pb::principal::Kind::Human(pb::HumanPrincipal {
            user_id: human_id(name).to_string(),
            display_name: name.to_owned(),
        })),
    }
}

fn tool_kind(kind: ToolKind) -> pb::ToolKind {
    match kind {
        ToolKind::Read => pb::ToolKind::Read,
        ToolKind::Edit => pb::ToolKind::Edit,
        ToolKind::Execute => pb::ToolKind::Execute,
        ToolKind::Search => pb::ToolKind::Search,
    }
}

/// The same board member gets the same id in every task: the first 16 bytes of the name.
fn human_id(name: &str) -> UserId {
    let mut bytes = [0_u8; 16];
    for (slot, byte) in bytes.iter_mut().zip(name.bytes()) {
        *slot = byte;
    }
    UserId::from_uuid(Uuid::from_bytes(bytes))
}

/// A commit id that looks real and is the same for the same task.
fn fake_sha(key: &str) -> String {
    df_db::sha256_hex(key.as_bytes()).chars().take(40).collect()
}

fn secs(seconds: i64) -> Secs {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a few days in seconds fit a f64 exactly"
    )]
    let seconds = seconds as f64;
    seconds
}
