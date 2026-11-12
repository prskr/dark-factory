//! Everything derived from database rows for a run: the loaded run context, `Assign` and the
//! prompts to deliver (design D1, D5, D8).

use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use df_core::harness::{
    self, ConfigInput, HarnessProfile, INFERENCE_PATH, INSTANCE_CA_PATH, PLACEHOLDER_KEY,
    SANDBOX_HOME_DIR, SANDBOX_REPO_DIR,
};
use df_core::model_provider::ProviderModel;
use df_core::{LandlockMode, PersonaId, Principal, ProjectId, TaskId};
use df_db::model_providers::ModelProviderRecord;
use df_db::push_checks::push_policy;
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
    let hidden_paths = push_policy(&ctx.pool, rc.project.id.as_uuid())
        .await?
        .hidden_globs;
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
            hidden_paths: hidden_paths.clone(),
        }),
        harness: Some(Harness {
            kind: Some(harness_kind::Kind::Acp(AcpHarness {
                command,
                env,
                model: Some(rc.persona.model.clone()),
                files: harness_files(ctx, rc, profile, &hidden_paths),
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
    hidden_paths: &[String],
) -> Vec<HarnessFile> {
    let mut files = Vec::new();
    let prompt = system_prompt(&rc.persona.system_prompt, hidden_paths, ctx.config.landlock);
    if !prompt.is_empty() {
        files.push(HarnessFile {
            path: profile.system_prompt_path.to_string(),
            content: prompt,
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

/// The persona's `prompt`, followed by the sandbox section: the hidden paths (hidden-paths design
/// D8) and, with Landlock enforced, the writable directories (Landlock design D6). The sandbox
/// pods always have the default layout, so the paths are fixed.
fn system_prompt(prompt: &str, hidden_paths: &[String], landlock: LandlockMode) -> String {
    let mut section = Vec::new();
    if !hidden_paths.is_empty() {
        section.push(format!(
            "These repository paths are hidden from your workspace on purpose: {}. Do not \
             restore, check out or read them through Git (`git show`, `git checkout`, \
             `git sparse-checkout`); pushes changing them are rejected.",
            hidden_paths.join(", ")
        ));
    }
    if landlock == LandlockMode::Enforce {
        section.push(format!(
            "Writable: {SANDBOX_REPO_DIR} (workspace), {SANDBOX_HOME_DIR} (your home), /tmp. \
             Everything else is read-only or inaccessible on purpose; don't try to change \
             permissions."
        ));
    }
    if section.is_empty() {
        return prompt.to_owned();
    }
    let separator = if prompt.is_empty() { "" } else { "\n\n" };
    format!(
        "{prompt}{separator}## Sandbox\n\n{}\n",
        section.join("\n\n")
    )
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
        author_name: row.author.display_name().to_string(),
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
mod tests;
