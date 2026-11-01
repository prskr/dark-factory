//! Everything derived from database rows for a run: the loaded run context, `Assign` and the
//! prompts to deliver (design D1, D5, D8).

use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use df_core::config::ClaudeCodeConfig;
use df_core::{CredentialRef, CredentialRefId, PersonaId, Principal, ProjectId, TaskId};
use df_db::{
    TaskMessageRow, TaskRow, claim_held_back_messages, get_task, list_undelivered_messages,
};
use df_proto::agent::v1::{
    AcpHarness, Assign, Budgets, CommitIdentity, Continuation, Harness, Prompt, TaskRef,
    ToolAnnotations, ToolDescriptor, Workspace, harness,
};
use df_proto::pbjson_types::{Duration, Timestamp};
use df_proto::transcript::v1 as transcript;
use serde::Deserialize;
use sqlx::PgPool;

use crate::error::ChannelError;
use crate::runs::RunRow;
use crate::{ChannelContext, GitProxy};

const FACTORY_TOOLS_JSON: &str = include_str!("../../../contract/mcp/factory-tools.v2.json");
/// Upper bound of `Continuation.summary`.
const SUMMARY_MAX_BYTES: usize = 16 * 1024;
/// Share of the summary reserved for task title and description.
const SUMMARY_HEADER_MAX_BYTES: usize = 4 * 1024;
const SUMMARY_MESSAGES: usize = 20;
/// Transcript rows read to find the last [`SUMMARY_MESSAGES`] text messages (chunks included).
const SUMMARY_ROW_WINDOW: i64 = 500;

/// Project columns the channel needs.
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub id: ProjectId,
    pub key: String,
    pub repo_url: String,
    pub image: String,
    pub base_branch: String,
    /// Command that starts the adapter in the project image; empty means the harness image.
    pub adapter_command: Vec<String>,
}

impl ProjectInfo {
    /// The command that starts the adapter: the project's own, otherwise the harness default.
    pub fn adapter_command(&self, harness: &ClaudeCodeConfig) -> Vec<String> {
        if self.adapter_command.is_empty() {
            harness.adapter_command.clone()
        } else {
            self.adapter_command.clone()
        }
    }

    /// Whether the sandbox mounts the harness image; not when the project image brings its own
    /// adapter.
    pub fn mounts_harness(&self) -> bool {
        self.adapter_command.is_empty()
    }
}

/// Persona columns the channel needs.
#[derive(Debug, Clone)]
pub struct PersonaInfo {
    pub id: PersonaId,
    pub name: String,
    pub system_prompt: String,
    pub harness: String,
    pub model: Option<String>,
    pub harness_credential: Option<CredentialRef>,
}

/// A run with its task, project and persona, loaded once per stream.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub run: RunRow,
    pub task: TaskRow,
    pub project: ProjectInfo,
    pub persona: PersonaInfo,
}

impl RunContext {
    pub fn task_branch(&self) -> String {
        self.task.key.task_branch()
    }

    pub fn wip_branch(&self) -> String {
        self.task.key.wip_branch()
    }
}

#[derive(sqlx::FromRow)]
struct CredentialColumns {
    credential_id: Option<uuid::Uuid>,
    credential_name: Option<String>,
    secret_name: Option<String>,
    secret_key: Option<String>,
}

impl CredentialColumns {
    fn into_ref(self) -> Option<CredentialRef> {
        Some(CredentialRef {
            id: CredentialRefId::from_uuid(self.credential_id?),
            name: self.credential_name?,
            secret_name: self.secret_name?,
            secret_key: self.secret_key?,
        })
    }
}

#[derive(sqlx::FromRow)]
struct ProjectRecord {
    key: String,
    repo_url: String,
    image: String,
    base_branch: String,
    adapter_command: Vec<String>,
}

#[derive(sqlx::FromRow)]
struct PersonaRecord {
    name: String,
    system_prompt: String,
    harness: String,
    model: Option<String>,
    #[sqlx(flatten)]
    credential: CredentialColumns,
}

/// Loads task, project and persona of `run`.
pub async fn load_run_context(pool: &PgPool, run: RunRow) -> Result<RunContext, ChannelError> {
    let task = get_task(pool, run.task_id)
        .await?
        .ok_or_else(|| ChannelError::InvalidData(format!("task of run {} is missing", run.id)))?;

    let project: ProjectRecord = sqlx::query_as(
        "SELECT key, repo_url, image, base_branch, adapter_command FROM projects WHERE id = $1",
    )
    .bind(task.project_id.as_uuid())
    .fetch_one(pool)
    .await?;
    let project = ProjectInfo {
        id: task.project_id,
        key: project.key,
        repo_url: project.repo_url,
        image: project.image,
        base_branch: project.base_branch,
        adapter_command: project.adapter_command,
    };

    let persona: PersonaRecord = sqlx::query_as(
        "SELECT p.name, p.system_prompt, p.harness, p.model,
                c.id AS credential_id, c.name AS credential_name, c.secret_name, c.secret_key
         FROM personas p LEFT JOIN credential_refs c ON c.id = p.harness_credential_id
         WHERE p.id = $1",
    )
    .bind(task.persona_id.as_uuid())
    .fetch_one(pool)
    .await?;
    let persona = PersonaInfo {
        id: task.persona_id,
        name: persona.name,
        system_prompt: persona.system_prompt,
        harness: persona.harness,
        model: persona.model,
        harness_credential: persona.credential.into_ref(),
    };

    Ok(RunContext {
        run,
        task,
        project,
        persona,
    })
}

#[derive(Deserialize)]
struct ToolsFile {
    tools: Vec<ToolEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolEntry {
    name: String,
    title: String,
    description: String,
    input_schema: serde_json::Value,
    #[serde(default)]
    output_schema: Option<serde_json::Value>,
    #[serde(default)]
    annotations: AnnotationsEntry,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AnnotationsEntry {
    #[serde(default)]
    read_only_hint: bool,
    #[serde(default)]
    destructive_hint: bool,
    #[serde(default)]
    idempotent_hint: bool,
    #[serde(default)]
    open_world_hint: bool,
}

static TOOL_ENTRIES: LazyLock<ToolsFile> = LazyLock::new(|| {
    serde_json::from_str(FACTORY_TOOLS_JSON).expect("embedded factory-tools.v2.json is valid")
});

/// JSON Schema of the arguments of every factory tool, by name.
pub(crate) fn tool_input_schemas()
-> impl Iterator<Item = (&'static str, &'static serde_json::Value)> {
    TOOL_ENTRIES
        .tools
        .iter()
        .map(|t| (t.name.as_str(), &t.input_schema))
}

/// The factory tools v2 as sent in `Assign.tools`.
pub fn tool_descriptors() -> Vec<ToolDescriptor> {
    TOOL_ENTRIES
        .tools
        .iter()
        .map(|t| ToolDescriptor {
            name: t.name.clone(),
            title: t.title.clone(),
            description: t.description.clone(),
            input_schema_json: t.input_schema.to_string(),
            output_schema_json: t.output_schema.as_ref().map(ToString::to_string),
            annotations: Some(ToolAnnotations {
                read_only: t.annotations.read_only_hint,
                destructive: t.annotations.destructive_hint,
                idempotent: t.annotations.idempotent_hint,
                open_world: t.annotations.open_world_hint,
            }),
        })
        .collect()
}

/// Builds `Assign` for the run; identical for every call for the same run.
pub async fn build_assign(ctx: &ChannelContext, rc: &RunContext) -> Result<Assign, ChannelError> {
    let config = &ctx.config;
    let command = rc.project.adapter_command(&config.harness.claude_code);
    Ok(Assign {
        run_id: rc.run.id.to_string(),
        task: Some(TaskRef {
            task_id: rc.task.id.to_string(),
            task_key: rc.task.key.to_string(),
            project_key: rc.project.key.clone(),
            title: rc.task.title.clone(),
        }),
        workspace: Some(Workspace {
            repo_url: proxy_repo_url(&config.git_proxy, &rc.task.key.to_string()),
            base_branch: rc.project.base_branch.clone(),
            task_branch: rc.task_branch(),
            wip_branch: rc.wip_branch(),
            commit_identity: Some(CommitIdentity {
                name: config.agent.commit_identity.name.clone(),
                email: config.agent.commit_identity.email.clone(),
            }),
            git_hooks: Vec::new(),
            git_config: Vec::new(),
        }),
        harness: Some(Harness {
            system_prompt: rc.persona.system_prompt.clone(),
            kind: Some(harness::Kind::Acp(AcpHarness {
                command,
                env: config.harness.claude_code.env.clone().into_iter().collect(),
                model: rc.persona.model.clone(),
                settings_json: config.harness.claude_code.settings_json.clone(),
            })),
        }),
        tools: tool_descriptors(),
        budgets: Some(Budgets {
            max_wall_clock: Some(Duration {
                seconds: i64::try_from(config.runs.max_wall_clock_seconds).unwrap_or(i64::MAX),
                nanos: 0,
            }),
            max_output_tokens: None,
            max_turns: None,
        }),
        continuation: continuation(&ctx.pool, rc).await?,
    })
}

/// The task's repository URL on the Git proxy, `https://<host>:<port>/git/<KEY>.git`; the
/// project's real repository URL never reaches the sandbox.
pub fn proxy_repo_url(proxy: &GitProxy, task_key: &str) -> String {
    if proxy.port == 443 {
        format!("https://{}/git/{task_key}.git", proxy.host)
    } else {
        format!("https://{}:{}/git/{task_key}.git", proxy.host, proxy.port)
    }
}

async fn continuation(
    pool: &PgPool,
    rc: &RunContext,
) -> Result<Option<Continuation>, ChannelError> {
    if rc.run.number <= 1 {
        return Ok(None);
    }
    let previous: Option<String> = sqlx::query_scalar(
        "SELECT harness_session_id FROM runs
         WHERE task_id = $1 AND number < $2 AND harness_session_id <> ''
         ORDER BY number DESC LIMIT 1",
    )
    .bind(rc.task.id.as_uuid())
    .bind(rc.run.number)
    .fetch_optional(pool)
    .await?;
    Ok(Some(Continuation {
        harness_session_id: previous.unwrap_or_else(|| rc.task.harness_session_id.clone()),
        summary: transcript_summary(pool, rc).await?,
    }))
}

/// Task title and description, then the last text messages of the transcript, within 16 KiB
/// (design D5). *ponytail:* no model-generated summary.
pub async fn transcript_summary(pool: &PgPool, rc: &RunContext) -> Result<String, ChannelError> {
    let rows: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT kind, payload FROM transcript_events
         WHERE task_id = $1 AND kind IN ('user_message', 'agent_message_chunk')
         ORDER BY id DESC LIMIT $2",
    )
    .bind(rc.task.id.as_uuid())
    .bind(SUMMARY_ROW_WINDOW)
    .fetch_all(pool)
    .await?;

    // (speaker, message id, text); consecutive chunks of one agent message are merged.
    let mut messages: Vec<(&'static str, String, String)> = Vec::new();
    for (kind, payload) in rows.into_iter().rev() {
        let (speaker, root) = if kind == "user_message" {
            ("User", "/userMessage")
        } else {
            ("Agent", "/agentMessageChunk")
        };
        let body = payload.pointer(root);
        let text = match speaker {
            "User" => body
                .and_then(|body| body.get("content"))
                .and_then(serde_json::Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter(|b| {
                            b.get("hidden").and_then(serde_json::Value::as_bool) != Some(true)
                        })
                        .filter_map(|b| b.pointer("/text/text").and_then(serde_json::Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default(),
            _ => body
                .and_then(|body| body.pointer("/content/text/text"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
        };
        if text.is_empty() {
            continue;
        }
        let message_id = body
            .and_then(|body| body.get("messageId"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        match messages.last_mut() {
            Some((s, id, acc)) if speaker == "Agent" && *s == "Agent" && *id == message_id => {
                acc.push_str(&text);
            }
            _ => messages.push((speaker, message_id, text)),
        }
    }
    let skip = messages.len().saturating_sub(SUMMARY_MESSAGES);
    let header = format!("# {}\n\n{}", rc.task.title, rc.task.description);
    let entries = messages
        .into_iter()
        .skip(skip)
        .map(|(who, _, text)| format!("{who}: {text}"))
        .collect();
    Ok(fit_summary(header, entries))
}

/// Joins `header` and `entries` (oldest first) within [`SUMMARY_MAX_BYTES`]. The header is cut
/// at [`SUMMARY_HEADER_MAX_BYTES`]; entries are taken newest first until the space is used, so
/// the most recent messages survive.
fn fit_summary(header: String, entries: Vec<String>) -> String {
    const SEPARATOR: &str = "\n\n";
    let header = truncate_utf8(header, SUMMARY_HEADER_MAX_BYTES);
    let mut remaining = SUMMARY_MAX_BYTES - header.len();
    let mut kept: Vec<String> = Vec::new();
    for entry in entries.into_iter().rev() {
        let cost = entry.len() + SEPARATOR.len();
        if cost <= remaining {
            remaining -= cost;
            kept.push(entry);
        } else {
            // The newest message always makes it in, shortened if needed.
            if kept.is_empty() && remaining > SEPARATOR.len() {
                kept.push(truncate_utf8(entry, remaining - SEPARATOR.len()));
            }
            break;
        }
    }
    kept.reverse();
    std::iter::once(header)
        .chain(kept)
        .collect::<Vec<_>>()
        .join(SEPARATOR)
}

fn truncate_utf8(mut s: String, max_bytes: usize) -> String {
    if s.len() > max_bytes {
        let mut end = max_bytes;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

/// The prompts of the task that were forwarded to the runtime and are not yet confirmed as
/// delivered, oldest first. Unless the runtime is `busy` in a turn, the held-back prompts are
/// forwarded first (marked in the database), so they are part of the result.
pub async fn pending_prompts(
    pool: &PgPool,
    task_id: TaskId,
    busy: bool,
) -> Result<Vec<Prompt>, ChannelError> {
    if !busy {
        claim_held_back_messages(pool, task_id).await?;
    }
    Ok(list_undelivered_messages(pool, task_id)
        .await?
        .into_iter()
        .filter(|row| row.forwarded_at.is_some())
        .map(prompt_from_row)
        .collect())
}

fn prompt_from_row(row: TaskMessageRow) -> Prompt {
    Prompt {
        prompt_id: row.id.to_string(),
        author: Some(principal(&row.author)),
        content: row.content,
        queued_at: Some(timestamp(row.queued_at)),
    }
}

fn timestamp(at: DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: at.timestamp(),
        nanos: i32::try_from(at.timestamp_subsec_nanos()).unwrap_or(0),
    }
}

fn principal(p: &Principal) -> transcript::Principal {
    use transcript::principal::Kind;
    transcript::Principal {
        kind: Some(match p {
            Principal::Human {
                user_id,
                display_name,
            } => Kind::Human(transcript::HumanPrincipal {
                user_id: user_id.to_string(),
                display_name: display_name.clone(),
            }),
            Principal::Agent {
                persona_id,
                display_name,
                task_id,
                ..
            } => Kind::Agent(transcript::AgentPrincipal {
                persona_id: persona_id.to_string(),
                display_name: display_name.clone(),
                task_id: task_id.to_string(),
            }),
            Principal::System { reason } => Kind::System(transcript::SystemPrincipal {
                reason: reason.clone(),
            }),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::get_run;
    use crate::testing::Env;

    fn project(command: &[&str]) -> ProjectInfo {
        ProjectInfo {
            id: ProjectId::new(),
            key: "DF".into(),
            repo_url: "https://git.example.org/o/r.git".into(),
            image: "img".into(),
            base_branch: "main".into(),
            adapter_command: command.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn default_adapter_command_comes_from_the_harness_and_the_image_is_mounted() {
        let project = project(&[]);
        let harness = ClaudeCodeConfig::default();
        assert_eq!(project.adapter_command(&harness), harness.adapter_command);
        assert!(project.mounts_harness());
    }

    #[test]
    fn project_adapter_command_replaces_the_default_and_skips_the_mount() {
        let project = project(&["/opt/harness/bin/fake-acp-agent"]);
        assert_eq!(
            project.adapter_command(&ClaudeCodeConfig::default()),
            ["/opt/harness/bin/fake-acp-agent"]
        );
        assert!(!project.mounts_harness());
    }

    #[test]
    fn embedded_tools_are_exactly_the_factory_tools() {
        let names: Vec<_> = tool_descriptors().into_iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "update_pull_request",
                "comment_on_issue",
                "report_progress",
                "set_task_title",
                "complete_task",
                "ask_question",
                "request_approval"
            ]
        );
        let first = &tool_descriptors()[0];
        assert!(serde_json::from_str::<serde_json::Value>(&first.input_schema_json).is_ok());
        assert!(first.annotations.as_ref().is_some_and(|a| a.idempotent));
    }

    #[test]
    fn summary_keeps_the_newest_messages_when_it_has_to_cut() {
        let header = "x".repeat(10 * 1024);
        let entries: Vec<String> = (0..20)
            .map(|i| format!("Agent: msg-{i:02} {}", "y".repeat(1000)))
            .collect();

        let summary = fit_summary(header, entries);

        assert!(summary.len() <= SUMMARY_MAX_BYTES);
        assert!(summary.contains("msg-19"), "newest message must survive");
        assert!(
            !summary.contains("msg-00"),
            "oldest messages are dropped first"
        );
        let kept: Vec<usize> = (0..20)
            .filter(|i| summary.contains(&format!("msg-{i:02}")))
            .collect();
        assert_eq!(
            kept,
            (kept[0]..20).collect::<Vec<_>>(),
            "kept messages are contiguous and in order"
        );
    }

    #[test]
    fn a_single_oversized_message_is_shortened_not_dropped() {
        let summary = fit_summary("# t".into(), vec![format!("User: {}", "z".repeat(40_000))]);
        assert!(summary.len() <= SUMMARY_MAX_BYTES);
        assert!(summary.contains("User: zzz"));
    }

    #[test]
    fn truncation_keeps_char_boundaries() {
        assert_eq!(truncate_utf8("aäb".into(), 2), "a");
        assert_eq!(truncate_utf8("abc".into(), 10), "abc");
    }

    #[tokio::test]
    async fn assign_for_the_first_run_of_df_1() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run_id = env.insert_run(env.task.id, 1, "connecting").await;
        let run = get_run(&pool, run_id).await?.expect("run");
        let rc = load_run_context(&pool, run).await?;

        let assign = build_assign(&env.ctx, &rc).await?;

        let workspace = assign.workspace.expect("workspace");
        assert_eq!(workspace.task_branch, "factory/task/DF-1");
        assert_eq!(workspace.wip_branch, "factory/wip/DF-1");
        assert_eq!(workspace.base_branch, "main");
        assert_eq!(assign.tools.len(), 7);
        assert_eq!(assign.task.expect("task").task_key, "DF-1");
        assert!(assign.continuation.is_none());
        let Some(harness::Kind::Acp(acp)) = assign.harness.expect("harness").kind else {
            anyhow::bail!("expected ACP harness");
        };
        assert_eq!(
            acp.command,
            ["/opt/harness/claude-code/bin/claude-agent-acp"]
        );
        assert_eq!(
            acp.command,
            env.ctx.config.harness.claude_code.adapter_command
        );
        assert_eq!(
            assign
                .budgets
                .expect("budgets")
                .max_wall_clock
                .expect("limit")
                .seconds,
            7200
        );
        Ok(())
    }

    #[test]
    fn proxy_url_names_the_task_on_the_gateway_and_omits_port_443() {
        let proxy = |port| GitProxy {
            host: "df-gateway.df.svc".into(),
            port,
        };
        assert_eq!(
            proxy_repo_url(&proxy(8443), "DF-7"),
            "https://df-gateway.df.svc:8443/git/DF-7.git"
        );
        assert_eq!(
            proxy_repo_url(&proxy(443), "DF-7"),
            "https://df-gateway.df.svc/git/DF-7.git"
        );
    }

    #[tokio::test]
    async fn assign_carries_the_repository_through_the_proxy() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let mut env = Env::new(pool.clone()).await;
        env.ctx.config = std::sync::Arc::new(crate::ChannelConfig {
            git_proxy: GitProxy {
                host: "df-gateway.df.svc".into(),
                port: 8443,
            },
            ..crate::ChannelConfig::default()
        });
        let run_id = env.insert_run(env.task.id, 1, "connecting").await;
        let run = get_run(&pool, run_id).await?.expect("run");
        let rc = load_run_context(&pool, run).await?;

        let assign = build_assign(&env.ctx, &rc).await?;

        let workspace = assign.workspace.expect("workspace");
        assert_eq!(
            workspace.repo_url,
            "https://df-gateway.df.svc:8443/git/DF-1.git"
        );
        assert_eq!(workspace.task_branch, "factory/task/DF-1");
        Ok(())
    }

    #[tokio::test]
    async fn assign_uses_the_project_adapter_command_when_set() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        sqlx::query(
            "UPDATE projects SET adapter_command = ARRAY['/opt/harness/bin/fake-acp-agent']",
        )
        .execute(&pool)
        .await?;
        let run_id = env.insert_run(env.task.id, 1, "connecting").await;
        let run = get_run(&pool, run_id).await?.expect("run");
        let rc = load_run_context(&pool, run).await?;

        let assign = build_assign(&env.ctx, &rc).await?;

        let Some(harness::Kind::Acp(acp)) = assign.harness.expect("harness").kind else {
            anyhow::bail!("expected ACP harness");
        };
        assert_eq!(acp.command, ["/opt/harness/bin/fake-acp-agent"]);
        Ok(())
    }

    #[tokio::test]
    async fn follow_up_run_carries_session_id_and_summary() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run1 = env.insert_run(env.task.id, 1, "running").await;
        sqlx::query("UPDATE runs SET state = 'succeeded', harness_session_id = 'S' WHERE id = $1")
            .bind(run1.as_uuid())
            .execute(&pool)
            .await?;
        let run2 = env.insert_run(env.task.id, 2, "connecting").await;
        let run = get_run(&pool, run2).await?.expect("run");
        let rc = load_run_context(&pool, run).await?;

        let continuation = build_assign(&env.ctx, &rc)
            .await?
            .continuation
            .expect("continuation");
        assert_eq!(continuation.harness_session_id, "S");
        assert!(continuation.summary.contains("Fix login"));
        Ok(())
    }

    #[tokio::test]
    async fn summary_merges_agent_chunks_and_keeps_the_last_messages() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        let run = env.insert_run(env.task.id, 1, "running").await;
        for (seq, (kind, payload)) in [
            ("user_message", serde_json::json!({"userMessage": {"content": [{"text": {"text": "please fix"}}, {"hidden": true, "text": {"text": "secret instruction"}}]}})),
            ("agent_message_chunk", serde_json::json!({"agentMessageChunk": {"messageId": "m", "content": {"text": {"text": "on "}}}})),
            ("agent_message_chunk", serde_json::json!({"agentMessageChunk": {"messageId": "m", "content": {"text": {"text": "it"}}}})),
        ]
        .into_iter()
        .enumerate()
        {
            sqlx::query(
                "INSERT INTO transcript_events (task_id, run_id, seq, occurred_at, kind, payload)
                 VALUES ($1, $2, $3, now(), $4, $5)",
            )
            .bind(env.task.id.as_uuid())
            .bind(run.as_uuid())
            .bind(i64::try_from(seq)? + 1)
            .bind(kind)
            .bind(payload)
            .execute(&pool)
            .await?;
        }
        let rc = load_run_context(&pool, get_run(&pool, run).await?.expect("run")).await?;
        let summary = transcript_summary(&pool, &rc).await?;
        assert!(summary.contains("User: please fix"), "{summary}");
        assert!(summary.contains("Agent: on it"), "{summary}");
        assert!(!summary.contains("secret instruction"), "{summary}");
        Ok(())
    }

    #[test]
    fn principals_convert_to_the_wire_type() {
        let p = principal(&Principal::system("task.assigned"));
        assert!(
            matches!(p.kind, Some(transcript::principal::Kind::System(s)) if s.reason == "task.assigned")
        );
    }
}
