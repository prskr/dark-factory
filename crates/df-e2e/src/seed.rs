//! The seed: credential reference, persona and projects of the demo. Sandboxes reach Forgejo
//! only through the gateway's Git proxy, so there is no forge allowlist entry.
//!
//! Idempotent: a record is looked up by name or key and only written when it differs, so a second
//! run changes nothing (and writes no audit entries).

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

    let credential = ensure_credential(pool, actor).await?;
    let model_credential = ensure_model_credential(pool, actor).await?;
    let provider = ensure_model_provider(
        pool,
        actor,
        &model_credential,
        data::MODEL_PROVIDER_NAME,
        &data::fake_upstream_url(),
    )
    .await?;
    ensure_model_provider(
        pool,
        actor,
        &model_credential,
        data::SETTINGS_PROVIDER_NAME,
        data::SETTINGS_PROVIDER_URL,
    )
    .await?;
    ensure_persona(pool, actor, provider).await?;
    let project = |key, name, script| SeedProject {
        key,
        name,
        script,
        hidden: &[],
    };
    let projects = [
        project(data::PROJECT_KEY, data::PROJECT_NAME, data::DEMO_SCRIPT),
        project(
            data::RATE_LIMIT_PROJECT_KEY,
            data::RATE_LIMIT_PROJECT_NAME,
            data::RATE_LIMIT_SCRIPT,
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
    ];
    for seed in projects {
        ensure_project(pool, actor, &seed, credential).await?;
    }
    Ok(())
}

async fn ensure_credential(pool: &PgPool, actor: Actor<'_>) -> Result<Uuid> {
    let wanted = CredentialRefInput {
        name: data::CREDENTIAL_NAME.into(),
        secret_name: data::CREDENTIAL_SECRET.into(),
        secret_key: data::CREDENTIAL_KEY.into(),
    };
    let existing = list_credential_refs(pool)
        .await?
        .into_iter()
        .find(|c| c.name == wanted.name);
    if let Some(c) = &existing
        && c.secret.name == wanted.secret_name
        && c.secret.key == wanted.secret_key
    {
        return Ok(c.id.as_uuid());
    }
    let id = existing.map(|c| c.id.as_uuid());
    let saved = save_credential_ref(pool, actor, id, &wanted)
        .await
        .context("save credential reference")?;
    Ok(saved.id.as_uuid())
}

/// The credential reference of the model providers.
async fn ensure_model_credential(pool: &PgPool, actor: Actor<'_>) -> Result<CredentialRef> {
    let wanted_credential = CredentialRefInput {
        name: data::MODEL_CREDENTIAL_NAME.into(),
        secret_name: data::MODEL_CREDENTIAL_SECRET.into(),
        secret_key: data::MODEL_CREDENTIAL_KEY.into(),
    };
    let existing = list_credential_refs(pool)
        .await?
        .into_iter()
        .find(|c| c.name == wanted_credential.name);
    let credential = match existing {
        Some(c)
            if c.secret.name == wanted_credential.secret_name
                && c.secret.key == wanted_credential.secret_key =>
        {
            c
        }
        other => save_credential_ref(
            pool,
            actor,
            other.map(|c| c.id.as_uuid()),
            &wanted_credential,
        )
        .await
        .context("save model credential reference")?,
    };
    Ok(credential)
}

/// The Anthropic Messages provider `name` with the upstream `upstream_url` and the persona model.
async fn ensure_model_provider(
    pool: &PgPool,
    actor: Actor<'_>,
    credential: &CredentialRef,
    name: &str,
    upstream_url: &str,
) -> Result<Uuid> {
    let wanted = ModelProviderInput {
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
    };
    let existing = list_model_providers(pool)
        .await?
        .into_iter()
        .find(|p| p.input.name == wanted.name);
    if let Some(p) = &existing
        && p.input == wanted
    {
        return Ok(p.id);
    }
    let saved = save_model_provider(pool, actor, existing.map(|p| p.id), &wanted)
        .await
        .context("save model provider")?;
    Ok(saved.id)
}

async fn ensure_persona(pool: &PgPool, actor: Actor<'_>, provider: Uuid) -> Result<()> {
    let wanted = PersonaInput {
        name: data::PERSONA_NAME.into(),
        description: String::new(),
        system_prompt: "You are the e2e persona.".into(),
        harness: df_core::config::CLAUDE_CODE_HARNESS.into(),
        model: data::PERSONA_MODEL.into(),
        model_provider_id: provider,
    };
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
        return Ok(());
    }
    let id = existing.map(|p| p.id);
    save_persona(pool, actor, id, &wanted)
        .await
        .context("save persona")?;
    Ok(())
}

async fn ensure_project(
    pool: &PgPool,
    actor: Actor<'_>,
    seed: &SeedProject,
    credential: Uuid,
) -> Result<()> {
    let SeedProject {
        key,
        name,
        script,
        hidden,
    } = *seed;
    let wanted = ProjectInput {
        key: key.into(),
        name: name.into(),
        repo_url: data::repo_url(),
        issue_tracker_url: None,
        image: data::PROJECT_IMAGE.into(),
        base_branch: data::BASE_BRANCH.into(),
        adapter_command: data::adapter_command(script),
        forge_credential_id: Some(credential),
        webhook_wake: df_core::webhooks::WebhookWake::Members,
    };
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
            save_project(pool, actor, other.as_ref().map(|p| p.id), &wanted)
                .await
                .with_context(|| format!("save project {key}"))?
                .id
        }
    };
    let mut policy = push_policy(pool, id).await?;
    if policy.hidden_globs != hidden {
        policy.hidden_globs = hidden.iter().map(|glob| (*glob).to_owned()).collect();
        update_project_with_push_policy(pool, actor, id, &wanted, &policy)
            .await
            .with_context(|| format!("save push policy of project {key}"))?;
    }
    Ok(())
}
