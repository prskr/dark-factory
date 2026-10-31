//! The seed writes the demo data once and changes nothing on the second run.

use anyhow::{Context, Result};
use df_db::MIGRATOR;
use df_db::catalog::{get_project_by_key, list_credential_refs, list_personas};
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
    assert_eq!(credentials.len(), 1);
    assert_eq!(credentials[0].name, data::CREDENTIAL_NAME);
    let personas = list_personas(&pool).await?;
    assert_eq!(personas[0].name, data::PERSONA_NAME);
    let project = get_project_by_key(&pool, data::PROJECT_KEY)
        .await?
        .context("project DF")?;
    assert_eq!(project.repo_url, data::repo_url());
    assert_eq!(
        project.forge_credential_id,
        Some(credentials[0].id.as_uuid())
    );
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
    assert_eq!(df_db::list_entries(&pool).await?.len(), 1);
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
