//! Tests of forwarding turn interrupts (`specs/run-orchestration/spec.md`, "Interrupting a turn
//! from the control plane").

use super::tests::{
    hello, msg, phase, run_of_running_task, run_session_with_output, sent_prompt_ids, session_for,
    state_and_status, turn_ended, turn_started,
};
use super::*;
use crate::testing::Env;

/// The `(by, turn_id)` of the interrupts sent.
fn sent_interrupts(
    rx: &mut mpsc::Receiver<Result<ControlMessage, Status>>,
) -> Vec<(String, String)> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|m| m.ok().and_then(|c| c.body))
        .filter_map(|body| match body {
            control_message::Body::Interrupt(interrupt) => Some((interrupt.by, interrupt.turn_id)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn an_interrupt_is_sent_only_while_a_turn_is_open() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let run_id = env.running_run().await;
    let (mut session, mut rx) = session_for(&env, run_id).await;
    // The assignment is out, so no prompt is pending that would make the session busy.
    assert!(session.send_new_prompts().await.unwrap());
    sent_prompt_ids(&mut rx);
    session.busy = false;

    // No turn: nothing is sent and nothing is kept for later.
    assert!(session.interrupt_turn(run_id, "Ada", "turn_1").await);
    assert!(sent_interrupts(&mut rx).is_empty());

    session
        .handle_batch(vec![msg(1, turn_started())])
        .await
        .unwrap();
    // Another run's interrupt is not ours.
    assert!(
        session
            .interrupt_turn(df_core::RunId::new(), "Ada", "turn_1")
            .await
    );
    assert!(sent_interrupts(&mut rx).is_empty());
    assert!(session.interrupt_turn(run_id, "Ada", "turn_1").await);
    assert_eq!(
        sent_interrupts(&mut rx),
        [("Ada".to_string(), "turn_1".to_string())]
    );

    session
        .handle_batch(vec![msg(2, turn_ended())])
        .await
        .unwrap();
    assert!(session.interrupt_turn(run_id, "Ada", "turn_1").await);
    assert!(sent_interrupts(&mut rx).is_empty());
}

#[tokio::test]
async fn an_interrupt_published_while_no_session_listens_never_reaches_a_later_turn() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.running_run().await;
    publish(
        &pool,
        &Notification::RunInterrupt {
            run_id,
            by: "Ada".into(),
            turn_id: "turn_1".into(),
        },
    )
    .await
    .unwrap();

    // The session connects afterwards with a turn in progress.
    let (result, sent) =
        run_session_with_output(&env, run_id, vec![hello(), msg(1, turn_started())]).await;

    result.unwrap();
    assert!(
        !sent
            .iter()
            .any(|m| matches!(m.body, Some(control_message::Body::Interrupt(_)))),
        "{sent:?}"
    );
}

#[tokio::test]
async fn the_running_phase_of_a_fresh_run_makes_it_running_and_open_to_an_interrupt() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let run_id = run_of_running_task(&env, "preparing").await;
    let (mut session, mut rx) = session_for(&env, run_id).await;
    assert!(session.send_new_prompts().await.unwrap());
    sent_prompt_ids(&mut rx);

    session
        .handle_batch(vec![
            msg(1, phase(RunPhase::Running)),
            msg(2, turn_started()),
        ])
        .await
        .unwrap();

    assert_eq!(state_and_status(&env, run_id).await.0, RunState::Running);
    assert!(session.interrupt_turn(run_id, "Ada", "turn_1").await);
    assert_eq!(
        sent_interrupts(&mut rx),
        [("Ada".to_string(), "turn_1".to_string())]
    );
}
