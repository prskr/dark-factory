//! Read models of the web UI: task list, task detail, runs with token usage (from the gateway's
//! usage records) and the transcript.

use df_core::{
    DependencyKind, Principal, RunState, StatusSet, TaskId, TaskKey, TaskSort, TaskStatus,
    TitleSource, is_satisfied,
};
use df_proto::transcript::v1::Event;
use jiff::Timestamp;
use sqlx::PgPool;
use uuid::Uuid;

use crate::pool::DbError;
use crate::tasks::PR_SYNC_KIND;

/// How far the direct subtasks of a task are (`task-ui`, "Subtask progress on task rows"): done
/// counts those `in_review` or `completed`, total those that are not `cancelled`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubtaskProgress {
    pub done: i64,
    pub total: i64,
}

impl SubtaskProgress {
    /// The progress of subtasks in these statuses. The list queries count the same way in SQL
    /// (`TaskStatus::SUBTASK_DONE`, `cancelled` left out of the total).
    pub fn of(statuses: impl IntoIterator<Item = TaskStatus>) -> Self {
        statuses
            .into_iter()
            .fold(Self::default(), |mut progress, status| {
                progress.done += i64::from(TaskStatus::SUBTASK_DONE.contains(&status));
                progress.total += i64::from(status != TaskStatus::Cancelled);
                progress
            })
    }
}

/// One row of the task list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskListItem {
    pub id: Uuid,
    pub key: TaskKey,
    pub title: String,
    /// `Placeholder` while the title is provisional.
    pub title_source: TitleSource,
    pub project_key: String,
    pub project_name: String,
    pub persona_id: Uuid,
    pub persona_handle: String,
    /// The persona's display name.
    pub persona_display_name: String,
    pub status: String,
    pub status_line: String,
    /// When the scheduler may start the task's next run.
    pub resume_at: Option<Timestamp>,
    /// Progress of the direct subtasks; total 0 for a task without subtasks.
    pub subtasks: SubtaskProgress,
    /// The task has direct subtasks, cancelled ones included (`subtasks.total` leaves them out).
    pub has_subtasks: bool,
    /// The task itself needs a board member (`task-ui`, "Needs you").
    pub needs_you: bool,
    /// How many tasks below this one, at any depth, need a board member.
    pub needs_you_below: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// A row of the task list as `query_as!` reads it.
struct TaskListRow {
    id: Uuid,
    key: String,
    title: String,
    title_source: String,
    project_key: String,
    project_name: String,
    persona_id: Uuid,
    persona_handle: String,
    persona_display_name: String,
    status: String,
    status_line: String,
    resume_at: Option<jiff_sqlx::Timestamp>,
    subtasks_done: i64,
    subtasks_total: i64,
    has_subtasks: bool,
    needs_you: bool,
    needs_you_below: i64,
    created_at: Timestamp,
    updated_at: Timestamp,
}

impl TryFrom<TaskListRow> for TaskListItem {
    type Error = DbError;

    fn try_from(row: TaskListRow) -> Result<Self, DbError> {
        Ok(Self {
            id: row.id,
            key: row.key.parse()?,
            title: row.title,
            title_source: row.title_source.parse()?,
            project_key: row.project_key,
            project_name: row.project_name,
            persona_id: row.persona_id,
            persona_handle: row.persona_handle,
            persona_display_name: row.persona_display_name,
            status: row.status,
            status_line: row.status_line,
            resume_at: row.resume_at.map(Into::into),
            subtasks: SubtaskProgress {
                done: row.subtasks_done,
                total: row.subtasks_total,
            },
            has_subtasks: row.has_subtasks,
            needs_you: row.needs_you,
            needs_you_below: row.needs_you_below,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// Which level of the task tree the list shows (`task-list-tree` design D2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListScope {
    /// Tasks without a parent task: a task created in a chat counts as top level.
    TopLevel,
    /// The direct subtasks of a task, whatever their status or project.
    SubtasksOf(TaskId),
    /// The task with this key, which a scoped list shows first; nothing for an unknown key.
    Task(TaskKey),
}

/// Which tasks the list shows; the default is all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskFilter {
    /// Only tasks whose status is in this set. Ignored for [`ListScope::SubtasksOf`] and
    /// [`ListScope::Task`].
    pub group: Option<StatusSet>,
    /// Only tasks of the project with this key. Ignored like `group`.
    pub project_key: Option<String>,
    /// The tree level; `None` lists every task, subtasks included (the link-only status filters).
    pub scope: Option<ListScope>,
    /// The order of the rows.
    pub sort: TaskSort,
}

/// What a [`TaskFilter`] binds in the list query.
#[derive(Default)]
struct Selection<'a> {
    statuses: Option<Vec<String>>,
    project_key: Option<&'a str>,
    subtasks_of: Option<Uuid>,
    top_level: bool,
    key: Option<String>,
    /// Active also keeps a final top-level task while a task below it needs you.
    keeps_finished: bool,
}

impl TaskFilter {
    /// The arguments of the query. A scope that picks tasks by their place in the tree
    /// ([`ListScope::SubtasksOf`], [`ListScope::Task`]) ignores the status group and the project.
    fn selection(&self) -> Selection<'_> {
        let by_filters = || Selection {
            statuses: self.group.map(statuses_of),
            project_key: self.project_key.as_deref(),
            ..Selection::default()
        };
        match &self.scope {
            Some(ListScope::SubtasksOf(parent)) => Selection {
                subtasks_of: Some(parent.as_uuid()),
                ..Selection::default()
            },
            Some(ListScope::Task(key)) => Selection {
                key: Some(key.to_string()),
                ..Selection::default()
            },
            Some(ListScope::TopLevel) => Selection {
                top_level: true,
                keeps_finished: self.group == Some(StatusSet::Active),
                ..by_filters()
            },
            None => by_filters(),
        }
    }
}

/// A project of the project filter with its number of tasks in the current status group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFilterItem {
    pub key: String,
    pub name: String,
    pub task_count: i64,
}

/// A task with the names and links the detail page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDetail {
    pub id: Uuid,
    pub key: String,
    pub title: String,
    pub title_source: TitleSource,
    pub description: String,
    pub project_key: String,
    pub project_name: String,
    pub repo_url: String,
    pub issue_tracker_url: Option<String>,
    pub persona_id: Uuid,
    pub persona_handle: String,
    /// The persona's display name.
    pub persona_display_name: String,
    /// Harness of the persona, for example `claude-code`.
    pub persona_harness: String,
    /// Model of the persona; `None` means the instance default.
    pub persona_model: Option<String>,
    pub persona_description: String,
    pub issue_number: Option<i64>,
    pub status: TaskStatus,
    pub status_line: String,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub pr_title: Option<String>,
    /// The PR was opened outside the factory.
    pub pr_attached: bool,
    /// When a chat was archived.
    pub archived_at: Option<Timestamp>,
    /// The task has an approval request that is still pending.
    pub has_pending_approval: bool,
    /// The branch the task commits to.
    pub branch: String,
    pub resume_at: Option<Timestamp>,
    /// Consecutive automatic resumes after infrastructure failures.
    pub auto_resumes: i32,
    pub created_by: Principal,
    /// Key of the task whose agent created this one, if an agent did.
    pub creator_task_key: Option<String>,
    pub parent_key: Option<String>,
    pub parent_title: Option<String>,
    /// The persona of the parent, which a chat parent shows next to its key.
    pub parent_persona_display_name: Option<String>,
    /// The task whose branch this one builds on.
    pub base_task_key: Option<String>,
    pub base_task_title: Option<String>,
}

/// A task detail as `query_as!` reads it.
struct TaskDetailRow {
    id: Uuid,
    key: String,
    title: String,
    title_source: String,
    description: String,
    project_key: String,
    project_name: String,
    repo_url: String,
    issue_tracker_url: Option<String>,
    persona_id: Uuid,
    persona_handle: String,
    persona_display_name: String,
    persona_harness: String,
    persona_model: Option<String>,
    persona_description: String,
    issue_number: Option<i64>,
    status: String,
    status_line: String,
    pr_number: Option<i64>,
    pr_url: Option<String>,
    pr_title: Option<String>,
    pr_attached: bool,
    archived_at: Option<jiff_sqlx::Timestamp>,
    has_pending_approval: bool,
    branch: String,
    resume_at: Option<jiff_sqlx::Timestamp>,
    auto_resumes: i32,
    created_by: serde_json::Value,
    creator_task_key: Option<String>,
    parent_key: Option<String>,
    parent_title: Option<String>,
    parent_persona_display_name: Option<String>,
    base_task_key: Option<String>,
    base_task_title: Option<String>,
}

impl TryFrom<TaskDetailRow> for TaskDetail {
    type Error = DbError;

    fn try_from(row: TaskDetailRow) -> Result<Self, DbError> {
        Ok(Self {
            id: row.id,
            key: row.key,
            title: row.title,
            title_source: row.title_source.parse()?,
            description: row.description,
            project_key: row.project_key,
            project_name: row.project_name,
            repo_url: row.repo_url,
            issue_tracker_url: row.issue_tracker_url,
            persona_id: row.persona_id,
            persona_handle: row.persona_handle,
            persona_display_name: row.persona_display_name,
            persona_harness: row.persona_harness,
            persona_model: row.persona_model,
            persona_description: row.persona_description,
            issue_number: row.issue_number,
            status: row.status.parse()?,
            status_line: row.status_line,
            pr_number: row.pr_number,
            pr_url: row.pr_url,
            pr_title: row.pr_title,
            pr_attached: row.pr_attached,
            archived_at: row.archived_at.map(Into::into),
            has_pending_approval: row.has_pending_approval,
            branch: row.branch,
            resume_at: row.resume_at.map(Into::into),
            auto_resumes: row.auto_resumes,
            created_by: serde_json::from_value(row.created_by)?,
            creator_task_key: row.creator_task_key,
            parent_key: row.parent_key,
            parent_title: row.parent_title,
            parent_persona_display_name: row.parent_persona_display_name,
            base_task_key: row.base_task_key,
            base_task_title: row.base_task_title,
        })
    }
}

/// A task this task waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyView {
    pub key: String,
    pub title: String,
    pub kind: DependencyKind,
    pub satisfied: bool,
}

/// A task that waits for this task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependantView {
    pub key: String,
    pub title: String,
    pub kind: DependencyKind,
    pub status: TaskStatus,
}

/// The dependencies of a task and the tasks that depend on it, ordered by task number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dependencies {
    pub waits_for: Vec<DependencyView>,
    pub needed_by: Vec<DependantView>,
}

/// Token counts of one model within a run, summed over its requests with known usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelUsage {
    pub model_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

/// A run with its raw token counts per model and the number of requests without usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunView {
    pub id: Uuid,
    pub number: i64,
    pub state: RunState,
    pub outcome: Option<String>,
    pub detail: String,
    pub retry_after: Option<Timestamp>,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    pub usage: Vec<ModelUsage>,
    /// Model requests whose response carried no usage (unknown, not zero).
    pub unknown_usage_requests: i64,
    /// The calls of the run's agent to third-party MCP tools (and servers left out of a tool
    /// list), oldest first.
    pub mcp_calls: Vec<crate::mcp_servers::McpCallRecord>,
}

struct RunRow {
    id: Uuid,
    number: i64,
    state: String,
    outcome: Option<String>,
    detail: String,
    retry_after: Option<jiff_sqlx::Timestamp>,
    started_at: Timestamp,
    ended_at: Option<jiff_sqlx::Timestamp>,
}

struct RunUsageRow {
    run_id: Uuid,
    model_id: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
}

/// A stored transcript row; `event` is `None` if the payload cannot be decoded as v1.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvent {
    /// Task-wide, increasing id; the SSE event id.
    pub id: i64,
    pub event: Option<Event>,
    /// When the runtime recorded the event.
    pub occurred_at: Timestamp,
    /// Number of the run that produced the event and when that run started.
    pub run_number: i64,
    pub run_started_at: Timestamp,
}

/// The tasks matching `filter` in the order of `filter.sort`. Ties are ordered by task ID in the
/// direction of the sort, so rows with the same time keep their place between calls.
///
/// A task needs you (`needs_you`, `needs_you_below`; `task-ui` "Needs you") when it is in one of
/// `TaskStatus::MAY_NEED_YOU`, and, if `in_review`, is not a subtask without a pull request or pending
/// pull-request job, or its parent agent merges it. The pull-request part repeats the one of
/// `tasks::tree::open_subtasks`; a test runs both against the same tasks. The Active list also
/// keeps a top-level task in a final status while a task below it needs you.
pub async fn list_task_items(
    pool: &PgPool,
    filter: &TaskFilter,
) -> Result<Vec<TaskListItem>, DbError> {
    let selection = filter.selection();
    sqlx::query_as!(
        TaskListRow,
        r#"
        SELECT t.id, t.key, t.title, t.title_source, p.key AS project_key,
               p.name AS project_name, ps.id AS persona_id, ps.handle AS persona_handle,
               ps.display_name AS persona_display_name,
               t.status, t.status_line, t.resume_at,
               COALESCE(sub.done, 0) AS "subtasks_done!", COALESCE(sub.total, 0) AS "subtasks_total!",
               COALESCE(sub.all_subtasks, 0) > 0 AS "has_subtasks!",
               COALESCE(nb.own, 0) > 0 AS "needs_you!", COALESCE(nb.below, 0) AS "needs_you_below!",
               t.created_at, t.updated_at
        FROM tasks t
        JOIN projects p ON p.id = t.project_id
        JOIN personas ps ON ps.id = t.persona_id
        LEFT JOIN tasks tp ON tp.id = t.parent_id
        LEFT JOIN LATERAL (
            SELECT count(*) FILTER (WHERE s.status = ANY($4)) AS done,
                   count(*) FILTER (WHERE s.status <> 'cancelled') AS total,
                   count(*) AS all_subtasks
            FROM tasks s WHERE s.parent_id = t.id
        ) sub ON true
        LEFT JOIN LATERAL (
            WITH RECURSIVE tree AS (
                SELECT t.id, 0 AS depth
                UNION ALL
                SELECT c.id, tr.depth + 1 FROM tasks c JOIN tree tr ON c.parent_id = tr.id
            )
            SELECT count(*) FILTER (WHERE tr.depth = 0) AS own,
                   count(*) FILTER (WHERE tr.depth > 0) AS below
            FROM tree tr
            JOIN tasks x ON x.id = tr.id
            JOIN projects xp ON xp.id = x.project_id
            LEFT JOIN tasks xparent ON xparent.id = x.parent_id AND xparent.kind = 'task'
            WHERE x.status = ANY($5)
              AND (x.status <> 'in_review'
                   OR (NOT (xparent.id IS NOT NULL AND x.pr_number IS NULL
                            AND NOT EXISTS (
                                SELECT 1 FROM outbox o
                                WHERE o.kind = $6 AND o.payload ->> 'task_id' = x.id::text
                                  AND (o.payload ->> 'open')::boolean
                                  AND o.done_at IS NULL AND o.gave_up_at IS NULL))
                       AND NOT COALESCE(xp.subtask_merge = 'parent-agent'
                                        AND x.base_task_id = x.parent_id
                                        AND xparent.status <> ALL($7), false)))
        ) nb ON true
        WHERE t.kind = 'task'
          AND ($8::uuid IS NULL OR t.parent_id = $8)
          AND ($11::text IS NULL OR t.key = $11)
          AND (NOT $9 OR t.parent_id IS NULL OR tp.kind = 'chat')
          AND ($1::text[] IS NULL OR t.status = ANY($1)
               OR ($10 AND t.status = ANY($7) AND nb.below > 0))
          AND ($2::text IS NULL OR p.key = $2)
        ORDER BY CASE WHEN $3 = 'updated_desc' THEN t.updated_at END DESC,
                 CASE WHEN $3 = 'updated_asc' THEN t.updated_at END ASC,
                 CASE WHEN $3 = 'created_desc' THEN t.created_at END DESC,
                 CASE WHEN $3 = 'updated_asc' THEN t.id END ASC,
                 t.id DESC
        "#,
        selection.statuses.as_deref(),
        selection.project_key,
        order_key(filter.sort),
        &status_list(TaskStatus::SUBTASK_DONE),
        &status_list(TaskStatus::MAY_NEED_YOU),
        PR_SYNC_KIND,
        &status_list(TaskStatus::DONE),
        selection.subtasks_of,
        selection.top_level,
        selection.keeps_finished,
        selection.key
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(TaskListItem::try_from)
    .collect()
}

/// A task above another one, a link of the breadcrumb of a scoped list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ancestor {
    pub key: TaskKey,
    pub title: String,
}

struct AncestorRow {
    key: String,
    title: String,
}

impl TryFrom<AncestorRow> for Ancestor {
    type Error = DbError;

    fn try_from(row: AncestorRow) -> Result<Self, DbError> {
        Ok(Self {
            key: row.key.parse()?,
            title: row.title,
        })
    }
}

/// The tasks above `id`, the parent first and the top-level task last. It stops below a chat
/// parent (the chat is no task of the list); an unknown `id` has none (`task-list-tree` D4).
pub async fn ancestors(pool: &PgPool, id: TaskId) -> Result<Vec<Ancestor>, DbError> {
    sqlx::query_as!(
        AncestorRow,
        r#"
        WITH RECURSIVE up AS (
            SELECT p.key, p.title, p.parent_id, 1 AS depth
            FROM tasks s JOIN tasks p ON p.id = s.parent_id AND p.kind = 'task'
            WHERE s.id = $1
            UNION ALL
            SELECT p.key, p.title, p.parent_id, up.depth + 1
            FROM up JOIN tasks p ON p.id = up.parent_id AND p.kind = 'task'
        )
        SELECT key AS "key!", title AS "title!" FROM up ORDER BY depth
        "#,
        id.as_uuid()
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Ancestor::try_from)
    .collect()
}

/// The key of `sort` in the `ORDER BY` of [`list_task_items`]. It is independent of the URL value
/// of the sort, and the exhaustive match makes a new `TaskSort` fail to compile here.
const fn order_key(sort: TaskSort) -> &'static str {
    match sort {
        TaskSort::Updated => "updated_desc",
        TaskSort::UpdatedAsc => "updated_asc",
        TaskSort::Created => "created_desc",
    }
}

/// All projects by name with their number of tasks in `group` (all tasks without a group).
pub async fn list_project_filter_items(
    pool: &PgPool,
    group: Option<StatusSet>,
) -> Result<Vec<ProjectFilterItem>, DbError> {
    let statuses = group.map(statuses_of);
    Ok(sqlx::query_as!(
        ProjectFilterItem,
        r#"
        SELECT p.key AS "key!", p.name AS "name!",
               count(t.id) FILTER (WHERE $1::text[] IS NULL OR t.status = ANY($1))
                   AS "task_count!"
        FROM projects p
        LEFT JOIN tasks t ON t.project_id = p.id AND t.kind = 'task'
        GROUP BY p.id
        ORDER BY p.name, p.key
        "#,
        statuses.as_deref()
    )
    .fetch_all(pool)
    .await?)
}

pub async fn get_task_detail_by_key(
    pool: &PgPool,
    key: &str,
) -> Result<Option<TaskDetail>, DbError> {
    sqlx::query_as!(
        TaskDetailRow,
        r#"
        SELECT t.id, t.key, t.title, t.title_source, t.description, p.key AS project_key,
            p.name AS project_name, p.repo_url, p.issue_tracker_url, ps.id AS persona_id,
            ps.handle AS persona_handle, ps.display_name AS persona_display_name,
            ps.harness AS persona_harness, ps.model AS persona_model,
            ps.description AS persona_description, t.issue_number, t.status,
            t.status_line, t.pr_number, t.pr_url, t.pr_title, t.pr_attached, t.archived_at,
            EXISTS (SELECT 1 FROM agent_requests r
                    WHERE r.task_id = t.id AND r.state = $2 AND r.kind = $3)
                AS "has_pending_approval!",
            t.branch,
            t.resume_at, t.auto_resumes, t.created_by, creator.key AS "creator_task_key?",
            parent.key AS "parent_key?", parent.title AS "parent_title?",
            parent_persona.display_name AS "parent_persona_display_name?",
            base.key AS "base_task_key?", base.title AS "base_task_title?"
        FROM tasks t
        JOIN projects p ON p.id = t.project_id
        JOIN personas ps ON ps.id = t.persona_id
        LEFT JOIN tasks creator ON creator.id =
            CASE WHEN t.created_by->>'kind' = 'agent' THEN (t.created_by->>'task_id')::uuid END
        LEFT JOIN tasks parent ON parent.id = t.parent_id
        LEFT JOIN personas parent_persona ON parent_persona.id = parent.persona_id
        LEFT JOIN tasks base ON base.id = t.base_task_id
        WHERE t.key = $1
        "#,
        key,
        crate::requests::RequestState::Pending.as_str(),
        crate::requests::RequestKind::Approval.as_str()
    )
    .fetch_optional(pool)
    .await?
    .map(TaskDetail::try_from)
    .transpose()
}

pub async fn get_task_detail(pool: &PgPool, id: TaskId) -> Result<Option<TaskDetail>, DbError> {
    sqlx::query_as!(
        TaskDetailRow,
        r#"
        SELECT t.id, t.key, t.title, t.title_source, t.description, p.key AS project_key,
            p.name AS project_name, p.repo_url, p.issue_tracker_url, ps.id AS persona_id,
            ps.handle AS persona_handle, ps.display_name AS persona_display_name,
            ps.harness AS persona_harness, ps.model AS persona_model,
            ps.description AS persona_description, t.issue_number, t.status,
            t.status_line, t.pr_number, t.pr_url, t.pr_title, t.pr_attached, t.archived_at,
            EXISTS (SELECT 1 FROM agent_requests r
                    WHERE r.task_id = t.id AND r.state = $2 AND r.kind = $3)
                AS "has_pending_approval!",
            t.branch,
            t.resume_at, t.auto_resumes, t.created_by, creator.key AS "creator_task_key?",
            parent.key AS "parent_key?", parent.title AS "parent_title?",
            parent_persona.display_name AS "parent_persona_display_name?",
            base.key AS "base_task_key?", base.title AS "base_task_title?"
        FROM tasks t
        JOIN projects p ON p.id = t.project_id
        JOIN personas ps ON ps.id = t.persona_id
        LEFT JOIN tasks creator ON creator.id =
            CASE WHEN t.created_by->>'kind' = 'agent' THEN (t.created_by->>'task_id')::uuid END
        LEFT JOIN tasks parent ON parent.id = t.parent_id
        LEFT JOIN personas parent_persona ON parent_persona.id = parent.persona_id
        LEFT JOIN tasks base ON base.id = t.base_task_id
        WHERE t.id = $1
        "#,
        id.as_uuid(),
        crate::requests::RequestState::Pending.as_str(),
        crate::requests::RequestKind::Approval.as_str()
    )
    .fetch_optional(pool)
    .await?
    .map(TaskDetail::try_from)
    .transpose()
}

/// A direct subtask of a task, a row of its Subtasks tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtaskItem {
    pub key: String,
    pub title: String,
    pub title_source: TitleSource,
    pub status: TaskStatus,
    /// The subtask's own subtasks.
    pub subtasks: SubtaskProgress,
}

struct SubtaskRecord {
    key: String,
    title: String,
    title_source: String,
    status: String,
    subtasks_done: i64,
    subtasks_total: i64,
}

/// The direct subtasks of `parent` by task number, each with the progress of its own.
pub async fn list_subtasks(pool: &PgPool, parent: TaskId) -> Result<Vec<SubtaskItem>, DbError> {
    sqlx::query_as!(
        SubtaskRecord,
        r#"
        SELECT s.key, s.title, s.title_source, s.status,
               count(g.id) FILTER (WHERE g.status = ANY($2)) AS "subtasks_done!",
               count(g.id) FILTER (WHERE g.status <> 'cancelled') AS "subtasks_total!"
        FROM tasks s LEFT JOIN tasks g ON g.parent_id = s.id
        WHERE s.parent_id = $1
        GROUP BY s.id
        ORDER BY s.number
        "#,
        parent.as_uuid(),
        &status_list(TaskStatus::SUBTASK_DONE)
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| {
        Ok(SubtaskItem {
            key: r.key,
            title: r.title,
            title_source: r.title_source.parse()?,
            status: r.status.parse()?,
            subtasks: SubtaskProgress {
                done: r.subtasks_done,
                total: r.subtasks_total,
            },
        })
    })
    .collect()
}

struct DependencyRecord {
    key: String,
    title: String,
    kind: String,
    status: String,
    first_completed_at: Option<jiff_sqlx::Timestamp>,
    first_done_at: Option<jiff_sqlx::Timestamp>,
}

/// The tasks `task_id` waits for and the tasks that wait for it.
pub async fn list_dependencies(pool: &PgPool, task_id: TaskId) -> Result<Dependencies, DbError> {
    let waits_for = sqlx::query_as!(
        DependencyRecord,
        "SELECT t.key, t.title, d.kind, t.status, t.first_completed_at, t.first_done_at
         FROM task_dependencies d JOIN tasks t ON t.id = d.depends_on_task_id
         WHERE d.task_id = $1 ORDER BY t.number",
        task_id.as_uuid()
    )
    .fetch_all(pool)
    .await?;
    let needed_by = sqlx::query_as!(
        DependencyRecord,
        "SELECT t.key, t.title, d.kind, t.status, t.first_completed_at, t.first_done_at
         FROM task_dependencies d JOIN tasks t ON t.id = d.task_id
         WHERE d.depends_on_task_id = $1 ORDER BY t.number",
        task_id.as_uuid()
    )
    .fetch_all(pool)
    .await?;
    Ok(Dependencies {
        waits_for: waits_for
            .into_iter()
            .map(|r| {
                let kind: DependencyKind = r.kind.parse()?;
                Ok(DependencyView {
                    satisfied: is_satisfied(
                        kind,
                        r.first_completed_at.map(Into::into),
                        r.first_done_at.map(Into::into),
                    ),
                    key: r.key,
                    title: r.title,
                    kind,
                })
            })
            .collect::<Result<_, DbError>>()?,
        needed_by: needed_by
            .into_iter()
            .map(|r| {
                Ok(DependantView {
                    kind: r.kind.parse()?,
                    status: r.status.parse()?,
                    key: r.key,
                    title: r.title,
                })
            })
            .collect::<Result<_, DbError>>()?,
    })
}

/// Runs of a task, newest first, with token usage summed per model from the gateway's records
/// and the count of requests with unknown usage.
pub async fn list_runs(pool: &PgPool, task_id: TaskId) -> Result<Vec<RunView>, DbError> {
    let runs = sqlx::query_as!(
        RunRow,
        "SELECT id, number, state, outcome, detail, retry_after, started_at, ended_at
         FROM runs WHERE task_id = $1 ORDER BY number DESC",
        task_id.as_uuid()
    )
    .fetch_all(pool)
    .await?;
    let usage = sqlx::query_as!(
        RunUsageRow,
        r#"
        SELECT u.run_id, COALESCE(u.model, 'unknown') AS "model_id!",
               COALESCE(SUM(u.input_tokens), 0)::bigint AS "input_tokens!",
               COALESCE(SUM(u.output_tokens), 0)::bigint AS "output_tokens!",
               COALESCE(SUM(u.cache_read_tokens), 0)::bigint AS "cache_read_tokens!",
               COALESCE(SUM(u.cache_write_tokens), 0)::bigint AS "cache_write_tokens!"
        FROM model_usage u JOIN runs r ON r.id = u.run_id
        WHERE r.task_id = $1 AND u.usage_known
        GROUP BY u.run_id, u.model
        ORDER BY u.run_id, 2
        "#,
        task_id.as_uuid()
    )
    .fetch_all(pool)
    .await?;
    let unknown = sqlx::query!(
        r#"
        SELECT u.run_id, count(*) AS "count!" FROM model_usage u JOIN runs r ON r.id = u.run_id
        WHERE r.task_id = $1 AND NOT u.usage_known GROUP BY u.run_id
        "#,
        task_id.as_uuid()
    )
    .fetch_all(pool)
    .await?;
    let run_ids: Vec<Uuid> = runs.iter().map(|run| run.id).collect();
    let mut mcp_calls = crate::mcp_servers::list_mcp_calls_of_runs(pool, &run_ids).await?;
    runs.into_iter()
        .map(|r| {
            Ok(RunView {
                mcp_calls: mcp_calls.remove(&r.id).unwrap_or_default(),
                usage: usage
                    .iter()
                    .filter(|u| u.run_id == r.id)
                    .map(|u| ModelUsage {
                        model_id: u.model_id.clone(),
                        input_tokens: u.input_tokens,
                        output_tokens: u.output_tokens,
                        cache_read_tokens: u.cache_read_tokens,
                        cache_write_tokens: u.cache_write_tokens,
                    })
                    .collect(),
                unknown_usage_requests: unknown
                    .iter()
                    .find(|row| row.run_id == r.id)
                    .map_or(0, |row| row.count),
                id: r.id,
                number: r.number,
                state: r.state.parse()?,
                outcome: r.outcome,
                detail: r.detail,
                retry_after: r.retry_after.map(Into::into),
                started_at: r.started_at,
                ended_at: r.ended_at.map(Into::into),
            })
        })
        .collect()
}

struct TranscriptRow {
    id: i64,
    payload: serde_json::Value,
    occurred_at: Timestamp,
    number: i64,
    started_at: Timestamp,
}

/// Decodes a stored transcript event. Events stored before the gateway measured usage carry
/// `turnEnded.usage` and `turnEnded.usageSource`, fields that no longer exist: they are dropped
/// (the decoder rejects unknown fields otherwise).
fn decode_event(mut payload: serde_json::Value) -> Result<Event, serde_json::Error> {
    if let Some(ended) = payload
        .get_mut("turnEnded")
        .and_then(serde_json::Value::as_object_mut)
    {
        ended.remove("usage");
        ended.remove("usageSource");
    }
    serde_json::from_value(payload)
}

/// Transcript rows of a task with `id > after`, in id order, at most `limit`.
pub async fn transcript_after(
    pool: &PgPool,
    task_id: TaskId,
    after: i64,
    limit: i64,
) -> Result<Vec<StoredEvent>, DbError> {
    let rows = sqlx::query_as!(
        TranscriptRow,
        "SELECT e.id, e.payload, e.occurred_at, r.number, r.started_at
         FROM transcript_events e JOIN runs r ON r.id = e.run_id
         WHERE e.task_id = $1 AND e.id > $2 ORDER BY e.id LIMIT $3",
        task_id.as_uuid(),
        after,
        limit
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let event = decode_event(row.payload)
                .inspect_err(
                    |error| tracing::warn!(%error, id = row.id, "undecodable transcript event"),
                )
                .ok();
            StoredEvent {
                id: row.id,
                event,
                occurred_at: row.occurred_at,
                run_number: row.number,
                run_started_at: row.started_at,
            }
        })
        .collect())
}

pub(crate) fn statuses_of(set: StatusSet) -> Vec<String> {
    status_list(set.statuses())
}

/// A status list as the `text[]` argument of `status = ANY($n)` / `status <> ALL($n)`.
pub(crate) fn status_list(statuses: &[TaskStatus]) -> Vec<String> {
    statuses.iter().map(|s| s.as_str().to_string()).collect()
}

/// Number of tasks of a project in each status that has any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectStatusCount {
    pub project_id: Uuid,
    pub status: String,
    pub count: i64,
}

/// Tasks per project and status in one grouped query; the UI folds them into its columns.
pub async fn project_status_counts(pool: &PgPool) -> Result<Vec<ProjectStatusCount>, DbError> {
    Ok(sqlx::query_as!(
        ProjectStatusCount,
        r#"SELECT project_id, status, count(*) AS "count!" FROM tasks GROUP BY project_id, status"#
    )
    .fetch_all(pool)
    .await?)
}

#[cfg(test)]
mod tests;
