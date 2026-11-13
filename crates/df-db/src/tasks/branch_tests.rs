//! Tests of the stored task branch: the migration, creation on an existing branch, the "one open
//! task per branch" rule, reopening and the effective base branch (`task-branches`).

use df_core::{PersonaId, ProjectId, TaskStatus};
use sqlx::PgPool;
use uuid::Uuid;

use super::tests::{human, new_task, seed_project_and_persona, text};
use super::*;

struct Env {
    pool: PgPool,
    project: ProjectId,
    persona: PersonaId,
}

async fn env() -> Env {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let key: ProjectKey = "DF".parse().unwrap();
    let (project, persona) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();
    Env {
        pool,
        project,
        persona,
    }
}

fn placement(branch: &str) -> TaskPlacement {
    TaskPlacement {
        branch: Some(ExistingBranch::parse(branch, "main").unwrap()),
        attached_pr: None,
    }
}

impl Env {
    async fn create_on(&self, placement: TaskPlacement) -> Result<TaskRow, DbError> {
        let input = new_task(self.project, self.persona, Some("T"));
        create_task_on(&self.pool, input, placement)
            .await
            .map(|(task, _)| task)
    }

    async fn set_status(&self, task: TaskId, status: &str) {
        sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
            .bind(task.as_uuid())
            .bind(status)
            .execute(&self.pool)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn the_migration_backfills_the_branch_and_the_rewrite_policy() {
    let pool = df_test_postgres::empty_pool().await;
    let before = crate::MIGRATOR
        .iter()
        .find(|m| m.description.contains("branch workflows"))
        .map(|m| m.version - 1)
        .unwrap();
    crate::MIGRATOR.run_to(before, &pool).await.unwrap();
    let key: ProjectKey = "DF".parse().unwrap();
    let (project, persona) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id,
             created_by)
         VALUES ($1, $2, 1, 'DF-1', 'T', 'user', $3, '{\"kind\":\"system\",\"reason\":\"test\"}')",
    )
    .bind(Uuid::now_v7())
    .bind(project.as_uuid())
    .bind(persona.as_uuid())
    .execute(&pool)
    .await
    .unwrap();

    crate::MIGRATOR.run(&pool).await.unwrap();

    let branch: String = sqlx::query_scalar("SELECT branch FROM tasks WHERE key = 'DF-1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let policy: String = sqlx::query_scalar("SELECT branch_rewrite_policy FROM projects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        (branch.as_str(), policy.as_str()),
        ("factory/task/DF-1", "with-lease")
    );
}

#[tokio::test]
async fn the_database_allows_one_open_task_per_branch() {
    let env = env().await;
    let first = env.create_on(placement("fix/login")).await.unwrap();

    let duplicate = sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id,
             created_by, branch)
         VALUES ($1, $2, 99, 'DF-99', 'T', 'user', $3, '{\"kind\":\"system\",\"reason\":\"t\"}', $4)",
    )
    .bind(Uuid::now_v7())
    .bind(env.project.as_uuid())
    .bind(env.persona.as_uuid())
    .bind("fix/login")
    .execute(&env.pool)
    .await;
    assert!(duplicate.is_err());

    env.set_status(first.id, "cancelled").await;
    assert!(env.create_on(placement("fix/login")).await.is_ok());
}

#[tokio::test]
async fn a_new_task_stores_its_own_branch_and_an_existing_one_the_given_branch() {
    let env = env().await;

    let own = env.create_on(TaskPlacement::default()).await.unwrap();
    let existing = env
        .create_on(placement("renovate/serde-1.x"))
        .await
        .unwrap();

    assert_eq!(own.branch, "factory/task/DF-1");
    assert!(!own.pr_attached && own.base_branch.is_none());
    assert_eq!(existing.branch, "renovate/serde-1.x");
    let stored = get_task(&env.pool, existing.id).await.unwrap().unwrap();
    assert_eq!(stored, existing);
}

#[tokio::test]
async fn creating_on_a_held_branch_names_the_holder() {
    let env = env().await;
    let holder = env
        .create_on(placement("renovate/serde-1.x"))
        .await
        .unwrap();
    env.set_status(holder.id, "in_review").await;

    let err = env
        .create_on(placement("renovate/serde-1.x"))
        .await
        .unwrap_err();

    assert!(
        matches!(&err, DbError::BranchHeld { task: Some(t) } if *t == holder.key),
        "{err:?}"
    );
    assert!(err.to_string().contains("DF-1"));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&env.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn creation_with_an_attached_pr_stores_number_url_base_and_flag() {
    let env = env().await;
    let attached = AttachedPr {
        number: 41,
        url: "https://forge.example/pulls/41".into(),
        base: "release/1.2".into(),
    };

    let task = env
        .create_on(TaskPlacement {
            branch: Some(ExistingBranch::parse("fix/cve", "main").unwrap()),
            attached_pr: Some(attached),
        })
        .await
        .unwrap();

    let stored = get_task(&env.pool, task.id).await.unwrap().unwrap();
    assert_eq!(stored.pr_number, Some(41));
    assert_eq!(
        stored.pr_url.as_deref(),
        Some("https://forge.example/pulls/41")
    );
    assert_eq!(stored.base_branch.as_deref(), Some("release/1.2"));
    assert!(stored.pr_attached);
}

#[tokio::test]
async fn reopening_a_final_task_fails_while_another_task_holds_its_branch() {
    let env = env().await;
    let old = env
        .create_on(placement("renovate/serde-1.x"))
        .await
        .unwrap();
    env.set_status(old.id, "cancelled").await;
    let new = env
        .create_on(placement("renovate/serde-1.x"))
        .await
        .unwrap();

    let err = queue_message(&env.pool, old.id, human(), text("again"))
        .await
        .unwrap_err();

    assert!(
        matches!(&err, DbError::BranchHeld { task: Some(t) } if *t == new.key),
        "{err:?}"
    );
    let stored = get_task(&env.pool, old.id).await.unwrap().unwrap();
    assert_eq!(stored.status, TaskStatus::Cancelled);
    let messages = list_undelivered_messages(&env.pool, old.id).await.unwrap();
    assert!(messages.iter().all(|m| m.author != human()));
}

#[tokio::test]
async fn reopening_a_final_task_works_while_its_branch_is_free() {
    let env = env().await;
    let old = env
        .create_on(placement("renovate/serde-1.x"))
        .await
        .unwrap();
    env.set_status(old.id, "cancelled").await;

    queue_message(&env.pool, old.id, human(), text("again"))
        .await
        .unwrap();

    let stored = get_task(&env.pool, old.id).await.unwrap().unwrap();
    assert_eq!(stored.status, TaskStatus::Queued);
}

#[tokio::test]
async fn the_effective_base_branch_is_the_attached_pr_base_before_the_project_base() {
    let env = env().await;
    let plain = env.create_on(placement("fix/login")).await.unwrap();
    let attached = env
        .create_on(TaskPlacement {
            branch: Some(ExistingBranch::parse("fix/cve", "main").unwrap()),
            attached_pr: Some(AttachedPr {
                number: 41,
                url: "u".into(),
                base: "release/1.2".into(),
            }),
        })
        .await
        .unwrap();

    assert_eq!(plain.base_branch("main"), "main");
    assert_eq!(attached.base_branch("main"), "release/1.2");
    assert!(attached.on_existing_branch() && plain.on_existing_branch());
    let own = env.create_on(TaskPlacement::default()).await.unwrap();
    assert!(!own.on_existing_branch());
}
