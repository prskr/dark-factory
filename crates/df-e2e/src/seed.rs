//! The seed: credential reference, persona and projects of the end-to-end suite. Sandboxes reach
//! Forgejo only through the gateway's Git proxy, so there is no forge allowlist entry. The demo
//! profile for screenshots is [`demo`].
//!
//! Idempotent: a record is looked up by name or key and only written when it differs, so a second
//! run changes nothing (and writes no audit entries).

pub mod demo;

use anyhow::{Context, Result};
use df_core::model_provider::{CredentialScheme, ProviderApi, ProviderModel, WireApi};
use df_core::{CredentialRef, Principal};
use df_db::PgPool;
use df_db::catalog::{
    Actor, CredentialRefInput, PersonaInput, ProjectInput, get_project_by_key,
    list_credential_refs, list_personas, save_credential_ref, save_persona, save_project,
    update_project_with_push_policy,
};
use df_db::model_providers::{ModelProviderInput, list_model_providers, save_model_provider};
use df_db::push_checks::push_policy;
use uuid::Uuid;

use crate::data;

/// A project of the demo: the fake agent script it runs and the paths it hides.
struct SeedProject {
    key: &'static str,
    name: &'static str,
    script: &'static str,
    hidden: &'static [&'static str],
}

/// Writes the well-known data into the database behind `pool`.
pub async fn run(pool: &PgPool) -> Result<()> {
    let principal = Principal::system("e2e seed");
    let actor = Actor {
        principal: &principal,
        roles: &[],
    };

    let credential = ensure_credentials_and_persona(pool, actor).await?;
    for SeedProject {
        key,
        name,
        script,
        hidden,
    } in projects()
    {
        let wanted = project_input(
            key,
            name,
            &data::repo_url(),
            None,
            data::PROJECT_IMAGE,
            data::adapter_command(script),
            credential,
        );
        ensure_project(pool, actor, &wanted, hidden).await?;
    }
    Ok(())
}

/// A project of the suite or of the demo with the defaults the seed profiles share.
pub(crate) fn project_input(
    key: &str,
    name: &str,
    repo_url: &str,
    issue_tracker_url: Option<&str>,
    image: &str,
    adapter_command: Vec<String>,
    forge_credential: Uuid,
) -> ProjectInput {
    ProjectInput {
        key: key.into(),
        name: name.into(),
        repo_url: repo_url.into(),
        issue_tracker_url: issue_tracker_url.map(str::to_owned),
        image: image.into(),
        base_branch: data::BASE_BRANCH.into(),
        adapter_command,
        forge_credential_id: Some(forge_credential),
        webhook_wake: df_core::webhooks::WebhookWake::Members,
        branch_rewrite_policy: df_core::RewritePolicy::default(),
        agent_branch_globs: df_core::default_agent_branch_globs(),
    }
}

/// The projects of the end-to-end suite.
fn projects() -> [SeedProject; 13] {
    let project = |key, name, script| SeedProject {
        key,
        name,
        script,
        hidden: &[],
    };
    [
        project(data::PROJECT_KEY, data::PROJECT_NAME, data::DEMO_SCRIPT),
        project(
            data::RATE_LIMIT_PROJECT_KEY,
            data::RATE_LIMIT_PROJECT_NAME,
            data::RATE_LIMIT_SCRIPT,
        ),
        project(
            data::RATE_LIMIT_SHORT_PROJECT_KEY,
            data::RATE_LIMIT_SHORT_PROJECT_NAME,
            data::RATE_LIMIT_SHORT_SCRIPT,
        ),
        project(
            data::NETWORK_PROJECT_KEY,
            data::NETWORK_PROJECT_NAME,
            data::NETWORK_SCRIPT,
        ),
        project(
            data::SECRET_PROJECT_KEY,
            data::SECRET_PROJECT_NAME,
            data::SECRET_SCRIPT,
        ),
        project(
            data::PROTECTED_PROJECT_KEY,
            data::PROTECTED_PROJECT_NAME,
            data::PROTECTED_SCRIPT,
        ),
        SeedProject {
            hidden: &[data::HIDDEN_GLOB],
            ..project(
                data::HIDDEN_PROJECT_KEY,
                data::HIDDEN_PROJECT_NAME,
                data::HIDDEN_SCRIPT,
            )
        },
        project(
            data::WEBHOOK_PROJECT_KEY,
            data::WEBHOOK_PROJECT_NAME,
            data::WEBHOOK_SCRIPT,
        ),
        project(
            data::EXISTING_BRANCH_PROJECT_KEY,
            data::EXISTING_BRANCH_PROJECT_NAME,
            data::EXISTING_BRANCH_SCRIPT,
        ),
        project(
            data::DELEGATION_PROJECT_KEY,
            data::DELEGATION_PROJECT_NAME,
            data::DELEGATION_SCRIPT,
        ),
        project(
            data::RESEARCH_PROJECT_KEY,
            data::RESEARCH_PROJECT_NAME,
            data::RESEARCH_SCRIPT,
        ),
        project(
            data::INTERRUPT_PROJECT_KEY,
            data::INTERRUPT_PROJECT_NAME,
            data::INTERRUPT_SCRIPT,
        ),
        project(
            data::LANDLOCK_PROJECT_KEY,
            data::LANDLOCK_PROJECT_NAME,
            data::LANDLOCK_SCRIPT,
        ),
    ]
}

/// The forge and model credentials, the model providers and the persona `e2e`. Returns the id of
/// the forge credential.
async fn ensure_credentials_and_persona(pool: &PgPool, actor: Actor<'_>) -> Result<Uuid> {
    let credential = ensure_credential(
        pool,
        actor,
        &CredentialRefInput {
            name: data::CREDENTIAL_NAME.into(),
            secret_name: data::CREDENTIAL_SECRET.into(),
            secret_key: data::CREDENTIAL_KEY.into(),
        },
    )
    .await?
    .id
    .as_uuid();
    let model_credential = ensure_credential(
        pool,
        actor,
        &CredentialRefInput {
            name: data::MODEL_CREDENTIAL_NAME.into(),
            secret_name: data::MODEL_CREDENTIAL_SECRET.into(),
            secret_key: data::MODEL_CREDENTIAL_KEY.into(),
        },
    )
    .await?;
    let provider = ensure_model_provider(
        pool,
        actor,
        &anthropic_provider(
            &model_credential,
            data::MODEL_PROVIDER_NAME,
            &data::fake_upstream_url(),
        ),
    )
    .await?;
    ensure_model_provider(
        pool,
        actor,
        &anthropic_provider(
            &model_credential,
            data::SETTINGS_PROVIDER_NAME,
            data::SETTINGS_PROVIDER_URL,
        ),
    )
    .await?;
    ensure_persona(
        pool,
        actor,
        &PersonaInput {
            name: data::PERSONA_NAME.into(),
            description: String::new(),
            system_prompt: "You are the e2e persona.".into(),
            harness: df_core::config::CLAUDE_CODE_HARNESS.into(),
            model: data::PERSONA_MODEL.into(),
            model_provider_id: provider,
        },
    )
    .await?;
    Ok(credential)
}

/// The credential reference `wanted`, written when it is missing or differs.
pub(crate) async fn ensure_credential(
    pool: &PgPool,
    actor: Actor<'_>,
    wanted: &CredentialRefInput,
) -> Result<CredentialRef> {
    let existing = list_credential_refs(pool)
        .await?
        .into_iter()
        .find(|c| c.name == wanted.name);
    match existing {
        Some(c) if c.secret.name == wanted.secret_name && c.secret.key == wanted.secret_key => {
            Ok(c)
        }
        other => save_credential_ref(pool, actor, other.map(|c| c.id.as_uuid()), wanted)
            .await
            .context("save credential reference"),
    }
}

/// An Anthropic Messages provider `name` with the upstream `upstream_url` and the persona model.
fn anthropic_provider(
    credential: &CredentialRef,
    name: &str,
    upstream_url: &str,
) -> ModelProviderInput {
    ModelProviderInput {
        name: name.into(),
        credential_ref_id: credential.id.as_uuid(),
        scheme: CredentialScheme::XApiKey,
        credential_header: None,
        credential_prefix: None,
        headers: Vec::new(),
        apis: vec![ProviderApi {
            wire_api: WireApi::AnthropicMessages,
            upstream_url: upstream_url.into(),
        }],
        models: vec![ProviderModel {
            model_id: data::PERSONA_MODEL.into(),
            wire_api: WireApi::AnthropicMessages,
            limits: None,
        }],
    }
}

/// The model provider `wanted`, written when it is missing or differs.
pub(crate) async fn ensure_model_provider(
    pool: &PgPool,
    actor: Actor<'_>,
    wanted: &ModelProviderInput,
) -> Result<Uuid> {
    let existing = list_model_providers(pool)
        .await?
        .into_iter()
        .find(|p| p.input.name == wanted.name);
    if let Some(p) = &existing
        && p.input == *wanted
    {
        return Ok(p.id);
    }
    let saved = save_model_provider(pool, actor, existing.map(|p| p.id), wanted)
        .await
        .context("save model provider")?;
    Ok(saved.id)
}

/// The persona `wanted`, written when it is missing or differs.
pub(crate) async fn ensure_persona(
    pool: &PgPool,
    actor: Actor<'_>,
    wanted: &PersonaInput,
) -> Result<Uuid> {
    let existing = list_personas(pool)
        .await?
        .into_iter()
        .find(|p| p.name == wanted.name);
    if let Some(p) = &existing
        && p.system_prompt == wanted.system_prompt
        && p.description == wanted.description
        && p.harness == wanted.harness
        && p.model == wanted.model
        && p.model_provider_id == wanted.model_provider_id
    {
        return Ok(p.id);
    }
    let id = existing.map(|p| p.id);
    let saved = save_persona(pool, actor, id, wanted)
        .await
        .context("save persona")?;
    Ok(saved.id)
}

/// The project `wanted` with the hidden globs `hidden`, written when it is missing or differs.
/// Returns its id.
pub(crate) async fn ensure_project(
    pool: &PgPool,
    actor: Actor<'_>,
    wanted: &ProjectInput,
    hidden: &[&str],
) -> Result<Uuid> {
    let key = wanted.key.as_str();
    let existing = get_project_by_key(pool, key).await?;
    let id = match &existing {
        Some(p)
            if p.name == wanted.name
                && p.repo_url == wanted.repo_url
                && p.issue_tracker_url == wanted.issue_tracker_url
                && p.image == wanted.image
                && p.base_branch == wanted.base_branch
                && p.adapter_command == wanted.adapter_command
                && p.forge_credential_id == wanted.forge_credential_id
                && p.webhook_wake == wanted.webhook_wake =>
        {
            p.id
        }
        other => {
            save_project(pool, actor, other.as_ref().map(|p| p.id), wanted)
                .await
                .with_context(|| format!("save project {key}"))?
                .id
        }
    };
    let mut policy = push_policy(pool, id).await?;
    if policy.hidden_globs != hidden {
        policy.hidden_globs = hidden.iter().map(|glob| (*glob).to_owned()).collect();
        update_project_with_push_policy(pool, actor, id, wanted, &policy)
            .await
            .with_context(|| format!("save push policy of project {key}"))?;
    }
    Ok(id)
}
