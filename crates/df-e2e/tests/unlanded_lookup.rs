//! The lookup of a task whose page never opened finds the task of that creation, not a task of the
//! same project with the description of another test, nor an earlier task of the same test
//! (container-backed, no cluster).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use df_core::{PersonaId, Principal, ProjectId};
use df_db::catalog::{get_project_by_key, list_personas};
use df_db::{MIGRATOR, NewTask, create_task};
use df_e2e::browser::submitted_description;
use df_e2e::{data, db, seed};

async fn create(pool: &df_db::PgPool, description: &str) -> Result<String> {
    let project = get_project_by_key(pool, data::PROJECT_KEY)
        .await?
        .context("project DF")?;
    let persona = list_personas(pool)
        .await?
        .into_iter()
        .next()
        .context("persona")?;
    let (task, _message) = create_task(
        pool,
        NewTask {
            project_id: ProjectId::from_uuid(project.id),
            persona_id: PersonaId::from_uuid(persona.id),
            title: None,
            description: description.to_owned(),
            issue_number: None,
            created_by: Principal::system("test"),
            parent_id: None,
            depends_on: Vec::new(),
        },
    )
    .await?;
    Ok(task.key.to_string())
}

/// When the database created task `key`, as the lookup takes it.
async fn created_at(pool: &df_db::PgPool, key: &str) -> Result<SystemTime> {
    let micros: i64 = sqlx::query_scalar(
        "SELECT (extract(epoch FROM created_at) * 1000000)::bigint FROM tasks WHERE key = $1",
    )
    .bind(key)
    .fetch_one(pool)
    .await?;
    Ok(UNIX_EPOCH + Duration::from_micros(u64::try_from(micros)?))
}

#[tokio::test]
async fn the_lookup_of_an_unlanded_task_finds_only_its_own_task_since_the_submit() -> Result<()> {
    let pool = df_test_postgres::migrated_pool(&MIGRATOR).await;
    seed::run(&pool).await?;
    let own = &submitted_description("Add a greeting file", Some(&"1a2b3c4d".to_owned()));
    let other = &submitted_description("Add a greeting file", Some(&"5e6f7a8b".to_owned()));
    let earlier = create(&pool, own).await?;
    let other_test = create(&pool, other).await?;
    let submitted = create(&pool, own).await?;
    let submitted_at = created_at(&pool, &submitted).await?;

    let since_the_submit =
        db::tasks_submitted_since(&pool, data::PROJECT_KEY, own, submitted_at).await?;
    let since_the_start = db::tasks_submitted_since(
        &pool,
        data::PROJECT_KEY,
        own,
        created_at(&pool, &earlier).await?,
    )
    .await?;

    assert_eq!(since_the_submit, std::slice::from_ref(&submitted));
    assert_eq!(since_the_start, [earlier, submitted]);
    assert!(!since_the_start.contains(&other_test));
    Ok(())
}
