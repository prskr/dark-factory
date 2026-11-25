//! A run with its task, project and persona, loaded from the database. The scheduler provisions
//! the sandbox from it; the agent port builds `Assign` from it.

use df_core::{PersonaId, ProjectId, RewritePolicy};
use df_db::TaskRow;
use df_db::catalog;
use df_db::get_task;
use df_db::runs::RunRow;
use sqlx::PgPool;

use crate::error::OrchestratorError;

/// Project columns a run needs.
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub id: ProjectId,
    pub key: String,
    pub repo_url: String,
    pub image: String,
    pub base_branch: String,
    /// Command that starts the adapter in the project image; empty means the harness image.
    pub adapter_command: Vec<String>,
    pub rewrite_policy: RewritePolicy,
    /// Globs of the existing branches an agent may start tasks on.
    pub agent_branch_globs: Vec<String>,
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

/// Persona columns a run needs.
#[derive(Debug, Clone)]
pub struct PersonaInfo {
    pub id: PersonaId,
    pub display_name: String,
    pub system_prompt: String,
    pub harness: String,
    pub model: String,
    pub model_provider_id: uuid::Uuid,
    /// The public signing key (OpenSSH format) when the context was loaded.
    pub signing_public_key: Option<String>,
}

/// A run with its task, project and persona, loaded once per stream.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub run: RunRow,
    pub task: TaskRow,
    pub project: ProjectInfo,
    pub persona: PersonaInfo,
    /// The task's effective base branch when the context was loaded (`df_db::base_branch`).
    pub base_branch: String,
}

impl RunContext {
    /// The task's stored branch.
    pub fn task_branch(&self) -> &str {
        &self.task.branch
    }

    /// The task's effective base branch: its base task's branch, the base of its attached PR or
    /// the project's.
    pub fn base_branch(&self) -> &str {
        &self.base_branch
    }

    /// The WIP snapshot branch; `None` for a chat, which has none.
    pub fn wip_branch(&self) -> Option<String> {
        self.task.key.wip_branch()
    }

    /// Whether the run is a chat's: a read-only checkout of the base branch.
    pub fn is_chat(&self) -> bool {
        self.task.key.is_chat()
    }

    /// What the run's task is.
    pub fn kind(&self) -> df_core::TaskKind {
        if self.is_chat() {
            df_core::TaskKind::Chat
        } else {
            df_core::TaskKind::Task
        }
    }
}

/// Loads task, project and persona of `run`.
pub async fn load_run_context(pool: &PgPool, run: RunRow) -> Result<RunContext, OrchestratorError> {
    let task = get_task(pool, run.task_id).await?.ok_or_else(|| {
        OrchestratorError::InvalidData(format!("task of run {} is missing", run.id))
    })?;

    let project = catalog::get_project(pool, task.project_id.as_uuid())
        .await?
        .ok_or_else(|| {
            OrchestratorError::InvalidData(format!("project of task {} is missing", task.id))
        })?;
    let project = ProjectInfo {
        id: task.project_id,
        key: project.key,
        repo_url: project.repo_url,
        image: project.image,
        base_branch: project.base_branch,
        adapter_command: project.adapter_command,
        rewrite_policy: project.branch_rewrite_policy,
        agent_branch_globs: project.agent_branch_globs,
    };

    let persona = catalog::get_persona(pool, task.persona_id.as_uuid())
        .await?
        .ok_or_else(|| {
            OrchestratorError::InvalidData(format!("persona of task {} is missing", task.id))
        })?;
    let persona = PersonaInfo {
        id: task.persona_id,
        display_name: persona.display_name,
        system_prompt: persona.system_prompt,
        harness: persona.harness,
        model: persona.model,
        model_provider_id: persona.model_provider_id,
        signing_public_key: persona.signing_key.map(|key| key.public_key),
    };

    let base_branch = df_db::base_branch(pool, task.id).await?;

    Ok(RunContext {
        run,
        task,
        project,
        persona,
        base_branch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
            rewrite_policy: RewritePolicy::WithLease,
            agent_branch_globs: Vec::new(),
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
}
