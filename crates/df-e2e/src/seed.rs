//! The seed: credential reference, persona and projects of the demo. Sandboxes reach Forgejo
//! only through the gateway's Git proxy, so there is no forge allowlist entry.
//!
//! Idempotent: a record is looked up by name or key and only written when it differs, so a second
//! run changes nothing (and writes no audit entries).

use anyhow::{Context, Result};
use df_core::Principal;
use df_core::model_provider::{CredentialScheme, ProviderApi, ProviderModel, WireApi};
use df_db::PgPool;
use df_db::catalog::{
    Actor, CredentialRefInput, PersonaInput, ProjectInput, get_project_by_key,
    list_credential_refs, list_personas, save_credential_ref, save_persona, save_project,
};
use df_db::model_providers::{ModelProviderInput, list_model_providers, save_model_provider};
use uuid::Uuid;

use crate::data;

/// Writes the well-known data into the database behind `pool`.
pub async fn run(pool: &PgPool) -> Result<()> {
    let principal = Principal::system("e2e seed");
    let actor = Actor {
        principal: &principal,
        roles: &[],
    };

    let credential = ensure_credential(pool, actor).await?;
    let provider = ensure_model_provider(pool, actor).await?;
    ensure_persona(pool, actor, provider).await?;
    ensure_project(
        pool,
        actor,
        data::PROJECT_KEY,
        data::PROJECT_NAME,
        data::DEMO_SCRIPT,
        credential,
    )
    .await?;
    ensure_project(
        pool,
        actor,
        data::RATE_LIMIT_PROJECT_KEY,
        data::RATE_LIMIT_PROJECT_NAME,
        data::RATE_LIMIT_SCRIPT,
        credential,
    )
    .await?;
    ensure_project(
        pool,
        actor,
        data::NETWORK_PROJECT_KEY,
        data::NETWORK_PROJECT_NAME,
        data::NETWORK_SCRIPT,
        credential,
    )
    .await?;
    ensure_project(
        pool,
        actor,
        data::SECRET_PROJECT_KEY,
        data::SECRET_PROJECT_NAME,
        data::SECRET_SCRIPT,
        credential,
    )
    .await?;
    ensure_project(
        pool,
        actor,
        data::PROTECTED_PROJECT_KEY,
        data::PROTECTED_PROJECT_NAME,
        data::PROTECTED_SCRIPT,
        credential,
    )
    .await?;
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
        && c.secret_name == wanted.secret_name
        && c.secret_key == wanted.secret_key
    {
        return Ok(c.id.as_uuid());
    }
    let id = existing.map(|c| c.id.as_uuid());
    let saved = save_credential_ref(pool, actor, id, &wanted)
        .await
        .context("save credential reference")?;
    Ok(saved.id.as_uuid())
}

/// The fake Anthropic upstream with its credential reference.
async fn ensure_model_provider(pool: &PgPool, actor: Actor<'_>) -> Result<Uuid> {
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
            if c.secret_name == wanted_credential.secret_name
                && c.secret_key == wanted_credential.secret_key =>
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
    let wanted = ModelProviderInput {
        name: data::MODEL_PROVIDER_NAME.into(),
        credential_ref_id: credential.id.as_uuid(),
        scheme: CredentialScheme::XApiKey,
        credential_header: None,
        credential_prefix: None,
        headers: Vec::new(),
        apis: vec![ProviderApi {
            wire_api: WireApi::AnthropicMessages,
            upstream_url: data::fake_upstream_url(),
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
    key: &str,
    name: &str,
    script: &str,
    credential: Uuid,
) -> Result<()> {
    let wanted = ProjectInput {
        key: key.into(),
        name: name.into(),
        repo_url: data::repo_url(),
        issue_tracker_url: None,
        image: data::PROJECT_IMAGE.into(),
        base_branch: data::BASE_BRANCH.into(),
        adapter_command: data::adapter_command(script),
        forge_credential_id: Some(credential),
    };
    let existing = get_project_by_key(pool, key).await?;
    if let Some(p) = &existing
        && p.name == wanted.name
        && p.repo_url == wanted.repo_url
        && p.issue_tracker_url == wanted.issue_tracker_url
        && p.image == wanted.image
        && p.base_branch == wanted.base_branch
        && p.adapter_command == wanted.adapter_command
        && p.forge_credential_id == wanted.forge_credential_id
    {
        return Ok(());
    }
    let id = existing.map(|p| p.id);
    save_project(pool, actor, id, &wanted)
        .await
        .with_context(|| format!("save project {key}"))?;
    Ok(())
}
