//! The demo profile of the seed (`seed --demo`, `just seed-demo`): projects, personas, tasks in
//! every state with runs, transcripts, token usage and artifacts, for screenshots and demos of
//! `just serve-local`. It writes its own credential references and model providers and uses
//! project keys and persona names that the end-to-end fixtures don't, so both profiles share a
//! database.
//!
//! Idempotent: projects and personas are compared field by field like the e2e seed does. A task
//! is identified by project key and title; an existing task is left as it is with its runs,
//! transcript and artifacts. The one update is the resume time of a `queued` or `rate_limited`
//! task that has passed and that no run has started since: it moves forward again, so a long-lived
//! database keeps showing the state. A task is written in one transaction.
//!
//! The control plane of `just serve-local` runs without a cluster (`serve --no-cluster`). Its
//! scheduler starts `queued` and `rate_limited` tasks that are due, and its startup sweep ends
//! runs that are not finished, so the profile writes no task with an unfinished run: a `running`
//! task is shown as `waiting`, and the `queued` and `rate_limited` tasks have a resume time in the
//! future.

mod data;
mod tasks;
mod write;

use std::collections::HashMap;

use anyhow::{Context, Result};
use df_core::Principal;
use df_core::model_provider::{CredentialScheme, ProviderApi, ProviderModel, WireApi};
use df_db::PgPool;
use df_db::catalog::{Actor, CredentialRefInput, PersonaInput};
use df_db::model_providers::ModelProviderInput;
use uuid::Uuid;

use super::{
    ensure_credential, ensure_model_provider, ensure_persona, ensure_project, project_input,
};
use data::{PERSONAS, PROJECTS};

/// What [`run`] found that the profile cannot fix.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Hints for the person who ran the seed.
    pub hints: Vec<String>,
}

/// Ids by name the task writer needs.
struct Ids {
    projects: HashMap<&'static str, Uuid>,
    personas: HashMap<&'static str, Uuid>,
    /// Tasks written or found so far, by `(project key, title)`.
    tasks: HashMap<(&'static str, &'static str), write::Written>,
}

/// Writes the demo data into the database behind `pool`.
pub async fn run(pool: &PgPool) -> Result<Report> {
    let principal = Principal::system("demo seed");
    let actor = Actor {
        principal: &principal,
        roles: &[],
    };
    let mut ids = supporting_records(pool, actor).await?;
    let mut report = Report::default();
    for task in tasks::all() {
        write::ensure_task(pool, &mut ids, &task, &mut report).await?;
    }
    Ok(report)
}

/// Credential references, model providers, personas and projects.
async fn supporting_records(pool: &PgPool, actor: Actor<'_>) -> Result<Ids> {
    let credential = |name: &str, secret: &str, key: &str| CredentialRefInput {
        name: name.into(),
        secret_name: secret.into(),
        secret_key: key.into(),
    };
    let forge = ensure_credential(
        pool,
        actor,
        &credential("demo-forge", "demo-forge-token", "token"),
    )
    .await?;
    let anthropic_key = ensure_credential(
        pool,
        actor,
        &credential("demo-anthropic", "demo-anthropic-key", "key"),
    )
    .await?;
    let openai_key = ensure_credential(
        pool,
        actor,
        &credential("demo-openai", "demo-openai-key", "key"),
    )
    .await?;
    let providers = HashMap::from([
        (
            data::ANTHROPIC,
            ensure_model_provider(
                pool,
                actor,
                &provider(
                    data::ANTHROPIC,
                    anthropic_key.id.as_uuid(),
                    CredentialScheme::XApiKey,
                    WireApi::AnthropicMessages,
                    "https://api.anthropic.com",
                    &[data::SONNET, data::OPUS],
                ),
            )
            .await?,
        ),
        (
            data::OPENAI,
            ensure_model_provider(
                pool,
                actor,
                &provider(
                    data::OPENAI,
                    openai_key.id.as_uuid(),
                    CredentialScheme::Bearer,
                    WireApi::OpenAiChat,
                    "https://api.openai.com",
                    &[data::GPT],
                ),
            )
            .await?,
        ),
    ]);

    let personas = ensure_personas(pool, actor, &providers).await?;
    let projects = ensure_projects(pool, actor, forge.id.as_uuid()).await?;
    Ok(Ids {
        projects,
        personas,
        tasks: HashMap::new(),
    })
}

async fn ensure_personas(
    pool: &PgPool,
    actor: Actor<'_>,
    providers: &HashMap<&str, Uuid>,
) -> Result<HashMap<&'static str, Uuid>> {
    let mut personas = HashMap::new();
    for persona in PERSONAS {
        let wanted = PersonaInput {
            name: persona.name.into(),
            description: persona.description.into(),
            system_prompt: persona.system_prompt.into(),
            harness: persona.harness.into(),
            model: persona.model.into(),
            model_provider_id: *providers
                .get(persona.provider)
                .context("provider of the persona")?,
        };
        personas.insert(
            persona.name,
            ensure_persona(pool, actor, &wanted)
                .await
                .with_context(|| format!("persona {}", persona.name))?,
        );
    }
    Ok(personas)
}

async fn ensure_projects(
    pool: &PgPool,
    actor: Actor<'_>,
    forge: Uuid,
) -> Result<HashMap<&'static str, Uuid>> {
    let mut projects = HashMap::new();
    for project in PROJECTS {
        let wanted = project_input(
            project.key,
            project.name,
            project.repo_url,
            project.issue_tracker_url,
            project.image,
            Vec::new(),
            forge,
        );
        let id = ensure_project(pool, actor, &wanted, project.hidden).await?;
        projects.insert(project.key, id);
    }
    Ok(projects)
}

/// A provider with one wire API that serves `models`.
fn provider(
    name: &str,
    credential: Uuid,
    scheme: CredentialScheme,
    wire_api: WireApi,
    upstream_url: &str,
    models: &[&str],
) -> ModelProviderInput {
    let mut models: Vec<ProviderModel> = models
        .iter()
        .map(|model| ProviderModel {
            model_id: (*model).into(),
            wire_api,
            limits: None,
        })
        .collect();
    models.sort_by(|a, b| a.model_id.cmp(&b.model_id));
    ModelProviderInput {
        name: name.into(),
        credential_ref_id: credential,
        scheme,
        credential_header: None,
        credential_prefix: None,
        headers: Vec::new(),
        apis: vec![ProviderApi {
            wire_api,
            upstream_url: upstream_url.into(),
        }],
        models,
    }
}
