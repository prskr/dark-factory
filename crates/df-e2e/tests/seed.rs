//! The seed writes the demo data once and changes nothing on the second run.

use anyhow::{Context, Result};
use df_db::MIGRATOR;
use df_db::catalog::{get_project_by_key, list_credential_refs, list_personas};
use df_db::model_providers::list_model_providers;
use df_e2e::{data, seed};

async fn audit_entries(pool: &df_db::PgPool) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM audit_log")
        .fetch_one(pool)
        .await?)
}

#[tokio::test]
async fn seed_writes_the_demo_data() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&MIGRATOR).await;

    seed::run(&pool).await?;

    let credentials = list_credential_refs(&pool).await?;
    assert_eq!(credentials.len(), 2, "forge and model credential");
    let forge = credentials
        .iter()
        .find(|c| c.name == data::CREDENTIAL_NAME)
        .context("forge credential")?;
    let providers = list_model_providers(&pool).await?;
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].input.name, data::MODEL_PROVIDER_NAME);
    assert_eq!(
        providers[0].input.apis[0].upstream_url,
        data::fake_upstream_url()
    );
    let personas = list_personas(&pool).await?;
    assert_eq!(personas[0].name, data::PERSONA_NAME);
    assert_eq!(personas[0].model_provider_id, providers[0].id);
    assert_eq!(personas[0].model, data::PERSONA_MODEL);
    assert_eq!(providers[0].input.models.len(), 1);
    let project = get_project_by_key(&pool, data::PROJECT_KEY)
        .await?
        .context("project DF")?;
    assert_eq!(project.repo_url, data::repo_url());
    assert_eq!(project.forge_credential_id, Some(forge.id.as_uuid()));
    assert_eq!(
        project.adapter_command,
        data::adapter_command(data::DEMO_SCRIPT)
    );
    let rate_limit = get_project_by_key(&pool, data::RATE_LIMIT_PROJECT_KEY)
        .await?
        .context("project RL")?;
    assert_eq!(
        rate_limit.adapter_command,
        data::adapter_command(data::RATE_LIMIT_SCRIPT)
    );
    let network = get_project_by_key(&pool, data::NETWORK_PROJECT_KEY)
        .await?
        .context("project NA")?;
    assert_eq!(
        network.adapter_command,
        data::adapter_command(data::NETWORK_SCRIPT)
    );
    for (key, script) in [
        (data::SECRET_PROJECT_KEY, data::SECRET_SCRIPT),
        (data::PROTECTED_PROJECT_KEY, data::PROTECTED_SCRIPT),
        (data::DELEGATION_PROJECT_KEY, data::DELEGATION_SCRIPT),
    ] {
        let project = get_project_by_key(&pool, key)
            .await?
            .with_context(|| format!("project {key}"))?;
        assert_eq!(project.adapter_command, data::adapter_command(script));
    }
    // Sandboxes reach Forgejo through the gateway's Git proxy: no allowlist entry.
    assert!(df_db::list_entries(&pool).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn second_seed_run_changes_nothing() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&MIGRATOR).await;
    seed::run(&pool).await?;
    let before = audit_entries(&pool).await?;

    seed::run(&pool).await?;

    assert_eq!(audit_entries(&pool).await?, before);
    Ok(())
}
