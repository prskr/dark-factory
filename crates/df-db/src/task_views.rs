//! Read models of the web UI: task list, task detail, runs with token usage (from the gateway's
//! usage records) and the transcript.

use chrono::{DateTime, Utc};
use df_core::{
    DependencyKind, Principal, RunState, StatusSet, TaskId, TaskSort, TaskStatus, TitleSource,
    is_satisfied,
};
use df_proto::transcript::v1::Event;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::pool::DbError;

/// One row of the task list.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct TaskListItem {
    pub id: Uuid,
    pub key: String,
    pub title: String,
    /// `Placeholder` while the title is provisional.
    #[sqlx(try_from = "String")]
    pub title_source: TitleSource,
    pub project_key: String,
    pub project_name: String,
    pub persona_name: String,
    pub status: String,
    pub status_line: String,
    /// When the scheduler may start the task's next run.
    pub resume_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Which tasks the list shows; the default is all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskFilter {
    /// Only tasks whose status is in this set.
    pub group: Option<StatusSet>,
    /// Only tasks of the project with this key.
    pub project_key: Option<String>,
    /// The order of the rows.
    pub sort: TaskSort,
}

impl TaskFilter {
    /// The status names of the group, or `None` for every status.
    fn statuses(&self) -> Option<Vec<String>> {
        self.group.map(statuses_of)
    }
}

/// A project of the project filter with its number of tasks in the current status group.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ProjectFilterItem {
    pub key: String,
    pub name: String,
    pub task_count: i64,
}

/// A task with the names and links the detail page shows.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct TaskDetail {
    pub id: Uuid,
    pub key: String,
    pub title: String,
    #[sqlx(try_from = "String")]
    pub title_source: TitleSource,
    pub description: String,
    pub project_key: String,
    pub project_name: String,
    pub repo_url: String,
    pub issue_tracker_url: Option<String>,
    pub persona_name: String,
    /// Harness of the persona, for example `claude-code`.
    pub persona_harness: String,
    /// Model of the persona; `None` means the instance default.
    pub persona_model: Option<String>,
    pub issue_number: Option<i64>,
    #[sqlx(try_from = "String")]
    pub status: TaskStatus,
    pub status_line: String,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub pr_title: Option<String>,
    pub resume_at: Option<DateTime<Utc>>,
    /// Consecutive automatic resumes after infrastructure failures.
    pub auto_resumes: i32,
    #[sqlx(json)]
    pub created_by: Principal,
    /// Key of the task whose agent created this one, if an agent did.
    pub creator_task_key: Option<String>,
    pub parent_key: Option<String>,
    pub parent_title: Option<String>,
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
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
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
    pub retry_after: Option<DateTime<Utc>>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub usage: Vec<ModelUsage>,
    /// Model requests whose response carried no usage (unknown, not zero).
    pub unknown_usage_requests: i64,
}

#[derive(FromRow)]
struct RunRow {
    id: Uuid,
    number: i64,
    #[sqlx(try_from = "String")]
    state: RunState,
    outcome: Option<String>,
    detail: String,
    retry_after: Option<DateTime<Utc>>,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
}

#[derive(FromRow)]
struct RunUsageRow {
    run_id: Uuid,
    #[sqlx(flatten)]
    usage: ModelUsage,
}

/// A stored transcript row; `event` is `None` if the payload cannot be decoded as v1.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvent {
    /// Task-wide, increasing id; the SSE event id.
    pub id: i64,
    pub event: Option<Event>,
    /// When the runtime recorded the event.
    pub occurred_at: DateTime<Utc>,
    /// Number of the run that produced the event and when that run started.
    pub run_number: i64,
    pub run_started_at: DateTime<Utc>,
}

/// The tasks matching `filter` in the order of `filter.sort`. Ties are ordered by task ID in the
/// direction of the sort, so rows with the same time keep their place between calls.
pub async fn list_task_items(
    pool: &PgPool,
    filter: &TaskFilter,
) -> Result<Vec<TaskListItem>, DbError> {
    Ok(sqlx::query_as(
        "SELECT t.id, t.key, t.title, t.title_source, p.key AS project_key,
                p.name AS project_name, ps.name AS persona_name,
                t.status, t.status_line, t.resume_at, t.created_at, t.updated_at
         FROM tasks t
         JOIN projects p ON p.id = t.project_id
         JOIN personas ps ON ps.id = t.persona_id
         WHERE ($1::text[] IS NULL OR t.status = ANY($1))
           AND ($2::text IS NULL OR p.key = $2)
         ORDER BY CASE WHEN $3 = 'updated_desc' THEN t.updated_at END DESC,
                  CASE WHEN $3 = 'updated_asc' THEN t.updated_at END ASC,
                  CASE WHEN $3 = 'created_desc' THEN t.created_at END DESC,
                  CASE WHEN $3 = 'updated_asc' THEN t.id END ASC,
                  t.id DESC",
    )
    .bind(filter.statuses())
    .bind(&filter.project_key)
    .bind(order_key(filter.sort))
    .fetch_all(pool)
    .await?)
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
    Ok(sqlx::query_as(
        "SELECT p.key, p.name,
                count(t.id) FILTER (WHERE $1::text[] IS NULL OR t.status = ANY($1)) AS task_count
         FROM projects p
         LEFT JOIN tasks t ON t.project_id = p.id
         GROUP BY p.id
         ORDER BY p.name, p.key",
    )
    .bind(statuses)
    .fetch_all(pool)
    .await?)
}

/// `SELECT` of a task detail with a trailing `WHERE` clause.
macro_rules! select_task_detail {
    ($tail:literal) => {
        concat!(
            "SELECT t.id, t.key, t.title, t.title_source, t.description, p.key AS project_key,
                p.name AS project_name, p.repo_url, p.issue_tracker_url, ps.name AS persona_name,
                ps.harness AS persona_harness, ps.model AS persona_model, t.issue_number, t.status, t.status_line, t.pr_number, t.pr_url, t.pr_title,
                t.resume_at, t.auto_resumes, t.created_by, creator.key AS creator_task_key,
                parent.key AS parent_key, parent.title AS parent_title
             FROM tasks t
             JOIN projects p ON p.id = t.project_id
             JOIN personas ps ON ps.id = t.persona_id
             LEFT JOIN tasks creator ON creator.id =
                 CASE WHEN t.created_by->>'kind' = 'agent' THEN (t.created_by->>'task_id')::uuid END
             LEFT JOIN tasks parent ON parent.id = t.parent_id ",
            $tail
        )
    };
}

pub async fn get_task_detail_by_key(
    pool: &PgPool,
    key: &str,
) -> Result<Option<TaskDetail>, DbError> {
    Ok(sqlx::query_as(select_task_detail!("WHERE t.key = $1"))
        .bind(key)
        .fetch_optional(pool)
        .await?)
}

pub async fn get_task_detail(pool: &PgPool, id: TaskId) -> Result<Option<TaskDetail>, DbError> {
    Ok(sqlx::query_as(select_task_detail!("WHERE t.id = $1"))
        .bind(id.as_uuid())
        .fetch_optional(pool)
        .await?)
}

#[derive(FromRow)]
struct DependencyRecord {
    key: String,
    title: String,
    kind: String,
    status: String,
    first_completed_at: Option<DateTime<Utc>>,
    first_done_at: Option<DateTime<Utc>>,
}

/// The tasks `task_id` waits for and the tasks that wait for it.
pub async fn list_dependencies(pool: &PgPool, task_id: TaskId) -> Result<Dependencies, DbError> {
    let waits_for: Vec<DependencyRecord> = sqlx::query_as(
        "SELECT t.key, t.title, d.kind, t.status, t.first_completed_at, t.first_done_at
         FROM task_dependencies d JOIN tasks t ON t.id = d.depends_on_task_id
         WHERE d.task_id = $1 ORDER BY t.number",
    )
    .bind(task_id.as_uuid())
    .fetch_all(pool)
    .await?;
    let needed_by: Vec<DependencyRecord> = sqlx::query_as(
        "SELECT t.key, t.title, d.kind, t.status, t.first_completed_at, t.first_done_at
         FROM task_dependencies d JOIN tasks t ON t.id = d.task_id
         WHERE d.depends_on_task_id = $1 ORDER BY t.number",
    )
    .bind(task_id.as_uuid())
    .fetch_all(pool)
    .await?;
    Ok(Dependencies {
        waits_for: waits_for
            .into_iter()
            .map(|r| {
                let kind: DependencyKind = r.kind.parse()?;
                Ok(DependencyView {
                    satisfied: is_satisfied(kind, r.first_completed_at, r.first_done_at),
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
    let runs: Vec<RunRow> = sqlx::query_as(
        "SELECT id, number, state, outcome, detail, retry_after, started_at, ended_at
         FROM runs WHERE task_id = $1 ORDER BY number DESC",
    )
    .bind(task_id.as_uuid())
    .fetch_all(pool)
    .await?;
    let usage: Vec<RunUsageRow> = sqlx::query_as(
        "SELECT u.run_id, COALESCE(u.model, 'unknown') AS model_id,
                COALESCE(SUM(u.input_tokens), 0)::bigint AS input_tokens,
                COALESCE(SUM(u.output_tokens), 0)::bigint AS output_tokens,
                COALESCE(SUM(u.cache_read_tokens), 0)::bigint AS cache_read_tokens,
                COALESCE(SUM(u.cache_write_tokens), 0)::bigint AS cache_write_tokens
         FROM model_usage u JOIN runs r ON r.id = u.run_id
         WHERE r.task_id = $1 AND u.usage_known
         GROUP BY u.run_id, u.model
         ORDER BY u.run_id, model_id",
    )
    .bind(task_id.as_uuid())
    .fetch_all(pool)
    .await?;
    let unknown: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT u.run_id, count(*) FROM model_usage u JOIN runs r ON r.id = u.run_id
         WHERE r.task_id = $1 AND NOT u.usage_known GROUP BY u.run_id",
    )
    .bind(task_id.as_uuid())
    .fetch_all(pool)
    .await?;
    Ok(runs
        .into_iter()
        .map(|r| RunView {
            usage: usage
                .iter()
                .filter(|u| u.run_id == r.id)
                .map(|u| u.usage.clone())
                .collect(),
            unknown_usage_requests: unknown
                .iter()
                .find(|(run_id, _)| *run_id == r.id)
                .map_or(0, |(_, count)| *count),
            id: r.id,
            number: r.number,
            state: r.state,
            outcome: r.outcome,
            detail: r.detail,
            retry_after: r.retry_after,
            started_at: r.started_at,
            ended_at: r.ended_at,
        })
        .collect())
}

#[derive(FromRow)]
struct TranscriptRow {
    id: i64,
    payload: serde_json::Value,
    occurred_at: DateTime<Utc>,
    number: i64,
    started_at: DateTime<Utc>,
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
    let rows: Vec<TranscriptRow> = sqlx::query_as(
        "SELECT e.id, e.payload, e.occurred_at, r.number, r.started_at
         FROM transcript_events e JOIN runs r ON r.id = e.run_id
         WHERE e.task_id = $1 AND e.id > $2 ORDER BY e.id LIMIT $3",
    )
    .bind(task_id.as_uuid())
    .bind(after)
    .bind(limit)
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
    set.statuses()
        .iter()
        .map(|s| s.as_str().to_string())
        .collect()
}

/// Number of tasks of a project in each status that has any.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ProjectStatusCount {
    pub project_id: Uuid,
    pub status: String,
    pub count: i64,
}

/// Tasks per project and status in one grouped query; the UI folds them into its columns.
pub async fn project_status_counts(pool: &PgPool) -> Result<Vec<ProjectStatusCount>, DbError> {
    Ok(sqlx::query_as(
        "SELECT project_id, status, count(*) AS count FROM tasks GROUP BY project_id, status",
    )
    .fetch_all(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use df_core::TaskStatus;

    async fn seed(pool: &PgPool) -> (TaskId, Uuid) {
        let project = Uuid::now_v7();
        let persona = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, 'DF', 'Dark Factory', 'https://g/o/r.git', 'img')",
        )
        .bind(project)
        .execute(pool)
        .await
        .unwrap();
        crate::model_providers::tests::seed_provider(pool).await;
        sqlx::query("INSERT INTO personas (id, name, system_prompt, model, model_provider_id) VALUES ($1, 'Linus', 'p', 'test-model', (SELECT id FROM model_providers LIMIT 1))")
            .bind(persona)
            .execute(pool)
            .await
            .unwrap();
        let (task, _) = crate::create_task(
            pool,
            crate::NewTask {
                project_id: df_core::ProjectId::from_uuid(project),
                persona_id: df_core::PersonaId::from_uuid(persona),
                title: Some("Fix login".into()),
                description: String::new(),
                issue_number: Some(7),
                created_by: Principal::system("test"),
                parent_id: None,
                depends_on: Vec::new(),
            },
        )
        .await
        .unwrap();
        let run = Uuid::now_v7();
        sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'running')")
            .bind(run)
            .bind(task.id.as_uuid())
            .execute(pool)
            .await
            .unwrap();
        (task.id, run)
    }

    async fn insert_event(
        pool: &PgPool,
        task: TaskId,
        run: Uuid,
        seq: i64,
        kind: &str,
        payload: serde_json::Value,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO transcript_events (task_id, run_id, seq, occurred_at, kind, payload)
             VALUES ($1, $2, $3, now(), $4, $5) RETURNING id",
        )
        .bind(task.as_uuid())
        .bind(run)
        .bind(seq)
        .bind(kind)
        .bind(payload)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// A gateway usage record; without a model its usage is unknown.
    async fn insert_usage(
        pool: &PgPool,
        run: Uuid,
        model: Option<&str>,
        input: i64,
        output: i64,
        cache_read: i64,
    ) {
        sqlx::query(
            "INSERT INTO model_usage (request_id, run_id, recorded_at, model, wire_api, input_tokens, output_tokens, cache_read_tokens, usage_known)
             VALUES ($1, $2, now(), $3, 'anthropic-messages', $4, $5, $6, $7)",
        )
        .bind(Uuid::now_v7())
        .bind(run)
        .bind(model)
        .bind(input)
        .bind(output)
        .bind(cache_read)
        .bind(model.is_some())
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn lists_tasks_details_runs_usage_and_transcript() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, run) = seed(&pool).await;

        let list = list_task_items(&pool, &TaskFilter::default())
            .await
            .unwrap();
        assert_eq!(
            (list[0].key.as_str(), list[0].status.as_str()),
            ("DF-1", "queued")
        );
        let detail = get_task_detail_by_key(&pool, "DF-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (detail.project_key.as_str(), detail.issue_number),
            ("DF", Some(7))
        );
        assert_eq!(
            get_task_detail(&pool, task).await.unwrap().unwrap().id,
            detail.id
        );
        assert!(
            get_task_detail_by_key(&pool, "DF-9")
                .await
                .unwrap()
                .is_none()
        );

        let ended = || serde_json::json!({"turnEnded": {}});
        let first = insert_event(&pool, task, run, 1, "turn_ended", ended()).await;
        insert_event(&pool, task, run, 2, "turn_ended", ended()).await;
        insert_event(&pool, task, run, 3, "turn_ended", ended()).await;
        for (input, model) in [(10, "opus"), (20, "opus"), (1, "haiku")] {
            insert_usage(&pool, run, Some(model), input, 5, 2).await;
        }
        insert_usage(&pool, run, None, 0, 0, 0).await;
        insert_usage(&pool, run, None, 0, 0, 0).await;
        let last = insert_event(&pool, task, run, 4, "bogus", serde_json::json!({"nope": 1})).await;

        let runs = list_runs(&pool, task).await.unwrap();
        assert_eq!(runs.len(), 1);
        let opus = runs[0].usage.iter().find(|u| u.model_id == "opus").unwrap();
        assert_eq!(
            (
                opus.input_tokens,
                opus.output_tokens,
                opus.cache_read_tokens,
                opus.cache_write_tokens
            ),
            (30, 10, 4, 0)
        );
        assert_eq!(runs[0].usage.len(), 2);
        assert_eq!(runs[0].unknown_usage_requests, 2);

        let all = transcript_after(&pool, task, 0, 100).await.unwrap();
        assert_eq!(all.len(), 4);
        assert!(all[0].event.is_some());
        assert!(all[3].event.is_none());
        let tail = transcript_after(&pool, task, first, 100).await.unwrap();
        assert_eq!(tail.first().map(|e| e.id), Some(first + 1));
        assert_eq!(tail.last().map(|e| e.id), Some(last));
        assert_eq!(transcript_after(&pool, task, 0, 2).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn turn_ends_stored_with_the_removed_usage_fields_still_decode() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, run) = seed(&pool).await;
        let old = serde_json::json!({
            "schemaVersion": 1,
            "turnEnded": {
                "stopReason": "STOP_REASON_END_TURN",
                "usage": [{"modelId": "opus", "inputTokens": "10"}],
                "usageSource": 1
            }
        });
        insert_event(&pool, task, run, 1, "turn_ended", old).await;

        let rows = transcript_after(&pool, task, 0, 10).await.unwrap();

        let Some(Event {
            kind: Some(df_proto::transcript::v1::event::Kind::TurnEnded(ended)),
            ..
        }) = &rows[0].event
        else {
            panic!("the old turn end does not decode: {:?}", rows[0].event);
        };
        assert_eq!(
            ended.stop_reason(),
            df_proto::transcript::v1::StopReason::EndTurn
        );
    }

    #[tokio::test]
    async fn lists_runs_newest_first() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, _) = seed(&pool).await;
        sqlx::query(
            "INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 2, 'succeeded')",
        )
        .bind(Uuid::now_v7())
        .bind(task.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
        let numbers: Vec<i64> = list_runs(&pool, task)
            .await
            .unwrap()
            .iter()
            .map(|r| r.number)
            .collect();
        assert_eq!(numbers, [2, 1]);
    }

    /// Three tasks with chosen times: DF-1 updated last, DF-2 and DF-3 with the same update time,
    /// and a creation order (DF-3, DF-1, DF-2) unlike the order of the IDs, so that the sorts
    /// and the tie breaker (ID) can be told apart.
    async fn seed_sorted_tasks(pool: &PgPool) {
        seed(pool).await;
        let (project, persona): (Uuid, Uuid) =
            sqlx::query_as("SELECT project_id, persona_id FROM tasks")
                .fetch_one(pool)
                .await
                .unwrap();
        for title in ["Second", "Third"] {
            crate::create_task(
                pool,
                crate::NewTask {
                    project_id: df_core::ProjectId::from_uuid(project),
                    persona_id: df_core::PersonaId::from_uuid(persona),
                    title: Some(title.into()),
                    description: String::new(),
                    issue_number: None,
                    created_by: Principal::system("test"),
                    parent_id: None,
                    depends_on: Vec::new(),
                },
            )
            .await
            .unwrap();
        }
        for (key, created, updated) in [("DF-1", 10, 50), ("DF-2", 20, 20), ("DF-3", 0, 20)] {
            sqlx::query(
                "UPDATE tasks SET created_at = '2026-11-01T00:00:00Z'::timestamptz + make_interval(mins => $2::int),
                                  updated_at = '2026-11-01T00:00:00Z'::timestamptz + make_interval(mins => $3::int)
                 WHERE key = $1",
            )
            .bind(key)
            .bind(created)
            .bind(updated)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn lists_tasks_in_each_sort_with_a_stable_tie_order() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        seed_sorted_tasks(&pool).await;
        let keys = |sort| {
            let pool = pool.clone();
            async move {
                list_task_items(
                    &pool,
                    &TaskFilter {
                        sort,
                        ..TaskFilter::default()
                    },
                )
                .await
                .unwrap()
                .into_iter()
                .map(|t| t.key)
                .collect::<Vec<_>>()
            }
        };

        // The default is the last update, newest first.
        assert_eq!(TaskFilter::default().sort, TaskSort::Updated);
        // Every variant, so a sort whose key the query does not know cannot pass: it would fall
        // through to the tie breaker alone (newest ID first).
        for sort in TaskSort::ALL {
            let expected = match sort {
                // DF-2 and DF-3 tie on the update time; the tie breaks by ID, newest first.
                TaskSort::Updated => ["DF-1", "DF-3", "DF-2"],
                TaskSort::UpdatedAsc => ["DF-2", "DF-3", "DF-1"],
                TaskSort::Created => ["DF-2", "DF-1", "DF-3"],
            };
            assert_eq!(keys(sort).await, expected, "{sort:?}");
        }
    }

    #[tokio::test]
    async fn the_sort_keeps_the_status_and_project_filters() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        seed_sorted_tasks(&pool).await;
        let filter = TaskFilter {
            group: Some(StatusSet::Active),
            project_key: Some("DF".into()),
            sort: TaskSort::UpdatedAsc,
        };
        let keys: Vec<_> = list_task_items(&pool, &filter)
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.key)
            .collect();
        assert_eq!(keys, ["DF-2", "DF-3", "DF-1"]);
    }

    #[tokio::test]
    async fn filters_tasks_by_status_group_and_project_and_counts_per_project() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, _) = seed(&pool).await;
        sqlx::query("INSERT INTO projects (id, key, name, repo_url, image) VALUES ($1, 'WEB', 'Website', 'https://g/o/w.git', 'img')")
            .bind(Uuid::now_v7())
            .execute(&pool)
            .await
            .unwrap();
        let by = |group, project: Option<&str>| TaskFilter {
            group,
            project_key: project.map(str::to_string),
            ..TaskFilter::default()
        };

        let active = list_task_items(&pool, &by(Some(StatusSet::Active), None))
            .await
            .unwrap();
        let done = list_task_items(&pool, &by(Some(StatusSet::Done), None))
            .await
            .unwrap();
        let web = list_task_items(&pool, &by(None, Some("WEB")))
            .await
            .unwrap();
        let df = list_task_items(&pool, &by(None, Some("DF"))).await.unwrap();
        assert_eq!(
            (active.len(), done.len(), web.len(), df.len()),
            (1, 0, 0, 1)
        );

        crate::task_events::set_status_for_test(&pool, task, TaskStatus::Completed).await;
        let active = list_task_items(&pool, &by(Some(StatusSet::Active), None))
            .await
            .unwrap();
        let done = list_task_items(&pool, &by(Some(StatusSet::Done), None))
            .await
            .unwrap();
        assert_eq!((active.len(), done.len()), (0, 1));

        let counts = list_project_filter_items(&pool, Some(StatusSet::Done))
            .await
            .unwrap();
        let counts: Vec<_> = counts
            .iter()
            .map(|p| (p.key.as_str(), p.task_count))
            .collect();
        assert_eq!(counts, [("DF", 1), ("WEB", 0)]);
        let all = list_project_filter_items(&pool, None).await.unwrap();
        assert_eq!(all[0].task_count, 1);
    }

    #[tokio::test]
    async fn narrower_sets_and_counts_per_project_and_status() {
        let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
        let (task, _) = seed(&pool).await;
        let by = |set| TaskFilter {
            group: Some(set),
            project_key: None,
            ..TaskFilter::default()
        };
        crate::task_events::set_status_for_test(&pool, task, TaskStatus::RateLimited).await;
        let running = list_task_items(&pool, &by(StatusSet::Running))
            .await
            .unwrap();
        let queued = list_task_items(&pool, &by(StatusSet::Queued))
            .await
            .unwrap();
        assert_eq!((running.len(), queued.len()), (1, 0));

        let counts = project_status_counts(&pool).await.unwrap();
        assert_eq!(counts.len(), 1);
        assert_eq!(counts[0].status, "rate_limited");
        assert_eq!(counts[0].count, 1);
    }
}
