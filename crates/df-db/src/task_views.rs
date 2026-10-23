//! Read models of the web UI: task list, task detail, runs with token usage and the transcript.

use chrono::{DateTime, Utc};
use df_core::{RunState, StatusGroup, TaskId, TaskStatus, TitleSource};
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Which tasks the list shows; the default is all of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskFilter {
    /// Only tasks whose status is in this group.
    pub group: Option<StatusGroup>,
    /// Only tasks of the project with this key.
    pub project_key: Option<String>,
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
    pub issue_number: Option<i64>,
    pub status: String,
    pub status_line: String,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub pr_title: Option<String>,
}

/// Token counts of one model within a run, summed over all turns.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct ModelUsage {
    pub model_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
}

/// A run with its raw token counts per model.
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
}

/// The tasks matching `filter`, newest first.
pub async fn list_task_items(
    pool: &PgPool,
    filter: &TaskFilter,
) -> Result<Vec<TaskListItem>, DbError> {
    Ok(sqlx::query_as(
        "SELECT t.id, t.key, t.title, t.title_source, p.key AS project_key,
                p.name AS project_name, ps.name AS persona_name,
                t.status, t.status_line, t.created_at, t.updated_at
         FROM tasks t
         JOIN projects p ON p.id = t.project_id
         JOIN personas ps ON ps.id = t.persona_id
         WHERE ($1::text[] IS NULL OR t.status = ANY($1))
           AND ($2::text IS NULL OR p.key = $2)
         ORDER BY t.created_at DESC, t.id DESC",
    )
    .bind(filter.statuses())
    .bind(&filter.project_key)
    .fetch_all(pool)
    .await?)
}

/// All projects by name with their number of tasks in `group` (all tasks without a group).
pub async fn list_project_filter_items(
    pool: &PgPool,
    group: Option<StatusGroup>,
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
                t.issue_number, t.status, t.status_line, t.pr_number, t.pr_url, t.pr_title
             FROM tasks t
             JOIN projects p ON p.id = t.project_id
             JOIN personas ps ON ps.id = t.persona_id ",
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

/// Runs of a task in order, with token usage summed per model from the `TurnEnded` events.
pub async fn list_runs(pool: &PgPool, task_id: TaskId) -> Result<Vec<RunView>, DbError> {
    let runs: Vec<RunRow> = sqlx::query_as(
        "SELECT id, number, state, outcome, detail, retry_after, started_at, ended_at
         FROM runs WHERE task_id = $1 ORDER BY number",
    )
    .bind(task_id.as_uuid())
    .fetch_all(pool)
    .await?;
    // `payload` is the canonical protobuf JSON: 64-bit counters are strings and zeros are omitted.
    let usage: Vec<RunUsageRow> = sqlx::query_as(
        "SELECT e.run_id, u->>'modelId' AS model_id,
                COALESCE(SUM((u->>'inputTokens')::bigint), 0)::bigint AS input_tokens,
                COALESCE(SUM((u->>'outputTokens')::bigint), 0)::bigint AS output_tokens,
                COALESCE(SUM((u->>'cacheReadTokens')::bigint), 0)::bigint AS cache_read_tokens,
                COALESCE(SUM((u->>'cacheWriteTokens')::bigint), 0)::bigint AS cache_write_tokens
         FROM transcript_events e,
              jsonb_array_elements(COALESCE(e.payload->'turnEnded'->'usage', '[]'::jsonb)) u
         WHERE e.task_id = $1 AND e.kind = 'turn_ended'
         GROUP BY e.run_id, u->>'modelId'
         ORDER BY e.run_id, model_id",
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

/// Transcript rows of a task with `id > after`, in id order, at most `limit`.
pub async fn transcript_after(
    pool: &PgPool,
    task_id: TaskId,
    after: i64,
    limit: i64,
) -> Result<Vec<StoredEvent>, DbError> {
    let rows: Vec<(i64, serde_json::Value)> = sqlx::query_as(
        "SELECT id, payload FROM transcript_events
         WHERE task_id = $1 AND id > $2 ORDER BY id LIMIT $3",
    )
    .bind(task_id.as_uuid())
    .bind(after)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, payload)| {
            let event = serde_json::from_value(payload)
                .inspect_err(|error| tracing::warn!(%error, id, "undecodable transcript event"))
                .ok();
            StoredEvent { id, event }
        })
        .collect())
}

fn statuses_of(group: StatusGroup) -> Vec<String> {
    TaskStatus::ALL
        .iter()
        .filter(|s| s.group() == group)
        .map(|s| s.as_str().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        sqlx::query("INSERT INTO personas (id, name, system_prompt) VALUES ($1, 'Linus', 'p')")
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
                created_by: None,
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

        let ended = |input: &str, model: &str| serde_json::json!({"turnEnded": {"usage": [{"modelId": model, "inputTokens": input, "outputTokens": "5", "cacheReadTokens": "2"}]}});
        let first = insert_event(&pool, task, run, 1, "turn_ended", ended("10", "opus")).await;
        insert_event(&pool, task, run, 2, "turn_ended", ended("20", "opus")).await;
        insert_event(&pool, task, run, 3, "turn_ended", ended("1", "haiku")).await;
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
        };

        let active = list_task_items(&pool, &by(Some(StatusGroup::Active), None))
            .await
            .unwrap();
        let done = list_task_items(&pool, &by(Some(StatusGroup::Done), None))
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

        crate::update_task_status(&pool, task, TaskStatus::Completed)
            .await
            .unwrap();
        let active = list_task_items(&pool, &by(Some(StatusGroup::Active), None))
            .await
            .unwrap();
        let done = list_task_items(&pool, &by(Some(StatusGroup::Done), None))
            .await
            .unwrap();
        assert_eq!((active.len(), done.len()), (0, 1));

        let counts = list_project_filter_items(&pool, Some(StatusGroup::Done))
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
}
