//! Tests of the task queries.

use super::*;
use crate::notify::Notifier;
use std::collections::HashSet;
use std::time::Duration;
use uuid::Uuid;

async fn seed_project_and_persona(
    pool: &PgPool,
    project_key: &ProjectKey,
    persona_name: &str,
) -> Result<(ProjectId, PersonaId), DbError> {
    let project_id = ProjectId::new();
    let persona_id = PersonaId::new();

    sqlx::query(
        r#"
            INSERT INTO projects (id, key, name, repo_url, image, base_branch)
            VALUES ($1, $2, $3, $4, $5, 'main')
            "#,
    )
    .bind(project_id.as_uuid())
    .bind(project_key.as_str())
    .bind(format!("Project {project_key}"))
    .bind("https://forge.example.test/prskr/dark-factory.git")
    .bind("harbor.example.test/df/project:latest")
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
            INSERT INTO personas (id, name, system_prompt, harness)
            VALUES ($1, $2, $3, 'claude-code')
            "#,
    )
    .bind(persona_id.as_uuid())
    .bind(persona_name)
    .bind("You are Linus, a careful software engineer.")
    .execute(pool)
    .await?;

    Ok((project_id, persona_id))
}

#[tokio::test]
async fn first_task_gets_df_1_and_queues_assignment_prompt() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let project_key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &project_key, "Linus")
        .await
        .unwrap();

    let (task, msg) = create_task(
        &pool,
        NewTask {
            project_id,
            persona_id,
            title: Some("Fix login".into()),
            description: "Ensure OIDC callback validates state.".into(),
            issue_number: Some(42),
            created_by: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(task.key.to_string(), "DF-1");
    assert_eq!(task.number, 1);
    assert_eq!(task.status, TaskStatus::Queued);
    assert_eq!(task.title, "Fix login");
    assert_eq!(task.issue_number, Some(42));

    assert_eq!(msg.task_id, task.id);
    assert_eq!(msg.author, Principal::system("task.assigned"));
    assert!(msg.delivered_at.is_none());
    assert!(msg.delivered_run_id.is_none());

    let undelivered = list_undelivered_messages(&pool, task.id).await.unwrap();
    assert_eq!(undelivered.len(), 1);
    assert_eq!(undelivered[0].id, msg.id);
}

#[tokio::test]
async fn concurrent_task_creation_allocates_consecutive_gap_free_keys() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let project_key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &project_key, "Linus")
        .await
        .unwrap();

    let mut handles = Vec::new();
    for i in 0..8 {
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            create_task(
                &pool,
                NewTask {
                    project_id,
                    persona_id,
                    title: Some(format!("Concurrent task {i}")),
                    description: String::new(),
                    issue_number: None,
                    created_by: None,
                },
            )
            .await
            .expect("concurrent create_task should succeed")
            .0
        }));
    }

    let mut keys = HashSet::new();
    let mut numbers = Vec::new();
    for h in handles {
        let t = h.await.unwrap();
        keys.insert(t.key.to_string());
        numbers.push(t.number);
    }
    numbers.sort_unstable();
    assert_eq!(numbers, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    for n in 1..=8 {
        assert!(keys.contains(&format!("DF-{n}")));
    }
}

#[tokio::test]
async fn queueing_message_on_waiting_or_failed_or_rate_limited_task() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut rx = notifier.subscribe();

    let project_key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &project_key, "Linus")
        .await
        .unwrap();

    let (task, initial_prompt) = create_task(
        &pool,
        NewTask {
            project_id,
            persona_id,
            title: Some("Initial work".into()),
            description: "Desc".into(),
            issue_number: None,
            created_by: None,
        },
    )
    .await
    .unwrap();

    // Drain initial creation notifications
    let _ = rx.recv().await.unwrap();
    let _ = rx.recv().await.unwrap();

    // Create a dummy run to mark the initial prompt delivered
    let run_id = RunId::new();
    sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'running')")
        .bind(run_id.as_uuid())
        .bind(task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    assert!(
        mark_prompt_delivered(&pool, initial_prompt.id, run_id)
            .await
            .unwrap()
    );
    assert!(
        list_undelivered_messages(&pool, task.id)
            .await
            .unwrap()
            .is_empty()
    );

    // The run ended (no idle run to pick the message up), the task is `waiting`
    sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
        .bind(run_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    // Transition task to `waiting`, then queue a follow-up message
    update_task_status(&pool, task.id, TaskStatus::Waiting)
        .await
        .unwrap();
    let _ = rx.recv().await.unwrap(); // task_changed from update_task_status

    let author = Principal::human(UserId::new(), "Peter");
    let content = vec![ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent {
            text: "also add a test for X".into(),
        })),
        hidden: false,
    }];
    let follow_up = queue_message(&pool, task.id, author.clone(), content)
        .await
        .unwrap();
    assert!(follow_up.delivered_at.is_none());

    let reloaded = get_task(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(reloaded.status, TaskStatus::Queued);

    let n1 = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(n1, Notification::TaskMessageQueued { task_id: task.id });
    let _ = rx.recv().await.unwrap(); // task_changed from queue_message

    // Transition task to `failed`, then queue another message -> becomes `queued`
    update_task_status(&pool, task.id, TaskStatus::Failed)
        .await
        .unwrap();
    queue_message(&pool, task.id, author.clone(), vec![])
        .await
        .unwrap();
    let reloaded_after_failed = get_task(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(reloaded_after_failed.status, TaskStatus::Queued);

    // Per `iter1-run-orchestration` ("no new run starts until a message is queued")
    // and `iter1-demo-e2e` ("the task is `rate_limited`, and a new message starts a run
    // that continues the work"), queueing a new message on a `rate_limited` task moves
    // it to `queued`.
    update_task_status(&pool, task.id, TaskStatus::RateLimited)
        .await
        .unwrap();
    queue_message(&pool, task.id, author, vec![]).await.unwrap();
    let reloaded_after_rate_limited = get_task(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(reloaded_after_rate_limited.status, TaskStatus::Queued);

    // If `tasks.number` and `tasks.key` disagree in the row, `get_task` returns an error
    // rather than silently returning inconsistent data.
    sqlx::query("UPDATE tasks SET number = 99 WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let mismatched = get_task(&pool, task.id).await;
    assert!(matches!(mismatched, Err(DbError::InvalidData(_))));
}

#[tokio::test]
async fn database_check_constraints_reject_invalid_project_keys_task_statuses_and_duplicate_active_runs()
 {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    // Invalid project keys `df` and `D` rejected by DB check constraint
    for bad_key in ["df", "D"] {
        let res = sqlx::query(
                "INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, $2, 'Bad', 'https://example.test/r.git', 'img:latest')",
            )
            .bind(Uuid::now_v7())
            .bind(bad_key)
            .execute(&pool)
            .await;
        assert!(
            res.is_err(),
            "DB should reject invalid project key `{bad_key}`"
        );
    }

    let project_key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &project_key, "Linus")
        .await
        .unwrap();
    let (task, _) = create_task(
        &pool,
        NewTask {
            project_id,
            persona_id,
            title: Some("Check constraints".into()),
            description: String::new(),
            issue_number: None,
            created_by: None,
        },
    )
    .await
    .unwrap();

    // Invalid task status rejected by DB check constraint
    let bad_status = sqlx::query("UPDATE tasks SET status = 'bogus_status' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&pool)
        .await;
    assert!(bad_status.is_err(), "DB should reject invalid task status");

    // Partial unique index: at most one active run per task
    sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'running')")
        .bind(Uuid::now_v7())
        .bind(task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let second_active =
        sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 2, 'pending')")
            .bind(Uuid::now_v7())
            .bind(task.id.as_uuid())
            .execute(&pool)
            .await;
    assert!(
        second_active.is_err(),
        "partial unique index should reject second active run on same task"
    );
}

fn new_task(project_id: ProjectId, persona_id: PersonaId, title: Option<&str>) -> NewTask {
    NewTask {
        project_id,
        persona_id,
        title: title.map(str::to_string),
        description: "The nightly backup job sometimes runs twice. It started last week.".into(),
        issue_number: None,
        created_by: None,
    }
}

fn prompt_text(msg: &TaskMessageRow) -> String {
    msg.content
        .iter()
        .map(block_text)
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[test]
fn assignment_prompt_hides_only_the_title_instruction() {
    let blocks = format_assignment_prompt(None, "desc");
    assert_eq!(blocks.len(), 2);
    assert!(!blocks[0].hidden);
    assert_eq!(block_text(&blocks[0]), "desc");
    assert!(blocks[1].hidden);
    assert!(block_text(&blocks[1]).contains("set_task_title"));

    let blocks = format_assignment_prompt(Some("T"), "desc");
    assert!(blocks.iter().all(|b| !b.hidden));
    assert_eq!(blocks.len(), 1);

    let blocks = format_assignment_prompt(None, "");
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].hidden);
}

#[test]
fn content_block_without_hidden_key_reads_as_visible() {
    let block: ContentBlock =
        serde_json::from_value(serde_json::json!({"text": {"text": "x"}})).unwrap();
    assert!(!block.hidden);
    let json = serde_json::to_value(format_assignment_prompt(None, "d")).unwrap();
    assert_eq!(json[1]["hidden"], true);
}

fn block_text(block: &ContentBlock) -> &str {
    match &block.kind {
        Some(content_block::Kind::Text(t)) => &t.text,
        _ => "",
    }
}

#[tokio::test]
async fn task_without_title_gets_a_placeholder_and_asks_for_a_title() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();

    let (task, msg) = create_task(&pool, new_task(project_id, persona_id, None))
        .await
        .unwrap();

    assert_eq!(task.title, "The nightly backup job sometimes runs twice.");
    assert_eq!(task.title_source, TitleSource::Placeholder);
    let text = prompt_text(&msg);
    assert!(text.contains("The nightly backup job sometimes runs twice. It started"));
    assert!(text.contains("set_task_title"));
}

#[tokio::test]
async fn task_with_title_keeps_it_and_the_old_prompt() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();

    let (task, msg) = create_task(&pool, new_task(project_id, persona_id, Some("Fix backups")))
        .await
        .unwrap();

    assert_eq!(task.title, "Fix backups");
    assert_eq!(task.title_source, TitleSource::User);
    let text = prompt_text(&msg);
    assert!(text.starts_with("# Fix backups\n\n"));
    assert!(!text.contains("set_task_title"));
    assert_eq!(get_task(&pool, task.id).await.unwrap().unwrap(), task);
}

#[tokio::test]
async fn title_precedence_holds_for_every_source_pair() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();

    for current in TitleSource::ALL {
        for new in TitleSource::ALL {
            let (task, _) = create_task(&pool, new_task(project_id, persona_id, None))
                .await
                .unwrap();
            sqlx::query("UPDATE tasks SET title = 'current', title_source = $2 WHERE id = $1")
                .bind(task.id.as_uuid())
                .bind(current.as_str())
                .execute(&pool)
                .await
                .unwrap();

            let changed = set_task_title(&pool, task.id, "new", *new).await.unwrap();

            let stored = get_task(&pool, task.id).await.unwrap().unwrap();
            assert_eq!(changed, new.may_replace(*current), "{new} over {current}");
            if changed {
                assert_eq!((stored.title.as_str(), stored.title_source), ("new", *new));
            } else {
                assert_eq!(
                    (stored.title.as_str(), stored.title_source),
                    ("current", *current)
                );
            }
        }
    }
}

#[tokio::test]
async fn agent_title_is_sanitized_and_empty_is_rejected() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();
    let (task, _) = create_task(&pool, new_task(project_id, persona_id, None))
        .await
        .unwrap();

    assert!(
        set_task_title(&pool, task.id, " \n ", TitleSource::Agent)
            .await
            .is_err()
    );
    set_task_title(
        &pool,
        task.id,
        "Fix login\n\nand logout",
        TitleSource::Agent,
    )
    .await
    .unwrap();

    let stored = get_task(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(stored.title, "Fix login and logout");
}

#[tokio::test]
async fn rename_rejects_a_title_over_the_limit() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();
    let (task, _) = create_task(&pool, new_task(project_id, persona_id, Some("Keep")))
        .await
        .unwrap();

    let long = "x".repeat(TITLE_MAX_CHARS + 1);
    let result = rename_task(
        &pool,
        task.id,
        Principal::human(UserId::new(), "Ada"),
        &long,
    )
    .await;

    assert!(matches!(result, Err(DbError::InvalidData(_))));
    assert_eq!(
        get_task(&pool, task.id).await.unwrap().unwrap().title,
        "Keep"
    );
}

#[tokio::test]
async fn rename_locks_the_title_and_is_audited() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();
    let (task, _) = create_task(&pool, new_task(project_id, persona_id, None))
        .await
        .unwrap();
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut events = notifier.subscribe();

    rename_task(
        &pool,
        task.id,
        Principal::human(UserId::new(), "Ada"),
        "Stop duplicate nightly backups",
    )
    .await
    .unwrap();
    let agent_changed = set_task_title(&pool, task.id, "Agent title", TitleSource::Agent)
        .await
        .unwrap();

    let stored = get_task(&pool, task.id).await.unwrap().unwrap();
    assert_eq!(stored.title, "Stop duplicate nightly backups");
    assert_eq!(stored.title_source, TitleSource::User);
    assert!(!agent_changed);
    let (action, details): (String, serde_json::Value) = sqlx::query_as(
        "SELECT action, details FROM audit_log WHERE resource_id = $1 AND action = 'task.rename'",
    )
    .bind(task.id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(action, "task.rename");
    assert_eq!(
        details["old_title"],
        "The nightly backup job sometimes runs twice."
    );
    assert_eq!(details["new_title"], "Stop duplicate nightly backups");
    let changed = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(changed, Notification::TaskChanged { task_id: task.id });
}
async fn insert_run(pool: &PgPool, task: TaskId) -> RunId {
    let run_id = RunId::new();
    sqlx::query(
        "INSERT INTO runs (id, task_id, number, state)
             SELECT $1, $2, COALESCE(max(number), 0) + 1, 'succeeded' FROM runs WHERE task_id = $2",
    )
    .bind(run_id.as_uuid())
    .bind(task.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    run_id
}

fn text(t: &str) -> Vec<ContentBlock> {
    vec![ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent { text: t.into() })),
        hidden: false,
    }]
}

fn human() -> Principal {
    Principal::human(UserId::new(), "Ada")
}

/// A `waiting` task with one human message queued (status `queued`).
async fn task_with_queued_message(pool: &PgPool) -> (TaskRow, TaskMessageRow) {
    let key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(pool, &key, "Linus").await.unwrap();
    let (task, assignment) = create_task(pool, new_task(project_id, persona_id, Some("T")))
        .await
        .unwrap();
    mark_prompt_delivered(pool, assignment.id, insert_run(pool, task.id).await)
        .await
        .unwrap();
    update_task_status(pool, task.id, TaskStatus::Waiting)
        .await
        .unwrap();
    let msg = queue_message(pool, task.id, human(), text("first"))
        .await
        .unwrap();
    (task, msg)
}

async fn status_of(pool: &PgPool, task: TaskId) -> TaskStatus {
    get_task(pool, task).await.unwrap().unwrap().status
}

async fn audit_count(pool: &PgPool, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = $1")
        .bind(action)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A task in `status` with a run in `run_state`; its assignment message is delivered.
async fn task_with_run(pool: &PgPool, status: TaskStatus, run_state: &str) -> TaskRow {
    let key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(pool, &key, "Linus").await.unwrap();
    let (task, assignment) = create_task(pool, new_task(project_id, persona_id, Some("T")))
        .await
        .unwrap();
    let run = insert_run(pool, task.id).await;
    mark_prompt_delivered(pool, assignment.id, run)
        .await
        .unwrap();
    sqlx::query("UPDATE runs SET state = $2 WHERE id = $1")
        .bind(run.as_uuid())
        .bind(run_state)
        .execute(pool)
        .await
        .unwrap();
    update_task_status(pool, task.id, status).await.unwrap();
    task
}

/// A transaction that holds the run row and then wants the task row (`persist_batch`) must
/// not deadlock with `mark_task_done`: that waits for the run row before taking the task row.
#[tokio::test]
async fn mark_done_takes_the_run_lock_before_the_task_lock() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task_with_run(&pool, TaskStatus::Waiting, "awaiting_input").await;
    let mut other = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM runs WHERE task_id = $1 FOR UPDATE")
        .bind(task.id.as_uuid())
        .execute(&mut *other)
        .await
        .unwrap();
    let done = {
        let pool = pool.clone();
        let id = task.id;
        tokio::spawn(async move { mark_task_done(&pool, id, human()).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity
                     WHERE datname = current_database() AND wait_event_type = 'Lock'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiting > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("mark_task_done did not wait for the run lock");

    // Without the run lock first, `mark_task_done` now holds the task row and this deadlocks.
    sqlx::query("SELECT 1 FROM tasks WHERE id = $1 FOR UPDATE")
        .bind(task.id.as_uuid())
        .execute(&mut *other)
        .await
        .unwrap();
    other.commit().await.unwrap();
    done.await.unwrap().unwrap();
    assert_eq!(status_of(&pool, task.id).await, TaskStatus::Completed);
}

#[tokio::test]
async fn a_message_on_a_task_with_an_idle_run_makes_it_running() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task_with_run(&pool, TaskStatus::Waiting, "awaiting_input").await;
    queue_message(&pool, task.id, human(), text("again"))
        .await
        .unwrap();
    assert_eq!(status_of(&pool, task.id).await, TaskStatus::Running);
}

#[tokio::test]
async fn a_message_on_a_task_whose_run_is_finalizing_makes_it_queued() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task_with_run(&pool, TaskStatus::Running, "finalizing").await;
    queue_message(&pool, task.id, human(), text("too late"))
        .await
        .unwrap();
    assert_eq!(status_of(&pool, task.id).await, TaskStatus::Queued);
}

#[tokio::test]
async fn a_message_on_a_task_in_review_makes_it_queued() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task_with_run(&pool, TaskStatus::InReview, "succeeded").await;
    queue_message(&pool, task.id, human(), text("one more"))
        .await
        .unwrap();
    assert_eq!(status_of(&pool, task.id).await, TaskStatus::Queued);
}

#[tokio::test]
async fn mark_done_allows_the_statuses_that_wait_for_a_human() {
    for status in [
        TaskStatus::Waiting,
        TaskStatus::InReview,
        TaskStatus::RateLimited,
        TaskStatus::LimitReached,
        TaskStatus::Failed,
    ] {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task_with_run(&pool, status, "succeeded").await;
        mark_task_done(&pool, task.id, human()).await.unwrap();
        assert_eq!(status_of(&pool, task.id).await, TaskStatus::Completed);
        assert_eq!(audit_count(&pool, "task.done").await, 1);
    }
}

#[tokio::test]
async fn mark_done_is_rejected_for_queued_running_and_completed_tasks() {
    for status in [
        TaskStatus::Queued,
        TaskStatus::Running,
        TaskStatus::Completed,
    ] {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let task = task_with_run(&pool, status, "running").await;
        let err = mark_task_done(&pool, task.id, human()).await;
        assert!(matches!(err, Err(DbError::Conflict(_))), "{status}");
        assert_eq!(status_of(&pool, task.id).await, status);
        assert_eq!(audit_count(&pool, "task.done").await, 0);
    }
}

#[tokio::test]
async fn mark_done_finalizes_an_idle_run_and_a_later_message_queues_the_task() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task_with_run(&pool, TaskStatus::Waiting, "awaiting_input").await;
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut events = notifier.subscribe();

    mark_task_done(&pool, task.id, human()).await.unwrap();

    let state: String = sqlx::query_scalar("SELECT state FROM runs WHERE task_id = $1")
        .bind(task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "finalizing");
    let mut seen = Vec::new();
    while seen.len() < 2 {
        seen.push(
            tokio::time::timeout(Duration::from_secs(5), events.recv())
                .await
                .unwrap()
                .unwrap(),
        );
    }
    assert!(
        seen.iter()
            .any(|n| matches!(n, Notification::RunChanged { .. }))
    );
    assert!(seen.contains(&Notification::TaskChanged { task_id: task.id }));

    // The run is going away: a message queued now is for the next run.
    queue_message(&pool, task.id, human(), text("one more"))
        .await
        .unwrap();
    assert_eq!(status_of(&pool, task.id).await, TaskStatus::Queued);
}

#[tokio::test]
async fn claim_marks_held_back_messages_forwarded_once_in_queue_order() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, first) = task_with_queued_message(&pool).await;
    let second = queue_message(&pool, task.id, human(), text("second"))
        .await
        .unwrap();

    let claimed = claim_held_back_messages(&pool, task.id).await.unwrap();
    let ids: Vec<_> = claimed.iter().map(|m| m.id).collect();
    let again = claim_held_back_messages(&pool, task.id).await.unwrap();

    assert_eq!(ids, vec![first.id, second.id]);
    assert!(claimed.iter().all(|m| m.forwarded_at.is_some()));
    assert!(again.is_empty());
    // Forwarded messages stay undelivered until their event is persisted.
    assert_eq!(
        list_undelivered_messages(&pool, task.id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn claim_skips_delivered_messages() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, msg) = task_with_queued_message(&pool).await;
    mark_prompt_delivered(&pool, msg.id, insert_run(&pool, task.id).await)
        .await
        .unwrap();

    assert!(
        claim_held_back_messages(&pool, task.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn message_undelivered_before_the_migration_is_not_editable() {
    let pool = df_test_postgres::empty_pool().await;
    // The migration that added `forwarded_at` is the one the messages predate.
    let versions: Vec<_> = crate::MIGRATOR.iter().collect();
    let at = versions
        .iter()
        .position(|m| m.description.contains("forwarded at"))
        .unwrap();
    let before = versions[at - 1].version;
    crate::MIGRATOR.run_to(before, &pool).await.unwrap();
    let (task, msg) = task_with_queued_message_pre_migration(&pool).await;

    crate::MIGRATOR.run(&pool).await.unwrap();

    let err = edit_queued_message(&pool, task, msg, "new", human()).await;
    assert!(matches!(err, Err(DbError::Conflict(_))));
}

/// Same as [`task_with_queued_message`] for a schema without `forwarded_at`.
async fn task_with_queued_message_pre_migration(pool: &PgPool) -> (TaskId, PromptId) {
    let key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(pool, &key, "Linus").await.unwrap();
    let (task, _) = create_task(pool, new_task(project_id, persona_id, Some("T")))
        .await
        .unwrap();
    let msg = queue_message(pool, task.id, human(), text("old"))
        .await
        .unwrap();
    (task.id, msg.id)
}

#[tokio::test]
async fn edit_replaces_text_keeps_id_and_audits() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, msg) = task_with_queued_message(&pool).await;
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut events = notifier.subscribe();

    edit_queued_message(&pool, task.id, msg.id, "also check logout", human())
        .await
        .unwrap();

    let stored = list_undelivered_messages(&pool, task.id).await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].id, msg.id);
    assert_eq!(stored[0].queued_at, msg.queued_at);
    assert_eq!(prompt_text(&stored[0]), "also check logout");
    assert_eq!(audit_count(&pool, "message.edit").await, 1);
    let details: serde_json::Value =
        sqlx::query_scalar("SELECT details FROM audit_log WHERE action = 'message.edit'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(details["old_text"], "first");
    assert_eq!(details["new_text"], "also check logout");
    let changed = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        changed,
        Notification::TaskMessageChanged { task_id: task.id }
    );
}

#[tokio::test]
async fn edit_and_remove_are_refused_after_forwarding() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, msg) = task_with_queued_message(&pool).await;
    claim_held_back_messages(&pool, task.id).await.unwrap();

    let edit = edit_queued_message(&pool, task.id, msg.id, "late", human()).await;
    let remove = remove_queued_message(&pool, task.id, msg.id, human()).await;

    assert!(matches!(edit, Err(DbError::Conflict(_))));
    assert!(matches!(remove, Err(DbError::Conflict(_))));
    let stored = list_undelivered_messages(&pool, task.id).await.unwrap();
    assert_eq!(prompt_text(&stored[0]), "first");
    assert_eq!(audit_count(&pool, "message.edit").await, 0);
}

#[tokio::test]
async fn empty_edit_is_refused() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, msg) = task_with_queued_message(&pool).await;

    let err = edit_queued_message(&pool, task.id, msg.id, "  \n ", human()).await;

    assert!(matches!(err, Err(DbError::InvalidData(_))));
    let stored = list_undelivered_messages(&pool, task.id).await.unwrap();
    assert_eq!(prompt_text(&stored[0]), "first");
}

#[tokio::test]
async fn assignment_prompt_cannot_be_removed() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let key: ProjectKey = "DF".parse().unwrap();
    let (project_id, persona_id) = seed_project_and_persona(&pool, &key, "Linus")
        .await
        .unwrap();
    let (task, assignment) = create_task(&pool, new_task(project_id, persona_id, Some("T")))
        .await
        .unwrap();

    let err = remove_queued_message(&pool, task.id, assignment.id, human()).await;

    assert!(matches!(err, Err(DbError::Conflict(_))));
    assert_eq!(
        list_undelivered_messages(&pool, task.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn remove_deletes_the_message_and_audits() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, msg) = task_with_queued_message(&pool).await;
    queue_message(&pool, task.id, human(), text("second"))
        .await
        .unwrap();

    remove_queued_message(&pool, task.id, msg.id, human())
        .await
        .unwrap();

    let left = list_undelivered_messages(&pool, task.id).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(prompt_text(&left[0]), "second");
    assert_eq!(audit_count(&pool, "message.remove").await, 1);
    // Other messages still queued.
    assert_eq!(status_of(&pool, task.id).await, TaskStatus::Queued);
}

#[tokio::test]
async fn removing_the_last_message_returns_the_task_to_waiting() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, msg) = task_with_queued_message(&pool).await;
    assert_eq!(status_of(&pool, task.id).await, TaskStatus::Queued);

    remove_queued_message(&pool, task.id, msg.id, human())
        .await
        .unwrap();

    assert_eq!(status_of(&pool, task.id).await, TaskStatus::Waiting);
}

#[tokio::test]
async fn removing_a_message_while_another_is_queued_keeps_the_task_queued() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (task, msg) = task_with_queued_message(&pool).await;
    // A concurrent `queue_message`: it holds the task row lock, its message not committed yet.
    let mut queueing = pool.begin().await.unwrap();
    sqlx::query("UPDATE tasks SET updated_at = now() WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&mut *queueing)
        .await
        .unwrap();
    sqlx::query("INSERT INTO task_messages (id, task_id, author, content) VALUES ($1, $2, $3, $4)")
        .bind(PromptId::new().as_uuid())
        .bind(task.id.as_uuid())
        .bind(serde_json::to_value(human()).unwrap())
        .bind(serde_json::to_value(text("second")).unwrap())
        .execute(&mut *queueing)
        .await
        .unwrap();

    let remove = tokio::spawn({
        let pool = pool.clone();
        async move { remove_queued_message(&pool, task.id, msg.id, human()).await }
    });
    // Wait until the removal waits for the row lock, then let the queueing commit.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity
                     WHERE datname = current_database() AND wait_event_type = 'Lock'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiting > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    queueing.commit().await.unwrap();
    remove.await.unwrap().unwrap();

    assert_eq!(status_of(&pool, task.id).await, TaskStatus::Queued);
}

#[tokio::test]
async fn sql_and_rust_agree_on_which_messages_are_editable() {
    // Edit and remove each get fresh rows, so they cannot interfere.
    for remove in [false, true] {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, held) = task_with_queued_message(&pool).await;
        let forwarded = queue_message(&pool, task.id, human(), text("forwarded"))
            .await
            .unwrap();
        sqlx::query("UPDATE task_messages SET forwarded_at = now() WHERE id = $1")
            .bind(forwarded.id.as_uuid())
            .execute(&pool)
            .await
            .unwrap();
        let delivered = queue_message(&pool, task.id, human(), text("delivered"))
            .await
            .unwrap();
        let run = insert_run(&pool, task.id).await;
        mark_prompt_delivered(&pool, delivered.id, run)
            .await
            .unwrap();
        let system = queue_message(&pool, task.id, Principal::system("x"), text("system"))
            .await
            .unwrap();

        for (message, editable) in [
            (held, true),
            (forwarded, false),
            (delivered, false),
            (system, false),
        ] {
            let row: TaskMessageRow = sqlx::query_as!(
                MessageRecord,
                r#"SELECT id, task_id, author, content, queued_at, delivered_run_id,
                              delivered_at, forwarded_at
                       FROM task_messages WHERE id = $1"#,
                message.id.as_uuid()
            )
            .fetch_one(&pool)
            .await
            .unwrap()
            .try_into()
            .unwrap();
            assert_eq!(row.is_editable(), editable, "rust: {row:?}");
            let result = if remove {
                remove_queued_message(&pool, task.id, message.id, human()).await
            } else {
                edit_queued_message(&pool, task.id, message.id, "x", human()).await
            };
            assert_eq!(
                result.is_ok(),
                editable,
                "sql (remove: {remove}): {message:?}"
            );
        }
    }
}
