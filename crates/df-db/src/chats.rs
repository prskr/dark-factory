//! Chats: tasks of kind `chat` (`specs/chats/spec.md`). A chat is created with the first message
//! of a board member, and archived and unarchived as the `Archived` and `Unarchived` task events.

use df_core::task_transition::{TaskEvent, TaskSnapshot, transition};
use df_core::{PersonaId, Principal, ProjectId, TaskId, TaskKind};
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};

use crate::audit::{NewAuditEntry, write_audit_entry};
use crate::pool::DbError;
use crate::requests::close_pending_questions;
use crate::task_events::{apply_task_event, execute_effects};
use crate::tasks::{
    NewTask, TaskMessageRow, TaskPlacement, TaskRow, allocate_task_key, ask_for_title,
    insert_message, insert_task, text_block,
};

/// A chat a board member starts.
#[derive(Debug, Clone)]
pub struct NewChat {
    pub project_id: ProjectId,
    pub persona_id: PersonaId,
    /// The board member who starts the chat and writes its first message.
    pub author: Principal,
    pub text: String,
}

/// Starts a chat in a transaction of its own: see [`create_chat_tx`].
pub async fn create_chat(
    pool: &PgPool,
    input: NewChat,
) -> Result<(TaskRow, TaskMessageRow), DbError> {
    let mut tx = pool.begin().await?;
    let created = create_chat_tx(&mut tx, input).await?;
    tx.commit().await?;
    Ok(created)
}

/// Creates a chat with the next number of the project (`<KEY>-<n>-CHAT`), a placeholder topic
/// derived from the first message, and queues that message from the board member followed by
/// the hidden request for a topic. The chat becomes `queued`. Audited as `chat.create`.
///
/// An empty message is [`DbError::InvalidData`] and creates nothing.
pub async fn create_chat_tx(
    tx: &mut Transaction<'_, Postgres>,
    input: NewChat,
) -> Result<(TaskRow, TaskMessageRow), DbError> {
    let text = input.text.trim().to_owned();
    if text.is_empty() {
        return Err(DbError::InvalidData("the first message is empty".into()));
    }
    let (key, _) = allocate_task_key(tx, input.project_id, TaskKind::Chat).await?;
    let snapshot = TaskSnapshot {
        kind: TaskKind::Chat,
        ..TaskSnapshot::new_task()
    };
    let created = transition(&snapshot, TaskEvent::Created)
        .map_err(|rejected| DbError::Conflict(rejected.to_string()))?;
    let new_task = NewTask {
        project_id: input.project_id,
        persona_id: input.persona_id,
        title: None,
        description: text.clone(),
        issue_number: None,
        created_by: input.author.clone(),
        parent_id: None,
        depends_on: Vec::new(),
    };
    let row = insert_task(
        tx,
        &new_task,
        &TaskPlacement::default(),
        key,
        None,
        created.status,
        None,
    )
    .await?;
    let message = insert_first_message(tx, &row, &input.author, text).await?;
    let followups = execute_effects(tx, row.id, &created, &input.author).await?;
    debug_assert!(followups.is_empty(), "Created yields no follow-up work");

    let persona = crate::catalog::get_persona(&mut **tx, input.persona_id.as_uuid())
        .await?
        .ok_or_else(|| DbError::NotFound(format!("persona {}", input.persona_id)))?
        .display_name;
    let entry = NewAuditEntry::new(
        input.author,
        Vec::new(),
        "chat.create",
        "task",
        row.id.to_string(),
    )
    .with_details(&json!({
        "key": row.key,
        "project": row.key.project(),
        "persona": persona,
    }))?;
    write_audit_entry(&mut **tx, &entry).await?;
    Ok((row, message))
}

async fn insert_first_message(
    tx: &mut Transaction<'_, Postgres>,
    chat: &TaskRow,
    author: &Principal,
    text: String,
) -> Result<TaskMessageRow, DbError> {
    let content = vec![text_block(text, false), text_block(ask_for_title(), true)];
    insert_message(tx, chat.id, author.clone(), content, None).await
}

/// A board member archives a chat: the chat keeps its status and records when and by whom, and
/// its pending questions are closed with reason `chat-archived`. A chat that is `queued` or
/// `running` or has a pending approval request is [`DbError::Conflict`] (nothing is closed), as
/// is a task or an archived chat.
pub async fn archive_chat(pool: &PgPool, chat: TaskId, actor: Principal) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    close_pending_questions(&mut tx, chat, "chat-archived", &actor).await?;
    apply_task_event(
        &mut tx,
        chat,
        TaskEvent::Archived { by: actor.clone() },
        &actor,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// A board member unarchives a chat. It becomes `queued` when messages wait for it, so a run
/// delivers them; otherwise it keeps its status.
pub async fn unarchive_chat(pool: &PgPool, chat: TaskId, actor: Principal) -> Result<(), DbError> {
    let mut tx = pool.begin().await?;
    apply_task_event(&mut tx, chat, TaskEvent::Unarchived, &actor).await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests;
