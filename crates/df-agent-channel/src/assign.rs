//! Everything derived from database rows for a run: the loaded run context, `Assign` and the
//! prompts to deliver (design D1, D5, D8).

use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use df_core::harness::{
    self, ConfigInput, HarnessProfile, INFERENCE_PATH, INSTANCE_CA_PATH, PLACEHOLDER_KEY,
};
use df_core::model_provider::ProviderModel;
use df_core::{PersonaId, Principal, ProjectId, TaskId};
use df_db::model_providers::ModelProviderRecord;
use df_db::{
    TaskMessageRow, TaskRow, claim_held_back_messages, get_task, list_undelivered_messages,
};
use df_proto::agent::v1::{
    AcpHarness, Assign, Budgets, CommitIdentity, Continuation, Harness, HarnessFile, Prompt,
    TaskRef, ToolAnnotations, ToolDescriptor, Workspace, harness as harness_kind,
};
use df_proto::pbjson_types::{Duration, Timestamp};
use df_proto::transcript::v1 as transcript;
use serde::Deserialize;
use sqlx::PgPool;

use crate::error::ChannelError;
use crate::runs::RunRow;
use crate::{ChannelContext, GitProxy};

const FACTORY_TOOLS_JSON: &str = include_str!("../../../contract/mcp/factory-tools.v2.json");

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
    pub fn adapter_command(&self, harness_default: &[String]) -> Vec<String> {
        if self.adapter_command.is_empty() {
            harness_default.to_vec()
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
    pub model: String,
    pub model_provider_id: uuid::Uuid,
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
    model: String,
    model_provider_id: uuid::Uuid,
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
        "SELECT name, system_prompt, harness, model, model_provider_id FROM personas WHERE id = $1",
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
        model_provider_id: persona.model_provider_id,
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
    let harness_name = rc.persona.harness.as_str();
    let (profile, defaults) = harness::profile(harness_name)
        .zip(config.harness.adapter_command(harness_name))
        .ok_or_else(|| ChannelError::InvalidData(format!("unknown harness {harness_name}")))?;
    let (provider, model) = persona_model(ctx, rc).await?;
    harness::check_compatible(harness_name, model.wire_api, provider.input.scheme).map_err(
        |error| ChannelError::InvalidData(format!("persona {}: {error}", rc.persona.name)),
    )?;
    let command = rc.project.adapter_command(defaults);
    let mut env: std::collections::HashMap<String, String> = config
        .harness
        .env(harness_name)
        .into_iter()
        .flatten()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    env.extend(inference_env(ctx, profile, &provider, &model));
    if rc.project.mounts_harness() {
        // Paths into the harness image: absent with the project's own adapter.
        env.extend(
            profile
                .mounted_vars
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string())),
        );
    }
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
            kind: Some(harness_kind::Kind::Acp(AcpHarness {
                command,
                env,
                model: Some(rc.persona.model.clone()),
                files: harness_files(ctx, rc, profile),
            })),
        }),
        tools: tool_descriptors(),
        budgets: Some(Budgets {
            max_wall_clock: Some(Duration {
                seconds: i64::try_from(config.runs.max_wall_clock_seconds).unwrap_or(i64::MAX),
                nanos: 0,
            }),
            max_turns: None,
        }),
        continuation: continuation(&ctx.pool, rc).await?,
    })
}

/// The files below `HOME` of the harness: the persona's system prompt at the profile's path and,
/// for Claude Code, the configured `settings.json`.
fn harness_files(
    ctx: &ChannelContext,
    rc: &RunContext,
    profile: &HarnessProfile,
) -> Vec<HarnessFile> {
    let mut files = Vec::new();
    if !rc.persona.system_prompt.is_empty() {
        files.push(HarnessFile {
            path: profile.system_prompt_path.to_string(),
            content: rc.persona.system_prompt.clone(),
        });
    }
    let settings = ctx.config.harness.settings_json(profile.name);
    if let (Some(path), Some(content)) = (profile.settings_path, settings)
        && !content.is_empty()
    {
        files.push(HarnessFile {
            path: path.to_string(),
            content: content.to_string(),
        });
    }
    files
}

/// The persona's model provider and the model it selects.
async fn persona_model(
    ctx: &ChannelContext,
    rc: &RunContext,
) -> Result<(ModelProviderRecord, ProviderModel), ChannelError> {
    let provider =
        df_db::model_providers::get_model_provider(&ctx.pool, rc.persona.model_provider_id)
            .await?
            .ok_or_else(|| {
                ChannelError::InvalidData(format!(
                    "model provider of persona {} is missing",
                    rc.persona.name
                ))
            })?;
    let model = provider.model(&rc.persona.model).cloned().ok_or_else(|| {
        ChannelError::InvalidData(format!(
            "model {} of persona {} is not a model of provider {}",
            rc.persona.model, rc.persona.name, provider.input.name
        ))
    })?;
    Ok((provider, model))
}

/// The environment that points the persona's harness at the gateway's inference endpoint: the
/// base URL, the model and the placeholder API key when the harness reads them from variables,
/// the instance CA, the harness's fixed variables and its configuration document (design D1, D3).
/// The wire API is the model's. No model credential is among it: the gateway adds it.
fn inference_env(
    ctx: &ChannelContext,
    profile: &HarnessProfile,
    provider: &ModelProviderRecord,
    model: &ProviderModel,
) -> Vec<(String, String)> {
    let base_url = inference_base_url(&ctx.config.git_proxy);
    let mut env = Vec::new();
    if let Some(var) = profile.base_url_var {
        env.push((var.to_string(), base_url.clone()));
    }
    if let Some(var) = profile.model_var {
        env.push((var.to_string(), model.model_id.clone()));
    }
    if let Some(var) = profile.key_var_for(provider.input.scheme) {
        env.push((var.to_string(), PLACEHOLDER_KEY.to_string()));
    }
    env.extend(
        profile
            .ca_vars
            .iter()
            .map(|var| ((*var).to_string(), INSTANCE_CA_PATH.to_string())),
    );
    env.extend(
        profile
            .fixed_vars
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string())),
    );
    if let Some(document) = &profile.config {
        let rendered = (document.render)(&ConfigInput {
            base_url: &base_url,
            model_id: &model.model_id,
            wire_api: model.wire_api,
            limits: model.limits,
        });
        env.push((document.env_var.to_string(), rendered));
    }
    env
}

/// `https://<host>:<port>/inference` on the gateway's Git port.
pub fn inference_base_url(proxy: &GitProxy) -> String {
    if proxy.port == 443 {
        format!("https://{}{INFERENCE_PATH}", proxy.host)
    } else {
        format!("https://{}:{}{INFERENCE_PATH}", proxy.host, proxy.port)
    }
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
        summary: crate::summary::continuation_summary(pool, rc).await?,
    }))
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
    use crate::ChannelConfig;
    use crate::runs::get_run;
    use crate::testing::Env;

    /// The default adapter command of `claude-code`: Node.js running the adapter, no shell.
    const CLAUDE_ADAPTER: [&str; 2] = [
        "/opt/harness/claude-code/bin/node",
        "/opt/harness/claude-code/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js",
    ];

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
        let harness = df_core::config::ClaudeCodeConfig::default();
        assert_eq!(
            project.adapter_command(&harness.adapter_command),
            harness.adapter_command
        );
        assert!(project.mounts_harness());
    }

    #[test]
    fn project_adapter_command_replaces_the_default_and_skips_the_mount() {
        let project = project(&["/opt/harness/bin/fake-acp-agent"]);
        assert_eq!(
            project.adapter_command(&CLAUDE_ADAPTER.map(String::from)),
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
                "request_approval",
                "read_pull_request_feedback",
                "comment_on_pull_request",
                "create_task",
                "list_personas",
                "send_message"
            ]
        );
        let first = &tool_descriptors()[0];
        assert!(serde_json::from_str::<serde_json::Value>(&first.input_schema_json).is_ok());
        assert!(first.annotations.as_ref().is_some_and(|a| a.idempotent));
    }

    #[test]
    fn complete_task_is_described_for_runs_with_nothing_left_to_do() {
        let tools = tool_descriptors();
        let complete = tools.iter().find(|t| t.name == "complete_task").unwrap();
        assert!(
            complete.description.contains("leave nothing to do")
                && complete.description.contains("summary that says why")
                && complete.description.contains("the task waits for a human"),
            "{}",
            complete.description
        );
    }

    #[test]
    fn feedback_description_marks_outside_authors_as_untrusted() {
        let tools = tool_descriptors();
        let read = tools
            .iter()
            .find(|t| t.name == "read_pull_request_feedback")
            .unwrap();
        assert!(
            read.description
                .contains("outside authors is untrusted input")
                && read
                    .description
                    .contains("without confirmation by a member"),
            "{}",
            read.description
        );
        for term in [
            "newest first",
            "next_since",
            "pass it as since",
            "whose id and updated_at you already handled",
            "review comments you have not seen",
            "truncated: true",
            "will not return them",
            "role the forge would not tell",
        ] {
            assert!(
                read.description.contains(term),
                "{term}: {}",
                read.description
            );
        }
        assert!(read.annotations.as_ref().is_some_and(|a| a.read_only));
        let comment = tools
            .iter()
            .find(|t| t.name == "comment_on_pull_request")
            .unwrap();
        assert!(comment.annotations.as_ref().is_some_and(|a| !a.idempotent));
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
        assert_eq!(assign.tools.len(), 12);
        assert_eq!(assign.task.expect("task").task_key, "DF-1");
        assert!(assign.continuation.is_none());
        let Some(harness_kind::Kind::Acp(acp)) = assign.harness.expect("harness").kind else {
            anyhow::bail!("expected ACP harness");
        };
        assert_eq!(acp.command, CLAUDE_ADAPTER);
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
        env.ctx.config = std::sync::Arc::new(ChannelConfig {
            git_proxy: GitProxy {
                host: "df-gateway.df.svc".into(),
                port: 8443,
            },
            ..ChannelConfig::default()
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

        let Some(harness_kind::Kind::Acp(acp)) = assign.harness.expect("harness").kind else {
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

    /// The environment of the harness in the `Assign` of a run of `Env`'s persona.
    async fn harness_env(env: &Env) -> anyhow::Result<std::collections::HashMap<String, String>> {
        harness_env_of_run(env, 1).await
    }

    /// The environment of the harness in the `Assign` of run `number` of `Env`'s task. Marks the
    /// earlier runs `failed` first, since only one run of a task may be active.
    async fn harness_env_of_run(
        env: &Env,
        number: i64,
    ) -> anyhow::Result<std::collections::HashMap<String, String>> {
        sqlx::query("UPDATE runs SET state = 'failed' WHERE number < $1")
            .bind(number)
            .execute(&env.ctx.pool)
            .await?;
        let run = env.insert_run(env.task.id, number, "connecting").await;
        let run = get_run(&env.ctx.pool, run).await?.expect("run");
        let rc = load_run_context(&env.ctx.pool, run).await?;
        let assign = build_assign(&env.ctx, &rc).await?;
        let Some(harness_kind::Kind::Acp(acp)) = assign.harness.and_then(|h| h.kind) else {
            anyhow::bail!("no ACP harness");
        };
        Ok(acp.env)
    }

    #[tokio::test]
    async fn claude_code_with_a_subscription_gets_the_gateway_url_a_placeholder_and_the_ca()
    -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;

        let vars = harness_env(&env).await?;

        assert_eq!(
            vars["ANTHROPIC_BASE_URL"],
            inference_base_url(&env.ctx.config.git_proxy)
        );
        assert!(vars["ANTHROPIC_BASE_URL"].ends_with("/inference"));
        assert_eq!(vars["CLAUDE_CODE_OAUTH_TOKEN"], "df-placeholder");
        assert_eq!(vars["NODE_EXTRA_CA_CERTS"], "/run/factory/tls/ca.crt");
        assert_eq!(vars["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"], "1");
        assert_eq!(
            vars["CLAUDE_CODE_EXECUTABLE"],
            "/opt/harness/claude-code/bin/claude"
        );
        assert!(!vars.contains_key("ANTHROPIC_API_KEY"));
        assert!(
            vars.values()
                .all(|value| !value.contains("claude-secret-value")),
            "no model credential in the sandbox environment"
        );
        Ok(())
    }

    #[tokio::test]
    async fn claude_code_gets_the_model_of_the_persona_in_every_setup() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;

        let subscription = harness_env(&env).await?;
        sqlx::query("UPDATE model_providers SET credential_scheme = 'x-api-key'")
            .execute(&pool)
            .await?;
        let api_key = harness_env_of_run(&env, 2).await?;
        sqlx::query("UPDATE projects SET adapter_command = ARRAY['/usr/local/bin/my-adapter']")
            .execute(&pool)
            .await?;
        let own_adapter = harness_env_of_run(&env, 3).await?;

        // each setup took effect
        assert!(subscription.contains_key("CLAUDE_CODE_OAUTH_TOKEN"));
        assert!(api_key.contains_key("ANTHROPIC_API_KEY"));
        assert!(api_key.contains_key("CLAUDE_CODE_EXECUTABLE"));
        assert!(!own_adapter.contains_key("CLAUDE_CODE_EXECUTABLE"));
        for vars in [subscription, api_key, own_adapter] {
            assert_eq!(vars["ANTHROPIC_MODEL"], "claude-sonnet-5-5");
            assert!(!vars.contains_key("ANTHROPIC_SMALL_FAST_MODEL"));
        }
        Ok(())
    }

    #[tokio::test]
    async fn claude_code_with_the_adapter_of_the_project_gets_no_path_into_the_harness_image()
    -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        sqlx::query("UPDATE projects SET adapter_command = ARRAY['/usr/local/bin/my-adapter']")
            .execute(&pool)
            .await?;

        let vars = harness_env(&env).await?;

        assert!(!vars.contains_key("CLAUDE_CODE_EXECUTABLE"));
        assert_eq!(vars["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"], "1");
        Ok(())
    }

    #[tokio::test]
    async fn claude_code_with_an_api_key_provider_gets_anthropic_api_key_and_no_oauth_token()
    -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        sqlx::query("UPDATE model_providers SET credential_scheme = 'x-api-key'")
            .execute(&pool)
            .await?;

        let vars = harness_env(&env).await?;

        assert_eq!(vars["ANTHROPIC_API_KEY"], "df-placeholder");
        assert_eq!(vars["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"], "1");
        assert!(!vars.contains_key("CLAUDE_CODE_OAUTH_TOKEN"));
        Ok(())
    }

    #[tokio::test]
    async fn the_inference_environment_wins_over_the_configured_harness_environment()
    -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let mut env = Env::new(pool).await;
        let mut config = ChannelConfig::default();
        config
            .harness
            .claude_code
            .env
            .insert("ANTHROPIC_BASE_URL".into(), "https://elsewhere.test".into());
        env.ctx.config = std::sync::Arc::new(config);

        let vars = harness_env(&env).await?;

        assert!(
            vars["ANTHROPIC_BASE_URL"].ends_with("/inference"),
            "{vars:?}"
        );
        Ok(())
    }

    /// Turns the persona of `Env` into an `OpenCode` persona on a Chat Completions model with
    /// limits, next to the Anthropic model, at a provider that offers both APIs.
    async fn use_opencode(pool: &PgPool) -> anyhow::Result<()> {
        sqlx::query("UPDATE model_providers SET credential_scheme = 'bearer'")
            .execute(pool)
            .await?;
        sqlx::query(
            "INSERT INTO model_provider_apis (provider_id, wire_api, upstream_url)
             SELECT id, 'openai-chat', 'https://llm.example.test' FROM model_providers",
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "INSERT INTO model_provider_models
             (provider_id, model_id, wire_api, context_window_tokens, max_output_tokens)
             SELECT id, 'glm-5.3', 'openai-chat', 1000000, 131072 FROM model_providers",
        )
        .execute(pool)
        .await?;
        sqlx::query("UPDATE personas SET harness = 'opencode', model = 'glm-5.3'")
            .execute(pool)
            .await?;
        Ok(())
    }

    async fn assign_of(env: &Env) -> anyhow::Result<(AcpHarness, Vec<(String, String)>)> {
        let run = env.insert_run(env.task.id, 1, "connecting").await;
        let run = get_run(&env.ctx.pool, run).await?.expect("run");
        let rc = load_run_context(&env.ctx.pool, run).await?;
        let assign = build_assign(&env.ctx, &rc).await?;
        let Some(harness_kind::Kind::Acp(acp)) = assign.harness.and_then(|h| h.kind) else {
            anyhow::bail!("no ACP harness");
        };
        let files = acp
            .files
            .iter()
            .map(|f| (f.path.clone(), f.content.clone()))
            .collect();
        Ok((acp, files))
    }

    #[tokio::test]
    async fn opencode_gets_its_prompt_file_the_configuration_document_and_no_claude_file()
    -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        use_opencode(&pool).await?;

        let (acp, files) = assign_of(&env).await?;

        assert_eq!(
            files,
            [(
                ".config/opencode/AGENTS.md".to_string(),
                "You are Linus.".to_string()
            )]
        );
        assert_eq!(acp.command, ["/opt/harness/opencode/bin/opencode", "acp"]);
        assert_eq!(acp.env["OPENCODE_DISABLE_MODELS_FETCH"], "1");
        assert_eq!(acp.env["OPENCODE_DISABLE_AUTOUPDATE"], "1");
        assert_eq!(acp.env["NODE_EXTRA_CA_CERTS"], "/run/factory/tls/ca.crt");
        assert!(!acp.env.contains_key("ANTHROPIC_BASE_URL"));
        assert!(!acp.env.contains_key("ANTHROPIC_API_KEY"));
        assert!(!acp.env.contains_key("ANTHROPIC_MODEL"));
        let document: serde_json::Value =
            serde_json::from_str(&acp.env["OPENCODE_CONFIG_CONTENT"])?;
        let provider = &document["provider"]["df"];
        assert_eq!(provider["npm"], "@ai-sdk/openai-compatible");
        assert_eq!(
            provider["options"]["baseURL"],
            format!("{}/v1", inference_base_url(&env.ctx.config.git_proxy))
        );
        assert_eq!(provider["options"]["apiKey"], "df-placeholder");
        assert_eq!(
            provider["models"]["glm-5.3"]["limit"],
            serde_json::json!({ "context": 1_000_000, "output": 131_072 })
        );
        assert_eq!(document["model"], "df/glm-5.3");
        assert_eq!(document["enabled_providers"], serde_json::json!(["df"]));
        Ok(())
    }

    #[tokio::test]
    async fn the_run_uses_the_wire_api_of_the_model_not_a_harness_preference() -> anyhow::Result<()>
    {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        use_opencode(&pool).await?;
        sqlx::query(
            "INSERT INTO model_provider_apis (provider_id, wire_api, upstream_url)
             SELECT id, 'openai-responses', 'https://llm.example.test' FROM model_providers",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO model_provider_models (provider_id, model_id, wire_api)
             SELECT id, 'gpt-6-luna', 'openai-responses' FROM model_providers",
        )
        .execute(&pool)
        .await?;
        sqlx::query("UPDATE personas SET model = 'gpt-6-luna'")
            .execute(&pool)
            .await?;

        let (acp, _) = assign_of(&env).await?;

        let document: serde_json::Value =
            serde_json::from_str(&acp.env["OPENCODE_CONFIG_CONTENT"])?;
        assert_eq!(document["provider"]["df"]["npm"], "@ai-sdk/openai");
        assert!(document["provider"]["df"]["models"]["gpt-6-luna"]["limit"].is_null());
        Ok(())
    }

    #[tokio::test]
    async fn claude_code_gets_claude_files_and_the_configured_settings() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let mut env = Env::new(pool).await;
        let mut config = ChannelConfig::default();
        config.harness.claude_code.settings_json = r#"{"theme":"dark"}"#.into();
        env.ctx.config = std::sync::Arc::new(config);

        let (acp, files) = assign_of(&env).await?;

        assert_eq!(
            files,
            [
                (
                    ".claude/CLAUDE.md".to_string(),
                    "You are Linus.".to_string()
                ),
                (
                    ".claude/settings.json".to_string(),
                    r#"{"theme":"dark"}"#.to_string()
                )
            ]
        );
        assert!(!acp.env.contains_key("OPENCODE_CONFIG_CONTENT"));
        Ok(())
    }

    #[tokio::test]
    async fn claude_code_without_settings_gets_only_the_prompt_file() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool).await;

        let (_, files) = assign_of(&env).await?;

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, ".claude/CLAUDE.md");
        Ok(())
    }

    #[tokio::test]
    async fn a_model_the_harness_cannot_use_fails_the_assignment() -> anyhow::Result<()> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = Env::new(pool.clone()).await;
        use_opencode(&pool).await?;
        sqlx::query("UPDATE personas SET harness = 'claude-code'")
            .execute(&pool)
            .await?;

        assert!(assign_of(&env).await.is_err());
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
