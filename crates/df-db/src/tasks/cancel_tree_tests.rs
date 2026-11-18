//! Tests of cancelling a subtree (`task-tree` "Cancelling a subtree").

use std::time::Duration;

use df_core::TaskStatus;
use df_core::run_transition::RunEnd;

use super::test_support::child_ended_messages;
use super::tests::{audit_count, human, new_task, status_of};
use super::tree_tests::{Env, env};
use super::*;

async fn stored(env: &Env, task: &TaskRow) -> TaskStatus {
    status_of(&env.pool, task.id).await
}

/// A `running` run of the task; `true` once a stop was requested on it.
async fn running_run(env: &Env, task: &TaskRow) -> RunId {
    let id = RunId::new();
    sqlx::query(
        "INSERT INTO runs (id, task_id, number, state, pod_ip) VALUES ($1, $2, 1, 'running', '10.0.0.1')",
    )
    .bind(id.as_uuid())
    .bind(task.id.as_uuid())
    .execute(&env.pool)
    .await
    .unwrap();
    id
}

async fn stop_requested(env: &Env, run: RunId) -> bool {
    sqlx::query_scalar("SELECT stop_requested_at IS NOT NULL FROM runs WHERE id = $1")
        .bind(run.as_uuid())
        .fetch_one(&env.pool)
        .await
        .unwrap()
}

async fn cancel(env: &Env, task: &TaskRow) -> Result<(), DbError> {
    cancel_task(&env.pool, task.id, human()).await
}

// Scenario "Feature cancelled".
#[tokio::test]
async fn cancelling_a_task_cancels_its_open_descendants_and_stops_their_runs() {
    let env = env().await;
    let feature = env.task(None, None, None).await.unwrap();
    let running = env.task(Some(feature.id), None, None).await.unwrap();
    let waiting = env.task(Some(feature.id), None, None).await.unwrap();
    let grandchild = env.task(Some(running.id), None, None).await.unwrap();
    env.set_status(&waiting, TaskStatus::AwaitingDependencies)
        .await;
    let run = running_run(&env, &running).await;

    cancel(&env, &feature).await.unwrap();

    for task in [&feature, &running, &waiting, &grandchild] {
        assert_eq!(
            stored(&env, task).await,
            TaskStatus::Cancelled,
            "{}",
            task.key
        );
    }
    assert!(stop_requested(&env, run).await);
    assert_eq!(audit_count(&env.pool, "task.cancel").await, 4);
    assert!(child_ended_messages(&env.pool, feature.id).await.is_empty());
    assert!(child_ended_messages(&env.pool, running.id).await.is_empty());
}

#[tokio::test]
async fn descendants_are_audited_with_the_task_that_took_them_along() {
    let env = env().await;
    let feature = env.task(None, None, None).await.unwrap();
    let child = env.task(Some(feature.id), None, None).await.unwrap();

    cancel(&env, &feature).await.unwrap();

    let via = |task: &TaskRow| {
        let (pool, id) = (env.pool.clone(), task.id.to_string());
        async move {
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT details ->> 'via' FROM audit_log
                 WHERE action = 'task.cancel' AND resource_id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    assert_eq!(via(&feature).await, None);
    assert_eq!(via(&child).await.as_deref(), Some("DF-1"));
}

// The parent of the cancelled task is woken as for any cancel.
#[tokio::test]
async fn the_parent_of_the_cancelled_task_is_woken_once() {
    let env = env().await;
    let parent = env.task(None, None, None).await.unwrap();
    let feature = env.task(Some(parent.id), None, None).await.unwrap();
    env.task(Some(feature.id), None, None).await.unwrap();
    env.set_status(&parent, TaskStatus::Waiting).await;

    cancel(&env, &feature).await.unwrap();

    let woken = child_ended_messages(&env.pool, parent.id).await;
    assert_eq!(woken.len(), 1, "{woken:?}");
    assert!(woken[0].contains("DF-2") && woken[0].contains("was cancelled"));
}

// Scenario "Done subtask stays done".
#[tokio::test]
async fn finished_descendants_are_left_alone() {
    let env = env().await;
    let feature = env.task(None, None, None).await.unwrap();
    let done = env.task(Some(feature.id), None, None).await.unwrap();
    let failed = env.task(Some(feature.id), None, None).await.unwrap();
    let cancelled = env.task(Some(feature.id), None, None).await.unwrap();
    env.set_status(&done, TaskStatus::Completed).await;
    env.set_status(&failed, TaskStatus::Failed).await;
    env.set_status(&cancelled, TaskStatus::Cancelled).await;

    cancel(&env, &feature).await.unwrap();

    assert_eq!(stored(&env, &done).await, TaskStatus::Completed);
    assert_eq!(stored(&env, &failed).await, TaskStatus::Failed);
    assert_eq!(audit_count(&env.pool, "task.cancel").await, 1);
}

#[tokio::test]
async fn an_open_task_below_a_done_one_is_cancelled_too() {
    let env = env().await;
    let feature = env.task(None, None, None).await.unwrap();
    let done = env.task(Some(feature.id), None, None).await.unwrap();
    let below = env.task(Some(done.id), None, None).await.unwrap();
    env.set_status(&done, TaskStatus::Completed).await;

    cancel(&env, &feature).await.unwrap();

    assert_eq!(stored(&env, &done).await, TaskStatus::Completed);
    assert_eq!(stored(&env, &below).await, TaskStatus::Cancelled);
}

// Scenario "Dependent outside the subtree".
#[tokio::test]
async fn a_dependent_outside_the_subtree_keeps_waiting() {
    let env = env().await;
    let feature = env.task(None, None, None).await.unwrap();
    let child = env.task(Some(feature.id), None, None).await.unwrap();
    let mut input = new_task(env.project, env.persona, Some("Dependent"));
    input.depends_on = vec![NewDependency {
        key: child.key.clone(),
        kind: DependencyKind::Done,
    }];
    let (dependent, _) = create_task(&env.pool, input).await.unwrap();

    cancel(&env, &feature).await.unwrap();

    assert_eq!(
        stored(&env, &dependent).await,
        TaskStatus::AwaitingDependencies
    );
}

#[tokio::test]
async fn a_final_root_is_rejected_and_its_descendants_stay() {
    let env = env().await;
    let feature = env.task(None, None, None).await.unwrap();
    let child = env.task(Some(feature.id), None, None).await.unwrap();
    env.set_status(&feature, TaskStatus::Completed).await;

    let err = cancel(&env, &feature).await.unwrap_err();

    assert!(matches!(err, DbError::Conflict(_)), "{err:?}");
    assert_ne!(stored(&env, &child).await, TaskStatus::Cancelled);
}

// A run end of a descendant races with the cancel of the subtree: both orders finish.
#[tokio::test]
async fn a_run_end_of_a_descendant_and_the_subtree_cancel_do_not_deadlock() {
    let env = env().await;
    let env = &env;
    for round in 0..6 {
        let feature = env.task(None, None, None).await.unwrap();
        let child = env.task(Some(feature.id), None, None).await.unwrap();
        let grandchild = env.task(Some(child.id), None, None).await.unwrap();
        let run = running_run(env, &grandchild).await;
        let end = RunEnd::failure(run, df_core::RunOutcome::Failed, "boom");
        let run_env = RunEnv::none();

        let cancelling = cancel(env, &feature);
        let ending = async {
            // Alternate which side starts first.
            if round % 2 == 0 {
                tokio::task::yield_now().await;
            }
            crate::runs::end_run(&env.pool, &end, &run_env).await
        };
        let (cancelled, ended) = tokio::time::timeout(Duration::from_secs(20), async {
            tokio::join!(cancelling, ending)
        })
        .await
        .expect("no deadlock");

        cancelled.unwrap();
        ended.unwrap();
        assert_eq!(stored(env, &feature).await, TaskStatus::Cancelled);
        // Whichever came first: a descendant whose run ended first stays failed.
        let below = stored(env, &grandchild).await;
        assert!(
            matches!(below, TaskStatus::Cancelled | TaskStatus::Failed),
            "round {round}: {below}"
        );
        assert!(child_ended_messages(&env.pool, feature.id).await.is_empty());
    }
}
