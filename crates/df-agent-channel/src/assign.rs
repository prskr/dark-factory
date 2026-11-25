//! Everything derived from database rows for a run that goes on the wire: `Assign` and the
//! prompts to deliver (design D1, D5, D8). The loaded run context is `df_orchestrator::run_context`.

use std::sync::LazyLock;

use df_core::harness::{self, ConfigInput, HarnessProfile, INFERENCE_PATH, PLACEHOLDER_KEY};
use df_core::model_provider::ProviderModel;
use df_core::{Principal, RewritePolicy, TaskId};
use df_db::model_providers::ModelProviderRecord;
use df_db::push_checks::push_policy;
use df_db::runs::previous_harness_session_id;
use df_db::{TaskMessageRow, claim_held_back_messages, list_undelivered_messages};
use df_orchestrator::OrchestratorError;
use df_orchestrator::run_context::RunContext;
use df_proto::agent::v1::{
    AcpHarness, Assign, Budgets, CommitIdentity, Continuation, Harness, HarnessFile, Prompt,
    RewritePolicy as RewritePolicyProto, TaskRef, ToolAnnotations, ToolDescriptor, Workspace,
    harness as harness_kind,
};
use df_proto::pbjson_types::Duration;
use df_proto::time::to_proto;
use df_proto::transcript::v1 as transcript;
use df_sandbox_contract::LandlockMode;
use df_sandbox_contract::paths::{INSTANCE_CA_PATH, SANDBOX_HOME_DIR, SANDBOX_REPO_DIR};
use serde::Deserialize;
use sqlx::PgPool;

use crate::error::ChannelError;
use crate::{ChannelContext, GitProxy};

const FACTORY_TOOLS_JSON: &str = include_str!("../../../contract/mcp/factory-tools.v2.json");

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

/// What every chat run is told after the persona's system prompt.
pub const CHAT_INSTRUCTIONS: &str = "\
## Chat

You are in a chat with a board member, who reads your replies as they come. \
The checkout of the repository is read-only and reset to the base branch at the start of every run: \
anything you change in it is gone with the run, and pushes are rejected. \
Work that needs a branch or a pull request goes into tasks: create them with `create_task` for a persona \
(its handle, from `list_personas`), and follow up with `send_message`. \
Results that should outlive the chat belong in artifacts or tasks. \
Set a short topic for the chat with `set_task_title` in your first reply. \
A chat is never completed: there is no `complete_task`, you just answer and wait for the next message.";

/// The tools a chat run is offered. A call to any other tool is rejected.
pub const CHAT_TOOLS: &[&str] = &[
    "set_task_title",
    "report_progress",
    "create_task",
    "list_personas",
    "send_message",
    "request_approval",
    "write_artifact",
    "list_artifacts",
    "read_artifact",
    "list_issues",
    "create_issue",
];

/// Tools offered and accepted only in a chat. Removed, with the two checks that read it
/// (`tool_descriptors_for` and `tools::try_execute`), when per-persona tool permissions land.
pub const CHAT_ONLY_TOOLS: &[&str] = &["list_issues", "create_issue"];

/// The factory tools v2 as sent in `Assign.tools`.
pub fn tool_descriptors() -> Vec<ToolDescriptor> {
    tool_descriptors_where(|_| true)
}

/// The tools of a run: all but [`CHAT_ONLY_TOOLS`], or only [`CHAT_TOOLS`] for a chat.
fn tool_descriptors_for(kind: df_core::TaskKind) -> Vec<ToolDescriptor> {
    tool_descriptors_where(|name| match kind {
        df_core::TaskKind::Task => !CHAT_ONLY_TOOLS.contains(&name),
        df_core::TaskKind::Chat => CHAT_TOOLS.contains(&name),
    })
}

fn tool_descriptors_where(offered: impl Fn(&str) -> bool) -> Vec<ToolDescriptor> {
    TOOL_ENTRIES
        .tools
        .iter()
        .filter(|t| offered(&t.name))
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
        .ok_or_else(|| OrchestratorError::InvalidData(format!("unknown harness {harness_name}")))?;
    let (provider, model) = persona_model(ctx, rc).await?;
    harness::check_compatible(harness_name, model.wire_api, provider.input.scheme).map_err(
        |error| {
            OrchestratorError::InvalidData(format!("persona {}: {error}", rc.persona.display_name))
        },
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
            base_branch: rc.base_branch().to_owned(),
            // Empty for a chat, which has no branch: its checkout is read-only.
            task_branch: rc.task_branch().to_owned(),
            wip_branch: rc.wip_branch().unwrap_or_default(),
            read_only: rc.is_chat(),
            commit_identity: Some(CommitIdentity {
                name: rc.run.commit_identity.name.clone(),
                email: rc.run.commit_identity.email.clone(),
            }),
            git_hooks: Vec::new(),
            git_config: Vec::new(),
            hidden_paths: hidden_paths.clone(),
            rewrite_policy: match rc.project.rewrite_policy {
                RewritePolicy::Never => RewritePolicyProto::Never,
                RewritePolicy::WithLease => RewritePolicyProto::WithLease,
            }
            .into(),
            existing_branch: rc.task.on_existing_branch(),
            signing_public_key: rc.persona.signing_public_key.clone().unwrap_or_default(),
        }),
        harness: Some(Harness {
            kind: Some(harness_kind::Kind::Acp(AcpHarness {
                command,
                env,
                model: Some(rc.persona.model.clone()),
                files: harness_files(ctx, rc, profile, &hidden_paths),
            })),
        }),
        tools: tool_descriptors_for(rc.kind()),
        budgets: Some(Budgets {
            max_wall_clock: Some(Duration {
                seconds: i64::try_from(ctx.orch.config.runs.max_wall_clock_seconds)
                    .unwrap_or(i64::MAX),
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
    let persona_prompt = if rc.is_chat() {
        format!("{}\n\n{CHAT_INSTRUCTIONS}", rc.persona.system_prompt)
    } else {
        rc.persona.system_prompt.clone()
    };
    let prompt = system_prompt(&persona_prompt, hidden_paths, ctx.config.landlock);
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
                OrchestratorError::InvalidData(format!(
                    "model provider of persona {} is missing",
                    rc.persona.display_name
                ))
            })?;
    let model = provider.model(&rc.persona.model).cloned().ok_or_else(|| {
        OrchestratorError::InvalidData(format!(
            "model {} of persona {} is not a model of provider {}",
            rc.persona.model, rc.persona.display_name, provider.input.name
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
    let previous = previous_harness_session_id(pool, rc.task.id, rc.run.number).await?;
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
        queued_at: Some(to_proto(row.queued_at)),
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
