//! Read models of the chat pages (`specs/chat-ui/spec.md`): the projects with their chats, the
//! chat list with its order, and the archive with its search.

use df_core::{TaskStatus, TitleSource};
use jiff::Timestamp;
use sqlx::PgPool;
use uuid::Uuid;

use crate::pool::DbError;
use crate::requests::RequestState;
use crate::transcript::{KIND_AGENT_MESSAGE_CHUNK, KIND_USER_MESSAGE};

/// A project of the chat page with the number of its chats that are not archived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatProject {
    pub key: String,
    pub name: String,
    pub chats: i64,
    /// The number of its archived chats.
    pub archived: i64,
    /// One of its chats needs a board member.
    pub needs_you: bool,
}

/// A chat of a project, a row of the chat list or of the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatListItem {
    pub key: String,
    pub title: String,
    pub title_source: TitleSource,
    pub status: TaskStatus,
    pub persona_id: Uuid,
    pub persona_name: String,
    /// The latest transcript event or message, or the creation.
    pub last_activity_at: Timestamp,
    /// The latest message of the board member or the agent, as plain text.
    pub last_message: String,
    /// The chat has an open approval request.
    pub needs_you: bool,
    pub artifacts: i64,
    pub archived_at: Option<Timestamp>,
}

struct ChatRecord {
    key: String,
    title: String,
    title_source: String,
    status: String,
    persona_id: Uuid,
    persona_name: String,
    last_activity_at: Timestamp,
    last_message: String,
    needs_you: bool,
    artifacts: i64,
    archived_at: Option<jiff_sqlx::Timestamp>,
}

impl TryFrom<ChatRecord> for ChatListItem {
    type Error = DbError;

    fn try_from(row: ChatRecord) -> Result<Self, DbError> {
        Ok(Self {
            key: row.key,
            title: row.title,
            title_source: row.title_source.parse()?,
            status: row.status.parse()?,
            persona_id: row.persona_id,
            persona_name: row.persona_name,
            last_activity_at: row.last_activity_at,
            last_message: row.last_message,
            needs_you: row.needs_you,
            artifacts: row.artifacts,
            archived_at: row.archived_at.map(Into::into),
        })
    }
}

/// Every project with its chats that are not archived, by name.
pub async fn list_chat_projects(pool: &PgPool) -> Result<Vec<ChatProject>, DbError> {
    Ok(sqlx::query_as!(
        ChatProject,
        r#"
        SELECT p.key AS "key!", p.name AS "name!",
               count(t.id) FILTER (WHERE t.archived_at IS NULL) AS "chats!",
               count(t.id) FILTER (WHERE t.archived_at IS NOT NULL) AS "archived!",
               COALESCE(bool_or(t.archived_at IS NULL AND EXISTS (
                   SELECT 1 FROM agent_requests r WHERE r.task_id = t.id AND r.state = $1)),
                   false) AS "needs_you!"
        FROM projects p
        LEFT JOIN tasks t ON t.project_id = p.id AND t.kind = 'chat'
        GROUP BY p.id
        ORDER BY p.name, p.key
        "#,
        RequestState::Pending.as_str()
    )
    .fetch_all(pool)
    .await?)
}

/// The project of the chat that was active last, if there is a chat.
pub async fn latest_chat_project(pool: &PgPool) -> Result<Option<String>, DbError> {
    Ok(sqlx::query_scalar!(
        r#"
        SELECT p.key AS "key!" FROM tasks t JOIN projects p ON p.id = t.project_id
        WHERE t.kind = 'chat'
        ORDER BY task_last_activity(t.id, t.created_at) DESC
        LIMIT 1
        "#
    )
    .fetch_optional(pool)
    .await?)
}

/// The chats of project `project_key`: the ones that are not archived (`archived` false), the
/// ones that need a board member first, then by last activity, newest first; or the archived ones
/// (`archived` true), most recently archived first, narrowed to those whose topic, persona or
/// messages contain `search` (case-insensitive).
pub async fn list_chats(
    pool: &PgPool,
    project_key: &str,
    archived: bool,
    search: Option<&str>,
) -> Result<Vec<ChatListItem>, DbError> {
    let pattern = search
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(like_pattern);
    sqlx::query_as!(
        ChatRecord,
        r#"
        SELECT t.key, t.title, t.title_source, t.status, ps.id AS persona_id,
               ps.name AS persona_name,
               task_last_activity(t.id, t.created_at) AS "last_activity_at!",
               COALESCE((
                   SELECT CASE e.kind
                       WHEN $4 THEN (
                           SELECT string_agg(b.block #>> '{text,text}', E'\n' ORDER BY b.ord)
                           FROM jsonb_array_elements(e.payload #> '{userMessage,content}')
                                WITH ORDINALITY AS b(block, ord)
                           WHERE COALESCE((b.block ->> 'hidden')::boolean, false) = false)
                       ELSE (
                           SELECT string_agg(c.payload #>> '{agentMessageChunk,content,text,text}',
                                             '' ORDER BY c.id)
                           FROM transcript_events c
                           WHERE c.task_id = t.id AND c.kind = $5
                             AND c.payload #>> '{agentMessageChunk,messageId}'
                                 = e.payload #>> '{agentMessageChunk,messageId}')
                   END
                   FROM transcript_events e
                   WHERE e.task_id = t.id AND e.kind IN ($4, $5)
                   ORDER BY e.id DESC LIMIT 1), '') AS "last_message!",
               EXISTS (SELECT 1 FROM agent_requests r WHERE r.task_id = t.id AND r.state = $3)
                   AS "needs_you!",
               (SELECT count(*) FROM task_artifacts a WHERE a.task_id = t.id) AS "artifacts!",
               t.archived_at
        FROM tasks t
        JOIN projects p ON p.id = t.project_id
        JOIN personas ps ON ps.id = t.persona_id
        WHERE t.kind = 'chat' AND p.key = $1 AND (t.archived_at IS NOT NULL) = $2
          AND ($6::text IS NULL
               OR t.title ILIKE $6 OR ps.name ILIKE $6
               -- Agent text arrives in chunks: search the joined text, not one chunk.
               OR (SELECT string_agg(
                          COALESCE(s.payload #>> '{agentMessageChunk,content,text,text}', ''),
                          '' ORDER BY s.id)
                   FROM transcript_events s WHERE s.task_id = t.id AND s.kind = $5) ILIKE $6
               OR EXISTS (
                   SELECT 1 FROM transcript_events s
                   WHERE s.task_id = t.id AND s.kind = $4
                     AND EXISTS (
                         SELECT 1 FROM jsonb_path_query(
                             s.payload, '$.userMessage.content[*].text.text') AS v
                         WHERE (v #>> '{}') ILIKE $6)))
        ORDER BY CASE WHEN t.archived_at IS NULL THEN EXISTS (
                     SELECT 1 FROM agent_requests r WHERE r.task_id = t.id AND r.state = $3)
                 END DESC NULLS LAST,
                 CASE WHEN t.archived_at IS NULL THEN task_last_activity(t.id, t.created_at) END
                     DESC NULLS LAST,
                 t.archived_at DESC NULLS LAST,
                 t.number DESC
        "#,
        project_key,
        archived,
        RequestState::Pending.as_str(),
        KIND_USER_MESSAGE,
        KIND_AGENT_MESSAGE_CHUNK,
        pattern
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(ChatListItem::try_from)
    .collect()
}

/// `%text%` for `ILIKE`, with the wildcards of `text` escaped.
fn like_pattern(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() + 2);
    pattern.push('%');
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

#[cfg(test)]
mod tests;
