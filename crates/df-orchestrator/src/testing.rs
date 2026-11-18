//! Test fixtures: database seeding, the fakes and a context wired to them. Enabled by the
//! `test-util` feature.

#![expect(
    clippy::disallowed_methods,
    reason = "test fixtures seed the database directly (AGENTS.md, \"Database access\")"
)]

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use df_core::testing::{FakeForgeClient, FakeSandboxProvider, FakeSecretStore};
use df_core::{InstanceSubscription, LifecycleEventsConfig, PersonaId, ProjectId, RunId, TaskId};
use df_db::lifecycle_events::EventSinks;
use df_db::{NewTask, Notifier, TaskRow, create_task};
use sqlx::PgPool;

use crate::{OrchestratorConfig, OrchestratorContext};

#[cfg(test)]
pub(crate) mod gated;

/// Status, resume time and resume count of a task, as stored.
#[derive(Debug, sqlx::FromRow)]
pub struct TaskResumeState {
    pub status: String,
    pub resume_at: Option<chrono::DateTime<chrono::Utc>>,
    pub auto_resumes: i32,
}

/// Reads the resume state of a task.
pub async fn task_resume_state(pool: &PgPool, task_id: TaskId) -> TaskResumeState {
    sqlx::query_as("SELECT status, resume_at, auto_resumes FROM tasks WHERE id = $1")
        .bind(task_id.as_uuid())
        .fetch_one(pool)
        .await
        .expect("read the resume state of a task")
}

/// The recorded lifecycle events (their envelopes) in the order they occurred.
pub async fn recorded_events(pool: &PgPool) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT payload FROM lifecycle_events ORDER BY occurred_at, id")
        .fetch_all(pool)
        .await
        .expect("read lifecycle events")
}

/// Seeded database and the fakes behind an [`OrchestratorContext`]. It holds no context itself,
/// so there is one copy of the context per test environment: the owner of the fixture
/// ([`Env`], or the agent channel's `Env`).
pub struct Fixture {
    pool: PgPool,
    pub provider: FakeSandboxProvider,
    pub forge: FakeForgeClient,
    pub secrets: FakeSecretStore,
    pub project_id: ProjectId,
    pub persona_id: PersonaId,
    pub task: TaskRow,
}

impl Fixture {
    /// Seeds project `DF`, persona "Linus" with the model provider "Claude subscription"
    /// (Secret `claude/token`), and task `DF-1`; returns the fixture and a context wired to the
    /// fakes.
    pub async fn seed(pool: PgPool) -> (Self, OrchestratorContext) {
        let provider = FakeSandboxProvider::new();
        let forge = FakeForgeClient::new();
        let secrets = FakeSecretStore::new();
        secrets.insert("claude", "token", "claude-secret-value");
        secrets.insert("forge", "token", "forge-secret-value");

        let (claude_ref, forge_ref) = (uuid::Uuid::now_v7(), uuid::Uuid::now_v7());
        for (id, name, secret) in [
            (claude_ref, "claude", "claude"),
            (forge_ref, "forge", "forge"),
        ] {
            sqlx::query(
                "INSERT INTO credential_refs (id, name, secret_name, secret_key) VALUES ($1, $2, $3, 'token')",
            )
            .bind(id)
            .bind(name)
            .bind(secret)
            .execute(&pool)
            .await
            .expect("seed credential_refs");
        }
        let project_id = ProjectId::new();
        sqlx::query(
            "INSERT INTO projects (id, key, name, repo_url, image, base_branch, forge_credential_id)
             VALUES ($1, 'DF', 'Dark Factory', 'https://git.example.org/peter/dark-factory.git',
                     'registry.example.org/df/project:latest', 'main', $2)",
        )
        .bind(project_id.as_uuid())
        .bind(forge_ref)
        .execute(&pool)
        .await
        .expect("seed project");
        let model_provider = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO model_providers (id, name, credential_ref_id, credential_scheme)
             VALUES ($1, 'Claude subscription', $2, 'claude-subscription')",
        )
        .bind(model_provider)
        .bind(claude_ref)
        .execute(&pool)
        .await
        .expect("seed model provider");
        sqlx::query(
            "WITH api AS (
                 INSERT INTO model_provider_apis (provider_id, wire_api, upstream_url)
                 VALUES ($1, 'anthropic-messages', 'https://api.anthropic.com')
                 RETURNING provider_id)
             INSERT INTO model_provider_models (provider_id, model_id, wire_api)
             SELECT provider_id, 'claude-sonnet-5-5', 'anthropic-messages' FROM api",
        )
        .bind(model_provider)
        .execute(&pool)
        .await
        .expect("seed model provider API and model");
        let persona_id = PersonaId::new();
        sqlx::query(
            "INSERT INTO personas (id, name, system_prompt, model, model_provider_id)
             VALUES ($1, 'Linus', 'You are Linus.', 'claude-sonnet-5-5', $2)",
        )
        .bind(persona_id.as_uuid())
        .bind(model_provider)
        .execute(&pool)
        .await
        .expect("seed persona");

        let (task, _) = create_task(
            &pool,
            NewTask {
                project_id,
                persona_id,
                title: Some("Fix login".into()),
                description: "Validate the OIDC state.".into(),
                issue_number: Some(42),
                created_by: df_core::Principal::system("test"),
                parent_id: None,
                depends_on: Vec::new(),
            },
        )
        .await
        .expect("seed task");

        let notifier = Arc::new(Notifier::start(&pool).await.expect("start notifier"));
        let ctx = OrchestratorContext {
            pool: pool.clone(),
            notifier,
            provider: Arc::new(provider.clone()),
            forge: Arc::new(forge.clone()),
            secrets: Arc::new(secrets.clone()),
            config: Arc::new(OrchestratorConfig::default()),
        };
        let fixture = Self {
            pool,
            provider,
            forge,
            secrets,
            project_id,
            persona_id,
            task,
        };
        (fixture, ctx)
    }

    /// Creates task `DF-2` in the seeded project, for the persona.
    pub async fn second_task(&self) -> TaskId {
        let (task, _) = create_task(
            &self.pool,
            NewTask {
                project_id: self.project_id,
                persona_id: self.persona_id,
                title: Some("Second".into()),
                description: String::new(),
                issue_number: None,
                created_by: df_core::Principal::system("test"),
                parent_id: None,
                depends_on: Vec::new(),
            },
        )
        .await
        .expect("create a second task");
        task.id
    }

    /// Creates a task of the seeded project on the existing `branch`, attached to `pr`.
    pub async fn task_on_branch(&self, branch: &str, pr: Option<df_db::AttachedPr>) -> TaskRow {
        let (task, _) = df_db::create_task_on(
            &self.pool,
            NewTask {
                project_id: self.project_id,
                persona_id: self.persona_id,
                title: Some("Existing branch".into()),
                description: String::new(),
                issue_number: None,
                created_by: df_core::Principal::system("test"),
                parent_id: None,
                depends_on: Vec::new(),
            },
            df_db::TaskPlacement {
                branch: Some(
                    df_core::ExistingBranch::parse(branch, "main").expect("a valid test branch"),
                ),
                attached_pr: pr,
            },
        )
        .await
        .expect("create a task on an existing branch");
        task
    }

    /// Creates a run for the task in `state` with pod IP `127.0.0.1`.
    pub async fn insert_run(&self, task_id: TaskId, number: i64, state: &str) -> RunId {
        let run_id = RunId::new();
        sqlx::query(
            "INSERT INTO runs (id, task_id, number, state, pod_ip) VALUES ($1, $2, $3, $4, '127.0.0.1')",
        )
        .bind(run_id.as_uuid())
        .bind(task_id.as_uuid())
        .bind(number)
        .bind(state)
        .execute(&self.pool)
        .await
        .expect("insert run");
        run_id
    }

    /// A board member as the actor of UI actions.
    pub fn human(&self) -> df_core::Principal {
        df_core::Principal::human(df_core::UserId::new(), "Ada")
    }
}

/// Seeded database, fakes and a context wired to them; the fixture is reachable through `Deref`.
pub struct Env {
    pub ctx: OrchestratorContext,
    fixture: Fixture,
}

impl Env {
    /// Seeds like [`Fixture::seed`].
    pub async fn new(pool: PgPool) -> Self {
        let (fixture, ctx) = Fixture::seed(pool).await;
        Self { ctx, fixture }
    }

    /// Turns lifecycle events on, see [`with_events`].
    #[must_use]
    pub fn with_events(mut self) -> Self {
        with_events(&mut self.ctx);
        self
    }

    /// A second context on the same database, see [`other_replica`].
    pub async fn other_replica(&self) -> OrchestratorContext {
        other_replica(&self.ctx).await
    }
}

impl Deref for Env {
    type Target = Fixture;

    fn deref(&self) -> &Fixture {
        &self.fixture
    }
}

impl DerefMut for Env {
    fn deref_mut(&mut self) -> &mut Fixture {
        &mut self.fixture
    }
}

/// Turns lifecycle events on with one instance subscription to every event type.
pub fn with_events(ctx: &mut OrchestratorContext) {
    let mut config = OrchestratorConfig::clone(&ctx.config);
    config.events = EventSinks::new(&LifecycleEventsConfig {
        signing_key_file: Some("signing.key".into()),
        subscriptions: vec![InstanceSubscription {
            url: "https://events.example.org/hook".into(),
            events: Vec::new(),
        }],
    });
    ctx.config = Arc::new(config);
}

/// A second context on the same database, as another replica would have it.
pub async fn other_replica(ctx: &OrchestratorContext) -> OrchestratorContext {
    OrchestratorContext {
        notifier: Arc::new(Notifier::start(&ctx.pool).await.expect("start notifier")),
        ..ctx.clone()
    }
}
