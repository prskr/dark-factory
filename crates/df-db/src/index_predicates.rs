//! Partial index predicates can't bind a parameter, so they repeat the state lists of the enum
//! constants. These tests read the predicates back from `pg_indexes`: a new member of a constant
//! fails them until a migration recreates the index.

use std::collections::BTreeSet;

use df_core::{RunState, TaskStatus};
use sqlx::PgPool;

use crate::requests::RequestState;

/// The quoted literals of the predicate of index `name`, for example `succeeded` and `failed`
/// of `WHERE (state <> ALL (ARRAY['succeeded'::text, 'failed'::text]))`.
async fn predicate_values(pool: &PgPool, name: &str) -> BTreeSet<String> {
    let definition: String =
        sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE indexname = $1")
            .bind(name)
            .fetch_one(pool)
            .await
            .unwrap();
    let (_, predicate) = definition.split_once(" WHERE ").unwrap();
    predicate
        .split('\'')
        .skip(1)
        .step_by(2)
        // `branch <> ''` of the open-branch index is no list member.
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn names(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(ToString::to_string).collect()
}

#[tokio::test]
async fn index_predicates_of_the_open_run_index_list_the_terminal_states() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;

    assert_eq!(
        predicate_values(&pool, "runs_one_active_per_task_idx").await,
        names(RunState::TERMINAL)
    );
}

#[tokio::test]
async fn index_predicates_of_the_open_branch_index_list_the_done_statuses() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;

    let done: Vec<&str> = TaskStatus::DONE.iter().map(TaskStatus::as_str).collect();
    assert_eq!(
        predicate_values(&pool, "tasks_open_branch").await,
        names(&done)
    );
}

#[tokio::test]
async fn index_predicates_of_the_pending_request_index_name_the_pending_state() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;

    assert_eq!(
        predicate_values(&pool, "agent_requests_pending_idx").await,
        names(&[RequestState::Pending.as_str()])
    );
}
