//! Tests of webhook messages, routing and completion on merge.

use df_core::seams::ReviewVerdict;
use df_core::{PersonaId, ProjectId, TaskStatus};
use df_proto::transcript::v1::content_block;
use serde_json::Value;
use uuid::Uuid;

use super::*;
use crate::tasks::test_support::{child_ended_messages, completed_run_ended};
use crate::tasks::{NewTask, create_task, get_task, list_undelivered_messages};

async fn seed_project(pool: &PgPool) -> (ProjectId, PersonaId) {
    let project_id = ProjectId::new();
    let persona_id = PersonaId::new();
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image, base_branch)
         VALUES ($1, 'DF', 'Dark Factory', 'https://forge.example.test/df.git', 'img', 'main')",
    )
    .bind(project_id.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    let provider_id = crate::model_providers::tests::seed_provider(pool).await;
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, harness, model, model_provider_id)
         VALUES ($1, 'Linus', 'You are Linus.', 'claude-code', 'test-model', $2)",
    )
    .bind(persona_id.as_uuid())
    .bind(provider_id)
    .execute(pool)
    .await
    .unwrap();
    (project_id, persona_id)
}

async fn add_task(pool: &PgPool, (project_id, persona_id): (ProjectId, PersonaId)) -> TaskId {
    let (task, _) = create_task(
        pool,
        NewTask {
            project_id,
            persona_id,
            title: Some("Fix login".into()),
            description: "Validate the state.".into(),
            issue_number: None,
            created_by: Principal::system("test"),
            parent_id: None,
            depends_on: Vec::new(),
        },
    )
    .await
    .unwrap();
    task.id
}

async fn seed_task(pool: &PgPool) -> TaskId {
    let project = seed_project(pool).await;
    add_task(pool, project).await
}

async fn set_status(pool: &PgPool, task_id: TaskId, status: &str) {
    sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
        .bind(task_id.as_uuid())
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
}

async fn queue_event(pool: &PgPool, task_id: TaskId, event: &WebhookEvent) -> bool {
    let mut tx = pool.begin().await.unwrap();
    let queued = queue_event_tx(&mut tx, task_id, event).await.unwrap();
    tx.commit().await.unwrap();
    queued
}

async fn undelivered(pool: &PgPool, task_id: TaskId) -> usize {
    list_undelivered_messages(pool, task_id)
        .await
        .unwrap()
        .len()
}

fn review(actor: &str) -> WebhookEvent {
    WebhookEvent::ReviewSubmitted {
        number: 12,
        head: "factory/task/DF-1".into(),
        actor: actor.into(),
        verdict: ReviewVerdict::ChangesRequested,
    }
}

fn ci_failed() -> WebhookEvent {
    WebhookEvent::CiFailed {
        head: "factory/task/DF-1".into(),
        check: "test".into(),
        url: "https://ci.example.test/1".into(),
    }
}

#[tokio::test]
async fn webhook_wake_defaults_to_members_for_existing_projects() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    seed_task(&pool).await;

    let wake: String = sqlx::query_scalar("SELECT webhook_wake FROM projects")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(wake, "members");
}

#[tokio::test]
async fn webhook_wake_rejects_an_unknown_value() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    seed_task(&pool).await;

    let updated = sqlx::query("UPDATE projects SET webhook_wake = 'everyone'")
        .execute(&pool)
        .await;
    assert!(updated.is_err());
}

#[tokio::test]
async fn webhook_event_queues_one_message_per_event() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task_id = seed_task(&pool).await;
    let before = undelivered(&pool, task_id).await;

    assert!(queue_event(&pool, task_id, &review("alice")).await);
    assert!(queue_event(&pool, task_id, &ci_failed()).await);

    let messages = list_undelivered_messages(&pool, task_id).await.unwrap();
    assert_eq!(messages.len(), before + 2);
    let message = messages.last().unwrap();
    assert_eq!(message.author, Principal::system(WEBHOOK));
    let [block] = message.content.as_slice() else {
        panic!("expected one block: {:?}", message.content);
    };
    let Some(content_block::Kind::Text(text)) = &block.kind else {
        panic!("expected a text block: {block:?}");
    };
    assert_eq!(text.text, render_message(&ci_failed()));
}

#[tokio::test]
async fn webhook_event_of_a_finished_task_is_dropped() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let project = seed_project(&pool).await;
    for status in ["completed", "cancelled", "failed"] {
        let task_id = add_task(&pool, project).await;
        set_status(&pool, task_id, status).await;
        let before = undelivered(&pool, task_id).await;

        assert!(
            !queue_event(&pool, task_id, &review("alice")).await,
            "{status}"
        );
        assert_eq!(undelivered(&pool, task_id).await, before, "{status}");
    }
}

#[tokio::test]
async fn webhook_event_for_a_deleted_task_is_dropped() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;

    assert!(!queue_event(&pool, TaskId::new(), &ci_failed()).await);
}

#[tokio::test]
async fn webhook_event_queues_a_waiting_task() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task_id = seed_task(&pool).await;
    set_status(&pool, task_id, "waiting").await;

    assert!(queue_event(&pool, task_id, &ci_failed()).await);
    let task = get_task(&pool, task_id).await.unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Queued);
}

#[tokio::test]
async fn webhook_message_has_no_payload_text() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task_id = seed_task(&pool).await;
    let comment = WebhookEvent::CommentCreated {
        number: 12,
        is_pull_request: true,
        actor: "carol".into(),
        mentions: vec!["df-bot".into()],
    };

    queue_event(&pool, task_id, &comment).await;
    let messages = list_undelivered_messages(&pool, task_id).await.unwrap();
    let text = serde_json::to_string(&messages.last().unwrap().content).unwrap();
    assert!(text.contains("carol commented on PR #12."), "{text}");
    assert!(!text.contains("df-bot"), "{text}");
}

async fn set_numbers(pool: &PgPool, task_id: TaskId, pr: Option<i64>, issue: Option<i64>) {
    sqlx::query("UPDATE tasks SET pr_number = $2, issue_number = $3 WHERE id = $1")
        .bind(task_id.as_uuid())
        .bind(pr)
        .bind(issue)
        .execute(pool)
        .await
        .unwrap();
}

async fn complete_on_merge(pool: &PgPool, task_id: TaskId) -> bool {
    let mut tx = pool.begin().await.unwrap();
    let completed = complete_on_merge_tx(&mut tx, task_id, "carol")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    completed
}

async fn done_audits(pool: &PgPool) -> Vec<(Value, Value)> {
    sqlx::query_as("SELECT actor, details FROM audit_log WHERE action = 'task.done'")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn webhook_projects_lists_every_project_with_its_wake_policy() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project_id, _) = seed_project(&pool).await;
    sqlx::query("UPDATE projects SET webhook_wake = 'anyone'")
        .execute(&pool)
        .await
        .unwrap();

    let projects = webhook_projects(&pool).await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, project_id);
    assert_eq!(projects[0].repo_url, "https://forge.example.test/df.git");
    assert_eq!(projects[0].wake, WebhookWake::Anyone);
}

#[tokio::test]
async fn target_tasks_finds_the_task_by_branch_pr_or_open_issue() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let project = seed_project(&pool).await;
    let first = add_task(&pool, project).await;
    let second = add_task(&pool, project).await;
    let done = add_task(&pool, project).await;
    set_numbers(&pool, first, Some(12), Some(7)).await;
    set_numbers(&pool, second, None, Some(7)).await;
    set_numbers(&pool, done, None, Some(7)).await;
    set_status(&pool, done, "completed").await;

    let by_branch = target_tasks(
        &pool,
        project.0,
        &Target::Branch("factory/task/DF-2".into()),
    )
    .await
    .unwrap();
    assert_eq!(by_branch, vec![second]);
    let by_pr = target_tasks(&pool, project.0, &Target::PullRequest(12))
        .await
        .unwrap();
    assert_eq!(by_pr, vec![first]);
    let mut by_issue = target_tasks(&pool, project.0, &Target::Issue(7))
        .await
        .unwrap();
    by_issue.sort_by_key(ToString::to_string);
    let mut open = vec![first, second];
    open.sort_by_key(ToString::to_string);
    assert_eq!(by_issue, open);
    let other = target_tasks(&pool, ProjectId::new(), &Target::PullRequest(12))
        .await
        .unwrap();
    assert!(other.is_empty());
}

#[tokio::test]
async fn merge_completes_the_task_and_audits_the_merging_login() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task_id = seed_task(&pool).await;
    set_status(&pool, task_id, "in_review").await;

    assert!(complete_on_merge(&pool, task_id).await);
    let task = get_task(&pool, task_id).await.unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Completed);
    let audits = done_audits(&pool).await;
    assert_eq!(audits.len(), 1);
    let (actor, details) = &audits[0];
    assert_eq!(actor["reason"], "pr.merged", "{actor}");
    assert_eq!(details["old_status"], "in_review");
    assert_eq!(details["login"], "carol");
}

#[tokio::test]
async fn merge_stops_the_active_run() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task_id = seed_task(&pool).await;
    set_status(&pool, task_id, "running").await;
    sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'running')")
        .bind(Uuid::now_v7())
        .bind(task_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    assert!(complete_on_merge(&pool, task_id).await);
    let (state, stopped): (String, bool) =
        sqlx::query_as("SELECT state, stop_requested_at IS NOT NULL FROM runs")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, "finalizing");
    assert!(stopped);
}

#[tokio::test]
async fn merge_of_a_cancelled_task_only_records_the_merge() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task_id = seed_task(&pool).await;
    set_status(&pool, task_id, "cancelled").await;

    assert!(complete_on_merge(&pool, task_id).await);
    let task = get_task(&pool, task_id).await.unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Cancelled);
    let merged: bool = sqlx::query_scalar("SELECT pr_merged_at IS NOT NULL FROM tasks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(merged);
    assert!(done_audits(&pool).await.is_empty());
}

#[tokio::test]
async fn merge_redelivery_is_a_no_op() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task_id = seed_task(&pool).await;

    assert!(complete_on_merge(&pool, task_id).await);
    assert!(!complete_on_merge(&pool, task_id).await);
    assert_eq!(done_audits(&pool).await.len(), 1);
}

#[tokio::test]
async fn merge_during_a_run_keeps_the_undelivered_messages() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task_id = seed_task(&pool).await;
    set_status(&pool, task_id, "running").await;
    crate::tasks::queue_message(
        &pool,
        task_id,
        Principal::system("test"),
        vec![text_block("and the tests".into(), false)],
    )
    .await
    .unwrap();
    let before = list_undelivered_messages(&pool, task_id)
        .await
        .unwrap()
        .len();

    assert!(complete_on_merge(&pool, task_id).await);
    let after = list_undelivered_messages(&pool, task_id)
        .await
        .unwrap()
        .len();
    assert_eq!(before, after);
}

#[tokio::test]
async fn closed_pr_wakes_the_task_without_changing_its_status_otherwise() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task_id = seed_task(&pool).await;
    set_status(&pool, task_id, "in_review").await;
    set_numbers(&pool, task_id, Some(12), None).await;
    let closed = WebhookEvent::PullRequestClosed {
        number: 12,
        head: "factory/task/DF-1".into(),
        actor: "carol".into(),
        merged: false,
    };
    assert!(queue_event(&pool, task_id, &closed).await);

    let task = get_task(&pool, task_id).await.unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Queued);
    assert_eq!(task.pr_number, Some(12));
}

/// A child task of `parent`, created by the parent's agent.
async fn add_child(
    pool: &PgPool,
    (project_id, persona_id): (ProjectId, PersonaId),
    parent: TaskId,
) -> TaskId {
    let (task, _) = create_task(
        pool,
        NewTask {
            project_id,
            persona_id,
            title: Some("Child".into()),
            description: String::new(),
            issue_number: None,
            created_by: Principal::agent(persona_id, "Linus", None, parent),
            parent_id: Some(parent),
            depends_on: Vec::new(),
        },
    )
    .await
    .unwrap();
    task.id
}

#[tokio::test]
async fn a_child_that_completed_and_is_merged_later_wakes_its_parent_twice() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let project = seed_project(&pool).await;
    let parent = add_task(&pool, project).await;
    let child = add_child(&pool, project, parent).await;
    set_status(&pool, child, "running").await;
    let run_id = df_core::RunId::new();
    sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'succeeded')")
        .bind(run_id.as_uuid())
        .bind(child.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    apply_task_event(
        &mut tx,
        child,
        completed_run_ended(run_id),
        &Principal::system("test"),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(child_ended_messages(&pool, parent).await.len(), 1);

    assert!(complete_on_merge(&pool, child).await);

    assert_eq!(child_ended_messages(&pool, parent).await.len(), 2);
}

#[tokio::test]
async fn a_pull_request_closed_without_merge_leaves_a_done_dependency_unsatisfied() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let project = seed_project(&pool).await;
    let target = add_task(&pool, project).await;
    set_status(&pool, target, "in_review").await;
    set_numbers(&pool, target, Some(20), None).await;
    let dependent = add_done_dependent(&pool, project).await;
    let closed = WebhookEvent::PullRequestClosed {
        number: 20,
        head: "factory/task/DF-1".into(),
        actor: "carol".into(),
        merged: false,
    };
    assert!(queue_event(&pool, target, &closed).await);

    let dependent = get_task(&pool, dependent).await.unwrap().unwrap();
    assert_eq!(dependent.status, TaskStatus::AwaitingDependencies);
}

/// A task that waits for the `done` of `DF-1`.
async fn add_done_dependent(pool: &PgPool, project: (ProjectId, PersonaId)) -> TaskId {
    let (task, _) = create_task(
        pool,
        NewTask {
            project_id: project.0,
            persona_id: project.1,
            title: Some("Dependent".into()),
            description: String::new(),
            issue_number: None,
            created_by: Principal::system("test"),
            parent_id: None,
            depends_on: vec![crate::tasks::NewDependency {
                key: "DF-1".parse().unwrap(),
                kind: df_core::DependencyKind::Done,
            }],
        },
    )
    .await
    .unwrap();
    task.id
}

#[tokio::test]
async fn a_branch_routes_to_its_open_holder_else_to_the_latest_task() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let project = seed_project(&pool).await;
    let old = add_task(&pool, project).await;
    let newer = add_task(&pool, project).await;
    set_status(&pool, old, "cancelled").await;
    for task in [old, newer] {
        sqlx::query("UPDATE tasks SET branch = 'renovate/serde-1.x' WHERE id = $1")
            .bind(task.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
    }
    let branch = Target::Branch("renovate/serde-1.x".into());

    // `newer` is open: it holds the branch.
    assert_eq!(
        target_tasks(&pool, project.0, &branch).await.unwrap(),
        vec![newer]
    );

    // Nobody holds it: the most recently created task gets the event.
    set_status(&pool, newer, "completed").await;
    assert_eq!(
        target_tasks(&pool, project.0, &branch).await.unwrap(),
        vec![newer]
    );

    let unknown = Target::Branch("feature/login".into());
    assert!(
        target_tasks(&pool, project.0, &unknown)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn an_older_open_holder_of_a_branch_gets_the_event_before_a_newer_final_task() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let project = seed_project(&pool).await;
    let open = add_task(&pool, project).await;
    let newer_final = add_task(&pool, project).await;
    set_status(&pool, newer_final, "completed").await;
    for task in [open, newer_final] {
        sqlx::query("UPDATE tasks SET branch = 'renovate/serde-1.x' WHERE id = $1")
            .bind(task.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
    }

    let routed = target_tasks(
        &pool,
        project.0,
        &Target::Branch("renovate/serde-1.x".into()),
    )
    .await
    .unwrap();

    assert_eq!(routed, vec![open]);
}
