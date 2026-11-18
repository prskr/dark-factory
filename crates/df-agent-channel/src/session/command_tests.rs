//! Tests of the commands a session sends for a `finalizing` run after a stop.

use super::tests::{hello, run_of_running_task, run_session_with_output, session_for};
use super::*;
use crate::testing::Env;

/// Releases an idle run through a session and returns it with its output; `stop` then acts on
/// the task, and the commands the session sent after a reconcile are returned.
async fn commands_after_a_release_and(
    stop: impl AsyncFnOnce(&Env, &sqlx::PgPool),
) -> Vec<control_message::Body> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = run_of_running_task(&env, "awaiting_input").await;
    sqlx::query("UPDATE task_messages SET delivered_at = now() WHERE task_id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let (mut session, mut rx) = session_for(&env, run_id).await;

    assert!(session.release_if_idle().await.unwrap());
    stop(&env, &pool).await;
    assert!(session.reconcile_finalizing().await.unwrap());
    assert!(session.reconcile_finalizing().await.unwrap());

    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|m| m.ok().and_then(|c| c.body))
        .collect()
}

#[tokio::test]
async fn cancelling_the_task_of_a_released_run_sends_cancel_after_the_release() {
    let sent = commands_after_a_release_and(async |env, pool| {
        df_db::cancel_task(pool, env.task.id, env.human())
            .await
            .unwrap();
    })
    .await;

    assert!(
        matches!(
            sent.as_slice(),
            [
                control_message::Body::Release(_),
                control_message::Body::Cancel(_)
            ]
        ),
        "{sent:?}"
    );
}

#[tokio::test]
async fn marking_the_task_of_a_released_run_done_sends_one_cancel_after_the_release() {
    let sent = commands_after_a_release_and(async |env, pool| {
        // The fixture's task is `running`; an idle one is `waiting`.
        sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
            .bind(env.task.id.as_uuid())
            .execute(pool)
            .await
            .unwrap();
        df_db::mark_task_done(pool, env.task.id, env.human())
            .await
            .unwrap();
    })
    .await;

    assert!(
        matches!(
            sent.as_slice(),
            [control_message::Body::Release(_), control_message::Body::Cancel(c)]
                if c.reason == "task marked done"
        ),
        "{sent:?}"
    );
}

#[tokio::test]
async fn a_board_stop_of_a_released_run_sends_nothing_after_the_release() {
    // A stop leaves a `finalizing` run alone; only cancelling the task marks it.
    let sent = commands_after_a_release_and(async |env, pool| {
        df_db::stop_run(pool, env.task.id, env.human())
            .await
            .unwrap();
    })
    .await;

    assert!(
        matches!(sent.as_slice(), [control_message::Body::Release(_)]),
        "{sent:?}"
    );
}

#[tokio::test]
async fn the_budget_sends_cancel_on_the_stream_that_sent_the_release() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = run_of_running_task(&env, "awaiting_input").await;
    sqlx::query("UPDATE task_messages SET delivered_at = now() WHERE task_id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let (mut session, mut rx) = session_for(&env, run_id).await;

    assert!(session.release_if_idle().await.unwrap());
    assert!(
        session
            .start_finalizing(FinalizeReason::Budget)
            .await
            .unwrap()
    );
    assert!(session.reconcile_finalizing().await.unwrap());
    assert!(session.reconcile_finalizing().await.unwrap());

    let sent: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|m| m.ok().and_then(|c| c.body))
        .collect();
    assert!(
        matches!(
            sent.as_slice(),
            [
                control_message::Body::Release(_),
                control_message::Body::Cancel(_)
            ]
        ),
        "{sent:?}"
    );
}

#[tokio::test]
async fn a_stream_after_the_budget_deadline_cancels_a_run_released_before() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    env.ctx.orch.config = Arc::new(OrchestratorConfig {
        runs: df_core::config::RunsConfig {
            max_wall_clock_seconds: 0,
            cancel_grace_seconds: 0,
            ..Default::default()
        },
        ..Default::default()
    });
    let run_id = run_of_running_task(&env, "finalizing").await;
    sqlx::query("UPDATE runs SET finalize_reason = 'idle' WHERE id = $1")
        .bind(run_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let (result, sent) = run_session_with_output(&env, run_id, vec![hello()]).await;
    result.unwrap();

    let commands: Vec<_> = sent
        .into_iter()
        .filter_map(|c| c.body)
        .filter(|b| {
            matches!(
                b,
                control_message::Body::Release(_) | control_message::Body::Cancel(_)
            )
        })
        .collect();
    assert!(
        matches!(commands.as_slice(), [control_message::Body::Cancel(_)]),
        "{commands:?}"
    );
}

#[tokio::test]
async fn a_run_stopped_before_it_connected_gets_one_stop_cancel_when_its_stream_starts() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = run_of_running_task(&env, "connecting").await;
    sqlx::query("UPDATE runs SET stop_requested_at = now() WHERE id = $1")
        .bind(run_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    let (result, sent) = run_session_with_output(&env, run_id, vec![hello()]).await;
    result.unwrap();

    let cancels: Vec<_> = sent
        .into_iter()
        .filter_map(|c| match c.body {
            Some(control_message::Body::Cancel(cancel)) => Some(cancel.reason),
            _ => None,
        })
        .collect();
    let RunCommand::Cancel(expected) = FinalizeReason::Stop.command() else {
        panic!("a stop sends a Cancel");
    };
    assert_eq!(cancels, [expected]);
    let stored = get_run(&pool, run_id).await.unwrap().unwrap();
    assert_eq!(stored.finalize_reason, Some(FinalizeReason::Stop));
}

#[tokio::test]
async fn a_run_finalizing_for_a_stop_or_a_done_task_gets_one_cancel_with_its_own_reason_after_the_budget()
 {
    for reason in [FinalizeReason::Stop, FinalizeReason::Done] {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let mut env = Env::new(pool.clone()).await;
        env.ctx.orch.config = Arc::new(OrchestratorConfig {
            runs: df_core::config::RunsConfig {
                max_wall_clock_seconds: 0,
                cancel_grace_seconds: 0,
                ..Default::default()
            },
            ..Default::default()
        });
        let run_id = run_of_running_task(&env, "finalizing").await;
        sqlx::query("UPDATE runs SET finalize_reason = $2 WHERE id = $1")
            .bind(run_id.as_uuid())
            .bind(reason.as_str())
            .execute(&pool)
            .await
            .unwrap();

        let (result, sent) = run_session_with_output(&env, run_id, vec![hello()]).await;
        result.unwrap();

        let cancels: Vec<_> = sent
            .into_iter()
            .filter_map(|c| match c.body {
                Some(control_message::Body::Cancel(cancel)) => Some(cancel.reason),
                _ => None,
            })
            .collect();
        let RunCommand::Cancel(expected) = reason.command() else {
            panic!("{reason} sends a Cancel");
        };
        assert_eq!(cancels, [expected], "{reason}");
        let stored = get_run(&pool, run_id).await.unwrap().unwrap();
        assert_eq!(stored.finalize_reason, Some(reason), "{reason}");
    }
}
