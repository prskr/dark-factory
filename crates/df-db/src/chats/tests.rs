//! Tests of the chat queries.

use df_core::{PersonaId, ProjectId, ProjectKey, TaskKind, TaskStatus, UserId};
use sqlx::PgPool;

use super::*;
use crate::requests::{Raise, raise_question};
use crate::task_events::set_status_for_test;
use crate::tasks::tests::{audit_count, human, insert_run, new_task, seed_project_and_persona};
use crate::tasks::{
    TaskMessageRow, cancel_task, create_task, get_task, list_undelivered_messages, mark_task_done,
    queue_message,
};

async fn seeded(pool: &PgPool) -> (ProjectId, PersonaId) {
    let key: ProjectKey = "DF".parse().unwrap();
    seed_project_and_persona(pool, &key, "Grace").await.unwrap()
}

fn chat_input(project_id: ProjectId, persona_id: PersonaId, text: &str) -> NewChat {
    NewChat {
        project_id,
        persona_id,
        author: Principal::human(UserId::new(), "Ada"),
        text: text.to_owned(),
    }
}

/// An idle chat: its first message is delivered and it is `waiting`.
async fn waiting_chat(pool: &PgPool) -> TaskRow {
    let (project_id, persona_id) = seeded(pool).await;
    let (chat, first) = create_chat(pool, chat_input(project_id, persona_id, "Plan it"))
        .await
        .unwrap();
    let run = insert_run(pool, chat.id).await;
    crate::tasks::mark_prompt_delivered(pool, chat.id, first.id, run)
        .await
        .unwrap();
    set_status_for_test(pool, chat.id, TaskStatus::Waiting).await;
    chat
}

fn plain(message: &TaskMessageRow) -> String {
    message
        .content
        .iter()
        .filter(|block| !block.hidden)
        .filter_map(|block| match &block.kind {
            Some(df_proto::transcript::v1::content_block::Kind::Text(t)) => Some(t.text.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn chat_create_starts_a_queued_chat_with_the_message_of_the_board_member() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project_id, persona_id) = seeded(&pool).await;

    let (chat, first) = create_chat(
        &pool,
        chat_input(project_id, persona_id, "Plan the Helm chart release"),
    )
    .await
    .unwrap();

    assert_eq!(chat.key.to_string(), "DF-1-CHAT");
    assert_eq!(chat.kind(), TaskKind::Chat);
    assert_eq!(chat.persona_id, persona_id);
    assert_eq!(chat.parent_id, None);
    assert_eq!(chat.status, TaskStatus::Queued);
    assert_eq!(chat.title, "Plan the Helm chart release");
    assert!(!chat.on_existing_branch());
    assert!(
        matches!(&first.author, Principal::Human { display_name, .. } if display_name == "Ada")
    );
    assert_eq!(plain(&first), "Plan the Helm chart release");
    assert!(
        first.content.iter().any(|block| block.hidden),
        "title block"
    );
    let kind: String = sqlx::query_scalar("SELECT kind FROM tasks WHERE id = $1")
        .bind(chat.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kind, "chat");
}

#[tokio::test]
async fn chat_create_allows_several_chats_with_one_persona() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project_id, persona_id) = seeded(&pool).await;

    let (first, _) = create_chat(&pool, chat_input(project_id, persona_id, "One"))
        .await
        .unwrap();
    let (second, _) = create_chat(&pool, chat_input(project_id, persona_id, "Two"))
        .await
        .unwrap();

    assert_ne!(first.id, second.id);
    assert_eq!(second.key.to_string(), "DF-2-CHAT");
}

#[tokio::test]
async fn a_chat_has_no_branch_and_is_no_base_task() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project_id, persona_id) = seeded(&pool).await;
    let (chat, _) = create_chat(&pool, chat_input(project_id, persona_id, "Hello"))
        .await
        .unwrap();
    assert_eq!(chat.branch, "");

    let placement = TaskPlacement {
        base_task: Some(chat.key.clone()),
        ..TaskPlacement::default()
    };
    let error = crate::create_task_on(
        &pool,
        new_task(project_id, persona_id, Some("T")),
        placement,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, DbError::InvalidBaseTask(_)), "{error}");
}

#[tokio::test]
async fn an_archived_chat_claims_no_held_back_message_until_it_is_unarchived() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let chat = waiting_chat(&pool).await;
    archive_chat(&pool, chat.id, human()).await.unwrap();
    queue_message(
        &pool,
        chat.id,
        Principal::system("task.child_ended"),
        crate::tasks::tests::text("DF-20 is done"),
    )
    .await
    .unwrap();

    assert!(
        crate::claim_held_back_messages(&pool, chat.id)
            .await
            .unwrap()
            .is_empty()
    );

    unarchive_chat(&pool, chat.id, human()).await.unwrap();
    assert_eq!(
        crate::claim_held_back_messages(&pool, chat.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn chat_create_rejects_an_empty_first_message() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project_id, persona_id) = seeded(&pool).await;

    let error = create_chat(&pool, chat_input(project_id, persona_id, "  \n "))
        .await
        .unwrap_err();

    assert!(matches!(error, DbError::InvalidData(_)), "{error}");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn chat_create_writes_an_audit_entry_naming_board_member_project_persona_and_key() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project_id, persona_id) = seeded(&pool).await;

    let (chat, _) = create_chat(&pool, chat_input(project_id, persona_id, "Hello"))
        .await
        .unwrap();

    let (actor, details): (serde_json::Value, serde_json::Value) = sqlx::query_as(
        "SELECT actor, details FROM audit_log WHERE action = 'chat.create' AND resource_id = $1",
    )
    .bind(chat.id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(actor["display_name"], "Ada");
    assert_eq!(details["key"], "DF-1-CHAT");
    assert_eq!(details["project"], "DF");
    assert_eq!(details["persona"], "Grace");
}

#[tokio::test]
async fn chat_create_shares_the_number_counter_with_tasks() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project_id, persona_id) = seeded(&pool).await;

    let (task, _) = create_task(&pool, new_task(project_id, persona_id, Some("T")))
        .await
        .unwrap();
    let (chat, _) = create_chat(&pool, chat_input(project_id, persona_id, "Hello"))
        .await
        .unwrap();
    let (next, _) = create_task(&pool, new_task(project_id, persona_id, Some("U")))
        .await
        .unwrap();

    assert_eq!(task.key.to_string(), "DF-1");
    assert_eq!(chat.key.to_string(), "DF-2-CHAT");
    assert_eq!(next.key.to_string(), "DF-3");
    assert_eq!(next.kind(), TaskKind::Task);
}

#[tokio::test]
async fn chat_schema_rejects_a_task_with_a_chat_key_and_a_chat_without_one() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project_id, persona_id) = seeded(&pool).await;
    let insert = |key: &'static str, kind: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO tasks (id, project_id, number, key, title, persona_id, kind, created_by,
                    title_source)
                 VALUES (gen_random_uuid(), $1, 7, $2, 't', $3, $4, '{}'::jsonb, 'user')",
            )
            .bind(project_id.as_uuid())
            .bind(key)
            .bind(persona_id.as_uuid())
            .bind(kind)
            .execute(&pool)
            .await
        }
    };
    let violated = |error: sqlx::Error| {
        error
            .as_database_error()
            .and_then(|e| e.constraint().map(str::to_owned))
    };

    let task_with_chat_key = insert("DF-7-CHAT", "task").await.unwrap_err();
    assert_eq!(
        violated(task_with_chat_key).as_deref(),
        Some("tasks_chat_key_check")
    );
    let chat_without_suffix = insert("DF-7", "chat").await.unwrap_err();
    assert_eq!(
        violated(chat_without_suffix).as_deref(),
        Some("tasks_chat_key_check")
    );
    let numbered_suffix = insert("DF-7-CHAT-1", "task").await.unwrap_err();
    assert_eq!(
        violated(numbered_suffix).as_deref(),
        Some("tasks_key_check")
    );
    insert("DF-7-CHAT", "chat").await.unwrap();
}

#[tokio::test]
async fn chat_schema_allows_archiving_chats_only() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project_id, persona_id) = seeded(&pool).await;
    let (task, _) = create_task(&pool, new_task(project_id, persona_id, Some("T")))
        .await
        .unwrap();

    let result = sqlx::query(
        "UPDATE tasks SET archived_at = now(), archived_by = '{}'::jsonb WHERE id = $1",
    )
    .bind(task.id.as_uuid())
    .execute(&pool)
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn chat_archive_keeps_the_status_and_records_who_and_when() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let chat = waiting_chat(&pool).await;

    archive_chat(&pool, chat.id, human()).await.unwrap();

    let archived = get_task(&pool, chat.id).await.unwrap().unwrap();
    assert!(archived.archived_at.is_some());
    assert_eq!(archived.status, TaskStatus::Waiting);
    let by: serde_json::Value = sqlx::query_scalar("SELECT archived_by FROM tasks WHERE id = $1")
        .bind(chat.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(by["display_name"], "Ada");
    assert_eq!(audit_count(&pool, "chat.archive").await, 1);
}

#[tokio::test]
async fn chat_archive_is_rejected_while_the_chat_runs_or_has_an_open_request() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let chat = waiting_chat(&pool).await;
    set_status_for_test(&pool, chat.id, TaskStatus::Running).await;
    let error = archive_chat(&pool, chat.id, human()).await.unwrap_err();
    assert!(matches!(error, DbError::Conflict(_)), "{error}");

    set_status_for_test(&pool, chat.id, TaskStatus::Waiting).await;
    let agent = Principal::system("test");
    let from = Raise {
        task_id: chat.id,
        run_id: None,
        actor: &agent,
        blocking: false,
    };
    raise_question(&pool, from, "Which one?", &[])
        .await
        .unwrap();
    let error = archive_chat(&pool, chat.id, human()).await.unwrap_err();
    assert!(error.to_string().contains("open request"), "{error}");

    let after = get_task(&pool, chat.id).await.unwrap().unwrap();
    assert_eq!(after.archived_at, None);
    assert_eq!(audit_count(&pool, "chat.archive").await, 0);
}

#[tokio::test]
async fn chat_archive_leaves_a_running_subtask_alone() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let chat = waiting_chat(&pool).await;
    let mut input = new_task(chat.project_id, chat.persona_id, Some("Sub"));
    input.parent_id = Some(chat.id);
    let (subtask, _) = create_task(&pool, input).await.unwrap();
    set_status_for_test(&pool, subtask.id, TaskStatus::Running).await;

    archive_chat(&pool, chat.id, human()).await.unwrap();

    let subtask = get_task(&pool, subtask.id).await.unwrap().unwrap();
    assert_eq!(subtask.status, TaskStatus::Running);
}

#[tokio::test]
async fn chat_archive_is_rejected_for_a_task() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project_id, persona_id) = seeded(&pool).await;
    let (task, _) = create_task(&pool, new_task(project_id, persona_id, Some("T")))
        .await
        .unwrap();
    set_status_for_test(&pool, task.id, TaskStatus::Waiting).await;

    let error = archive_chat(&pool, task.id, human()).await.unwrap_err();

    assert!(matches!(error, DbError::Conflict(_)), "{error}");
}

#[tokio::test]
async fn chat_archive_still_takes_the_child_ended_message_of_a_subtask() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let chat = waiting_chat(&pool).await;
    let mut input = new_task(chat.project_id, chat.persona_id, Some("Sub"));
    input.parent_id = Some(chat.id);
    let (subtask, _) = create_task(&pool, input).await.unwrap();
    set_status_for_test(&pool, subtask.id, TaskStatus::Waiting).await;
    archive_chat(&pool, chat.id, human()).await.unwrap();

    mark_task_done(&pool, subtask.id, human()).await.unwrap();

    let after = get_task(&pool, chat.id).await.unwrap().unwrap();
    assert!(after.archived_at.is_some());
    let queued = list_undelivered_messages(&pool, chat.id).await.unwrap();
    assert_eq!(queued.len(), 1);
}

#[tokio::test]
async fn chat_archive_unarchive_without_messages_keeps_the_status() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let chat = waiting_chat(&pool).await;
    archive_chat(&pool, chat.id, human()).await.unwrap();

    unarchive_chat(&pool, chat.id, human()).await.unwrap();

    let after = get_task(&pool, chat.id).await.unwrap().unwrap();
    assert_eq!(after.archived_at, None);
    assert_eq!(after.status, TaskStatus::Waiting);
    assert_eq!(audit_count(&pool, "chat.unarchive").await, 1);
}

#[tokio::test]
async fn chat_archive_unarchive_with_a_queued_message_queues_the_chat() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let chat = waiting_chat(&pool).await;
    archive_chat(&pool, chat.id, human()).await.unwrap();
    // A subtask ends while the chat is archived; its message waits and the chat stays archived.
    let system = Principal::system("task.child_ended");
    queue_message(
        &pool,
        chat.id,
        system,
        crate::tasks::tests::text("DF-20 is done"),
    )
    .await
    .unwrap();
    assert!(
        get_task(&pool, chat.id)
            .await
            .unwrap()
            .unwrap()
            .archived_at
            .is_some()
    );

    unarchive_chat(&pool, chat.id, human()).await.unwrap();

    let after = get_task(&pool, chat.id).await.unwrap().unwrap();
    assert_eq!(after.archived_at, None);
    assert_eq!(after.status, TaskStatus::Queued);
}

#[tokio::test]
async fn chat_archive_unarchive_is_rejected_for_a_chat_that_is_not_archived() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let chat = waiting_chat(&pool).await;

    let error = unarchive_chat(&pool, chat.id, human()).await.unwrap_err();

    assert!(matches!(error, DbError::Conflict(_)), "{error}");
}

#[tokio::test]
async fn a_chat_is_neither_marked_done_nor_cancelled_and_nothing_is_audited() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let chat = waiting_chat(&pool).await;

    let done = mark_task_done(&pool, chat.id, human()).await.unwrap_err();
    let cancelled = cancel_task(&pool, chat.id, human()).await.unwrap_err();

    assert!(matches!(done, DbError::Conflict(_)), "{done}");
    assert!(matches!(cancelled, DbError::Conflict(_)), "{cancelled}");
    let after = get_task(&pool, chat.id).await.unwrap().unwrap();
    assert_eq!(after.status, TaskStatus::Waiting);
    assert_eq!(audit_count(&pool, "task.done").await, 0);
    assert_eq!(audit_count(&pool, "task.cancel").await, 0);
}

#[tokio::test]
async fn a_message_reopens_a_failed_chat_while_another_chat_of_the_project_is_open() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let (project_id, persona_id) = seeded(&pool).await;
    let (failed, _) = create_chat(&pool, chat_input(project_id, persona_id, "One"))
        .await
        .unwrap();
    create_chat(&pool, chat_input(project_id, persona_id, "Two"))
        .await
        .unwrap();
    set_status_for_test(&pool, failed.id, TaskStatus::Failed).await;

    queue_message(
        &pool,
        failed.id,
        human(),
        crate::tasks::tests::text("Again"),
    )
    .await
    .unwrap();

    let after = get_task(&pool, failed.id).await.unwrap().unwrap();
    assert_eq!(after.status, TaskStatus::Queued);
}
