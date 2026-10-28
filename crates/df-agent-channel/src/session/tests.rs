//! Tests of the agent session.

use super::*;
use crate::metrics::probe::Probe;
use crate::testing::Env;
use df_core::testing::SandboxCall;
use df_proto::agent::v1::{PushStatus, PushTarget, RuntimeOperation, RuntimeTiming};
use df_proto::transcript::v1 as wire;

fn msg(seq: u64, body: agent_message::Body) -> AgentMessage {
    AgentMessage {
        seq,
        body: Some(body),
    }
}

fn hello() -> AgentMessage {
    msg(
        0,
        agent_message::Body::Hello(df_proto::agent::v1::Hello::default()),
    )
}

fn text_event() -> agent_message::Body {
    agent_message::Body::TranscriptEvent(wire::Event {
        schema_version: 1,
        occurred_at: None,
        turn_id: "t".into(),
        kind: Some(wire::event::Kind::AgentMessageChunk(
            wire::AgentMessageChunk {
                message_id: "m".into(),
                content: Some(wire::ContentBlock {
                    kind: Some(wire::content_block::Kind::Text(wire::TextContent {
                        text: "x".into(),
                    })),
                    hidden: false,
                }),
            },
        )),
    })
}

fn report(outcome: WireOutcome) -> agent_message::Body {
    agent_message::Body::RunReport(RunReport {
        outcome: outcome as i32,
        ..RunReport::default()
    })
}

/// Runs a session over a fixed list of inbound messages that are all available at once, so
/// they form a single batch.
async fn run_session(
    env: &Env,
    run_id: df_core::RunId,
    messages: Vec<AgentMessage>,
) -> Result<(), Status> {
    run_session_with_output(env, run_id, messages).await.0
}

/// Like [`run_session`], also returning what the session sent.
async fn run_session_with_output(
    env: &Env,
    run_id: df_core::RunId,
    messages: Vec<AgentMessage>,
) -> (Result<(), Status>, Vec<ControlMessage>) {
    let row = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let inbound = futures::stream::iter(messages.into_iter().map(Ok));
    let result = run(env.ctx.clone(), row, inbound, tx, never_stops()).await;
    let sent = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(Result::ok)
        .collect();
    (result, sent)
}

fn never_stops() -> watch::Receiver<bool> {
    watch::channel(false).1
}

#[tokio::test]
async fn effects_before_a_sequence_gap_still_happen() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.running_run().await;

    // RunReport (seq 1) and a message after a gap (seq 3) arrive in one batch.
    let result = run_session(
        &env,
        run_id,
        vec![
            hello(),
            msg(1, report(WireOutcome::EndTurn)),
            msg(3, text_event()),
        ],
    )
    .await;

    assert_eq!(result.unwrap_err().code(), tonic::Code::DataLoss);
    let run = get_run(&pool, run_id).await.unwrap().unwrap();
    assert_eq!(run.state, RunState::Succeeded);
    assert_eq!(run.last_persisted_seq, 1);
    assert!(
        env.provider
            .calls()
            .contains(&SandboxCall::Teardown(run_id))
    );
}

#[tokio::test]
async fn transcript_writes_wait_for_the_task_lock() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.running_run().await;
    let row = get_run(&pool, run_id).await.unwrap().unwrap();
    let rc = load_run_context(&pool, row).await.unwrap();

    // A late batch of another run of the same task (e.g. a run marked lost while its sandbox
    // still sends) has taken an event id but not committed yet. If this run's batch committed
    // first, a reader already past its id would never see the other one.
    let earlier = env.insert_run(env.task.id, 2, "failed").await;
    let mut other = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO transcript_events (task_id, run_id, seq, occurred_at, kind, payload)
             VALUES ($1, $2, 1, now(), 'agent_message_chunk', '{}')",
    )
    .bind(env.task.id.as_uuid())
    .bind(earlier.as_uuid())
    .execute(&mut *other)
    .await
    .unwrap();
    let ctx = env.ctx.clone();
    let write =
        tokio::spawn(async move { persist_batch(&ctx, &rc, vec![msg(1, text_event())]).await });

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity
                     WHERE datname = current_database() AND wait_event_type = 'Lock'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiting > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the batch did not wait for the task lock");
    assert!(!write.is_finished());

    other.commit().await.unwrap();
    write.await.unwrap().unwrap();
    let ids: Vec<(i64, uuid::Uuid)> =
        sqlx::query_as("SELECT id, run_id FROM transcript_events ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    let runs: Vec<_> = ids.iter().map(|(_, run)| *run).collect();
    assert_eq!(runs, [earlier.as_uuid(), run_id.as_uuid()]);
}

#[tokio::test]
async fn tool_calls_finish_after_the_stream_ended() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.running_run().await;
    let call = ToolCallRequest {
        call_id: uuid::Uuid::now_v7().to_string(),
        tool_name: "report_progress".into(),
        arguments_json: r#"{"summary":"almost done"}"#.into(),
    };

    // The stream ends right after the request, as when the runtime closes after RunReport.
    run_session(
        &env,
        run_id,
        vec![hello(), msg(1, agent_message::Body::ToolCallRequest(call))],
    )
    .await
    .unwrap();

    let line: String = sqlx::query_scalar("SELECT status_line FROM tasks WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(line, "almost done");
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM tool_calls WHERE result IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, 1);
}

fn phase(phase: RunPhase) -> agent_message::Body {
    agent_message::Body::PhaseChanged(df_proto::agent::v1::PhaseChanged {
        phase: phase as i32,
        detail: String::new(),
    })
}

async fn state_and_status(env: &Env, run_id: df_core::RunId) -> (RunState, String) {
    let run = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
    let status = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    (run.state, status)
}

/// A run in `state` of a task that is `running`, without undelivered messages.
async fn run_of_running_task(env: &Env, state: &str) -> df_core::RunId {
    let run_id = env.insert_run(env.task.id, 1, state).await;
    env.bind_pod(run_id);
    sqlx::query("UPDATE tasks SET status = 'running' WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE task_messages SET delivered_at = now() WHERE task_id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    run_id
}

#[tokio::test]
async fn the_idle_phase_makes_the_run_awaiting_input_and_the_task_waiting() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = run_of_running_task(&env, "running").await;

    run_session(
        &env,
        run_id,
        vec![hello(), msg(1, phase(RunPhase::AwaitingInput))],
    )
    .await
    .unwrap();
    assert_eq!(
        state_and_status(&env, run_id).await,
        (RunState::AwaitingInput, "waiting".to_string())
    );

    run_session(
        &env,
        run_id,
        vec![hello(), msg(2, phase(RunPhase::Running))],
    )
    .await
    .unwrap();
    assert_eq!(
        state_and_status(&env, run_id).await,
        (RunState::Running, "running".to_string())
    );
}

#[tokio::test]
async fn the_idle_phase_keeps_the_task_running_while_a_message_is_undelivered() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = run_of_running_task(&env, "running").await;
    let prompt_id = queue_human(&env, "one more thing").await;

    let (result, sent) = run_session_with_output(
        &env,
        run_id,
        vec![hello(), msg(1, phase(RunPhase::AwaitingInput))],
    )
    .await;
    result.unwrap();
    assert_eq!(
        state_and_status(&env, run_id).await,
        (RunState::AwaitingInput, "running".to_string())
    );
    // The session forwards the message to the idle run.
    let prompts: Vec<_> = sent
        .into_iter()
        .filter_map(|c| match c.body {
            Some(control_message::Body::Prompt(p)) => Some(p.prompt_id),
            _ => None,
        })
        .collect();
    assert_eq!(prompts, [prompt_id]);
}

#[tokio::test]
async fn idle_phases_do_not_reopen_a_finalizing_run() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = run_of_running_task(&env, "finalizing").await;
    sqlx::query("UPDATE tasks SET status = 'completed' WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();

    run_session(
        &env,
        run_id,
        vec![hello(), msg(1, phase(RunPhase::Running))],
    )
    .await
    .unwrap();
    assert_eq!(
        state_and_status(&env, run_id).await,
        (RunState::Finalizing, "completed".to_string())
    );
}

/// Persists a `TurnEnded` of the run that occurred at `at`.
async fn persist_turn_end(env: &Env, run_id: df_core::RunId, at: DateTime<Utc>) {
    sqlx::query(
        "INSERT INTO transcript_events (task_id, run_id, seq, occurred_at, kind, payload)
             VALUES ($1, $2, 1, $3, 'turn_ended', '{}')",
    )
    .bind(env.task.id.as_uuid())
    .bind(run_id.as_uuid())
    .bind(at)
    .execute(&env.ctx.pool)
    .await
    .unwrap();
}

fn with_keep_alive(env: &mut Env, seconds: u64) {
    env.ctx.config = Arc::new(crate::ChannelConfig {
        runs: df_core::config::RunsConfig {
            keep_alive_seconds: seconds,
            ..Default::default()
        },
        ..Default::default()
    });
}

/// Serves `run_id` on a stream that stays open and returns the time the first control
/// message accepted by `wanted` was sent, with the run state at that moment. Fails if none
/// comes within 30 s.
async fn serve_until(
    env: &Env,
    run_id: df_core::RunId,
    wanted: impl Fn(&control_message::Body) -> bool,
) -> (DateTime<Utc>, RunState) {
    let row = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
    let (to_session, inbound) = mpsc::channel(8);
    let (tx, mut rx) = mpsc::channel(64);
    to_session.send(Ok(hello())).await.unwrap();
    let ctx = env.ctx.clone();
    let session = tokio::spawn(async move {
        run(
            ctx,
            row,
            tokio_stream::wrappers::ReceiverStream::new(inbound),
            tx,
            never_stops(),
        )
        .await
    });
    let found = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while let Some(Ok(control)) = rx.recv().await {
            if control.body.as_ref().is_some_and(&wanted) {
                return Some(Utc::now());
            }
        }
        None
    })
    .await
    .expect("the command in time")
    .expect("the stream ended without the command");
    let (state, _) = state_and_status(env, run_id).await;
    drop(to_session);
    session.await.unwrap().unwrap();
    (found, state)
}

/// Serves an idle run whose turn ended at `turn_end` and returns when `Release` was sent,
/// with the run state at that moment. The deadline is wall-clock (a persisted timestamp plus
/// the keep-alive), so these tests wait in real time; the keep-alive is at most 1 s.
async fn wait_for_release(
    keep_alive: u64,
    turn_end: impl FnOnce() -> DateTime<Utc>,
) -> (DateTime<Utc>, RunState) {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    with_keep_alive(&mut env, keep_alive);
    let run_id = run_of_running_task(&env, "awaiting_input").await;
    persist_turn_end(&env, run_id, turn_end()).await;
    serve_until(&env, run_id, |b| {
        matches!(b, control_message::Body::Release(_))
    })
    .await
}

/// `persist_batch` holds the run row and then wants the task row; `start_finalizing` must
/// wait for the run row first instead of holding the task row, or both deadlock.
#[tokio::test]
async fn start_finalizing_takes_the_run_lock_before_the_task_lock() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = run_of_running_task(&env, "awaiting_input").await;
    let (session, _rx) = session_for(&env, run_id).await;
    let mut other = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM runs WHERE id = $1 FOR UPDATE")
        .bind(run_id.as_uuid())
        .execute(&mut *other)
        .await
        .unwrap();
    let task_id = env.task.id;
    let finalizing = tokio::spawn(async move { session.start_finalizing(false).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity
                     WHERE datname = current_database() AND wait_event_type = 'Lock'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiting > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("start_finalizing did not wait for the run lock");

    // Without the run lock first, `start_finalizing` now holds the task row: deadlock.
    lock_task(&mut other, task_id).await.unwrap();
    other.commit().await.unwrap();
    assert!(finalizing.await.unwrap().unwrap());
}

#[tokio::test]
async fn a_budget_cancel_is_never_followed_by_a_release() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    env.ctx.config = Arc::new(crate::ChannelConfig {
        runs: df_core::config::RunsConfig {
            max_wall_clock_seconds: 0,
            cancel_grace_seconds: 0,
            ..Default::default()
        },
        ..Default::default()
    });
    let run_id = run_of_running_task(&env, "awaiting_input").await;
    persist_turn_end(&env, run_id, Utc::now()).await;
    let (mut session, mut rx) = session_for(&env, run_id).await;

    // The session that sent the budget `Cancel` sees its own `finalizing` run.
    assert!(session.start_finalizing(false).await.unwrap());
    assert!(session.reconcile_finalizing().await.unwrap());
    // A replica that takes over later sees the same.
    let (mut other, mut other_rx) = session_for(&env, run_id).await;
    assert!(other.reconcile_finalizing().await.unwrap());

    assert!(rx.try_recv().is_err() && other_rx.try_recv().is_err());
}

#[tokio::test]
async fn the_budget_cancel_finalizes_the_run_first_and_no_prompt_follows() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    env.ctx.config = Arc::new(crate::ChannelConfig {
        runs: df_core::config::RunsConfig {
            max_wall_clock_seconds: 0,
            cancel_grace_seconds: 0,
            ..Default::default()
        },
        ..Default::default()
    });
    let run_id = run_of_running_task(&env, "running").await;

    // The run is `finalizing` when `Cancel` goes out.
    let (_, state) = serve_until(&env, run_id, |b| {
        matches!(b, control_message::Body::Cancel(_))
    })
    .await;
    assert_eq!(state, RunState::Finalizing);

    // A message queued afterwards is not forwarded to the cancelled run.
    let (mut session, mut rx) = session_for(&env, run_id).await;
    queue_human(&env, "too late").await;
    assert!(session.send_new_prompts().await.unwrap());
    assert!(sent_prompt_ids(&mut rx).is_empty());
    assert_eq!(state_and_status(&env, run_id).await.1, "queued");
}

#[tokio::test]
async fn the_idle_run_is_released_one_keep_alive_after_its_turn_end() {
    let turn_end = Utc::now();
    let (released_at, state) = wait_for_release(1, || turn_end).await;
    assert!(released_at - turn_end >= chrono::Duration::seconds(1));
    // `finalizing` is set before `Release` is sent.
    assert_eq!(state, RunState::Finalizing);
}

#[tokio::test]
async fn a_keep_alive_of_zero_releases_right_after_the_turn() {
    let turn_end = Utc::now();
    let (released_at, state) = wait_for_release(0, || turn_end).await;
    assert!(released_at - turn_end < chrono::Duration::seconds(30));
    assert_eq!(state, RunState::Finalizing);
}

#[tokio::test]
async fn a_new_session_after_the_deadline_releases_after_the_handshake() {
    let (_, state) = wait_for_release(600, || Utc::now() - chrono::Duration::seconds(3600)).await;
    assert_eq!(state, RunState::Finalizing);
}

#[tokio::test]
async fn a_run_with_a_message_in_flight_is_not_released() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool.clone()).await;
    with_keep_alive(&mut env, 0);
    let run_id = run_of_running_task(&env, "awaiting_input").await;
    persist_turn_end(&env, run_id, Utc::now()).await;
    let (mut session, mut rx) = session_for(&env, run_id).await;

    // A prompt was forwarded and its turn has not been confirmed yet.
    session.busy = true;
    session.refresh_idle().await.unwrap();
    assert!(session.idle_at.is_none());

    // The message was queued after the last forwarding: the deadline does not release.
    session.busy = false;
    queue_human(&env, "wait for me").await;
    assert!(session.release_if_idle().await.unwrap());
    assert!(rx.try_recv().is_err());
    assert_eq!(
        state_and_status(&env, run_id).await.0,
        RunState::AwaitingInput
    );
}

#[tokio::test]
async fn a_message_for_an_idle_run_starts_no_run_and_is_forwarded_at_once() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = run_of_running_task(&env, "awaiting_input").await;
    sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let (mut session, mut rx) = session_for(&env, run_id).await;

    let prompt_id = queue_human(&env, "and one more thing").await;

    assert_eq!(state_and_status(&env, run_id).await.1, "running");
    assert!(
        crate::runs::start_queued_runs(&pool, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(session.send_new_prompts().await.unwrap());
    assert_eq!(sent_prompt_ids(&mut rx), [prompt_id]);
}

#[tokio::test]
async fn marking_the_task_done_cancels_its_finalizing_run_once() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = run_of_running_task(&env, "awaiting_input").await;
    sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let (mut session, mut rx) = session_for(&env, run_id).await;

    // Nothing to do while the run is idle.
    assert!(session.reconcile_finalizing().await.unwrap());
    assert!(rx.try_recv().is_err());

    df_db::mark_task_done(
        &pool,
        env.task.id,
        df_core::Principal::human(df_core::UserId::new(), "Ada"),
    )
    .await
    .unwrap();
    assert!(session.reconcile_finalizing().await.unwrap());
    assert!(session.reconcile_finalizing().await.unwrap());

    let sent: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|m| m.ok().and_then(|c| c.body))
        .collect();
    assert!(
        matches!(sent.as_slice(), [control_message::Body::Cancel(_)]),
        "{sent:?}"
    );
}

#[tokio::test]
async fn no_prompt_is_sent_to_a_finalizing_run_and_its_end_queues_the_task() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = run_of_running_task(&env, "finalizing").await;
    let (mut session, mut rx) = session_for(&env, run_id).await;

    queue_human(&env, "too late").await;
    assert!(session.send_new_prompts().await.unwrap());
    assert!(sent_prompt_ids(&mut rx).is_empty());

    let end = RunEnd::failure(run_id, RunOutcome::EndTurn, "");
    crate::runs::end_run(&pool, &end).await.unwrap();
    assert_eq!(state_and_status(&env, run_id).await.1, "queued");
}

async fn session_for(
    env: &Env,
    run_id: df_core::RunId,
) -> (Session, mpsc::Receiver<Result<ControlMessage, Status>>) {
    let row = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
    let rc = load_run_context(&env.ctx.pool, row).await.unwrap();
    let (tx, rx) = mpsc::channel(64);
    let session = Session {
        ctx: env.ctx.clone(),
        rc: Arc::new(rc),
        out: tx,
        sent_prompts: HashSet::new(),
        busy: false,
        tasks: JoinSet::new(),
        ended: false,
        idle_at: None,
        done_cancel_sent: false,
        release_sent: false,
    };
    (session, rx)
}

#[tokio::test]
async fn a_hanging_forge_does_not_block_the_stream() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let run_id = env.running_run().await;
    env.forge.hang_find_open_pull_request();
    let (mut session, _rx) = session_for(&env, run_id).await;
    let push = agent_message::Body::PushReport(PushReport {
        target: df_proto::agent::v1::PushTarget::TaskBranch as i32,
        status: df_proto::agent::v1::PushStatus::Ok as i32,
        commits_ahead_of_base: 1,
        ..PushReport::default()
    });

    let handled = tokio::time::timeout(
        Duration::from_secs(10),
        session.handle_batch(vec![msg(1, push)]),
    )
    .await;

    assert!(handled.is_ok(), "the stream loop waited for the forge");
}

#[tokio::test]
async fn tool_calls_and_credentials_are_refused_after_the_run_ended() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.running_run().await;
    let (mut session, mut rx) = session_for(&env, run_id).await;
    // The scheduler ended the run (hard stop); the stream is still open.
    sqlx::query("UPDATE runs SET state = 'failed' WHERE id = $1")
        .bind(run_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let call = ToolCallRequest {
        call_id: uuid::Uuid::now_v7().to_string(),
        tool_name: "report_progress".into(),
        arguments_json: r#"{"summary":"too late"}"#.into(),
    };
    let credential = CredentialRequest {
        request_id: "r1".into(),
        kind: None,
    };

    let batch = vec![
        msg(1, agent_message::Body::ToolCallRequest(call)),
        msg(2, agent_message::Body::CredentialRequest(credential)),
    ];
    assert!(session.handle_batch(batch).await.unwrap());
    session.drain().await;

    let mut refused = (false, false);
    while let Ok(Ok(control)) = rx.try_recv() {
        match control.body {
            Some(control_message::Body::ToolCallResult(r)) => refused.0 = r.is_error,
            Some(control_message::Body::CredentialGrant(g)) => {
                refused.1 = matches!(
                    g.result,
                    Some(df_proto::agent::v1::credential_grant::Result::DeniedReason(
                        _
                    ))
                );
            }
            _ => {}
        }
    }
    assert_eq!(refused, (true, true));
    let line: String = sqlx::query_scalar("SELECT status_line FROM tasks WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(line, "too late");
}

#[tokio::test]
async fn a_stream_error_before_hello_is_reported() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let run_id = env.running_run().await;
    let row = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
    let inbound = futures::stream::iter([Err(Status::out_of_range("too large"))]);
    let (tx, _rx) = mpsc::channel(1);

    let status = run(env.ctx.clone(), row, inbound, tx, never_stops())
        .await
        .unwrap_err();

    assert_eq!(status.code(), tonic::Code::ResourceExhausted);
}

#[tokio::test]
async fn a_tool_call_in_flight_at_shutdown_stores_its_result() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.running_run().await;
    sqlx::query("UPDATE tasks SET issue_number = 42 WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let forge_reply = env.forge.hold_issue_comments();
    let call = ToolCallRequest {
        call_id: uuid::Uuid::now_v7().to_string(),
        tool_name: "comment_on_issue".into(),
        arguments_json: r#"{"body":"hello"}"#.into(),
    };
    let row = get_run(&pool, run_id).await.unwrap().unwrap();
    let (inbound_tx, inbound_rx) = mpsc::channel(8);
    let (out_tx, _out_rx) = mpsc::channel(64);
    let (stop_tx, stop_rx) = watch::channel(false);
    let ctx = env.ctx.clone();
    let session = tokio::spawn(run(
        ctx,
        row,
        tokio_stream::wrappers::ReceiverStream::new(inbound_rx),
        out_tx,
        stop_rx,
    ));
    inbound_tx.send(Ok(hello())).await.unwrap();
    inbound_tx
        .send(Ok(msg(1, agent_message::Body::ToolCallRequest(call))))
        .await
        .unwrap();
    // The forge call is under way.
    while env.forge.calls().is_empty() {
        tokio::task::yield_now().await;
    }

    stop_tx.send_replace(true);
    forge_reply.add_permits(1);
    session.await.unwrap().unwrap();

    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM tool_calls WHERE result IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, 1);
}

#[tokio::test]
async fn no_prompts_are_sent_once_the_run_ended() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.running_run().await;

    // Active run: the assignment prompt goes out.
    let (mut session, mut rx) = session_for(&env, run_id).await;
    assert!(session.send_new_prompts().await.unwrap());
    assert!(rx.try_recv().is_ok());

    // Ended in this session.
    let (mut session, mut rx) = session_for(&env, run_id).await;
    session.ended = true;
    assert!(!session.send_new_prompts().await.unwrap());
    assert!(rx.try_recv().is_err());

    // Ended elsewhere (another replica): the database says so.
    sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
        .bind(run_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let (mut session, mut rx) = session_for(&env, run_id).await;
    assert!(!session.send_new_prompts().await.unwrap());
    assert!(rx.try_recv().is_err());
}
fn turn_event(kind: wire::event::Kind) -> agent_message::Body {
    agent_message::Body::TranscriptEvent(wire::Event {
        schema_version: 1,
        occurred_at: None,
        turn_id: "t".into(),
        kind: Some(kind),
    })
}

fn turn_started() -> agent_message::Body {
    turn_event(wire::event::Kind::TurnStarted(wire::TurnStarted::default()))
}

fn turn_ended() -> agent_message::Body {
    turn_event(wire::event::Kind::TurnEnded(wire::TurnEnded::default()))
}

/// Queues a board member's message and returns its prompt id.
async fn queue_human(env: &Env, text: &str) -> String {
    let content = vec![wire::ContentBlock {
        kind: Some(wire::content_block::Kind::Text(wire::TextContent {
            text: text.into(),
        })),
        hidden: false,
    }];
    let author = df_core::Principal::human(df_core::UserId::new(), "Ada");
    df_db::queue_message(&env.ctx.pool, env.task.id, author, content)
        .await
        .unwrap()
        .id
        .to_string()
}

fn sent_prompt_ids(rx: &mut mpsc::Receiver<Result<ControlMessage, Status>>) -> Vec<String> {
    let mut ids = Vec::new();
    while let Ok(Ok(control)) = rx.try_recv() {
        if let Some(control_message::Body::Prompt(p)) = control.body {
            ids.push(p.prompt_id);
        }
    }
    ids
}

async fn forwarded(env: &Env, prompt_id: &str) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT forwarded_at IS NOT NULL FROM task_messages WHERE id = $1::uuid",
    )
    .bind(prompt_id)
    .fetch_one(&env.ctx.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn busy_follows_the_persisted_turn_events_of_the_run() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.running_run().await;
    let (mut session, mut rx) = session_for(&env, run_id).await;
    assert!(!turn_open(&pool, run_id).await.unwrap());
    assert!(session.send_new_prompts().await.unwrap());
    sent_prompt_ids(&mut rx);

    session
        .handle_batch(vec![msg(1, turn_started())])
        .await
        .unwrap();
    // A reconnect during the turn derives the same state.
    assert!(session.busy);
    assert!(turn_open(&pool, run_id).await.unwrap());

    session
        .handle_batch(vec![msg(2, turn_ended())])
        .await
        .unwrap();
    assert!(!session.busy);
    assert!(!turn_open(&pool, run_id).await.unwrap());
}

#[tokio::test]
async fn a_message_queued_during_a_turn_is_sent_when_the_turn_ends() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let run_id = env.running_run().await;
    let (mut session, mut rx) = session_for(&env, run_id).await;
    assert!(session.send_new_prompts().await.unwrap());
    let assignment = sent_prompt_ids(&mut rx);
    assert_eq!(assignment.len(), 1);
    session
        .handle_batch(vec![msg(1, turn_started())])
        .await
        .unwrap();

    let queued = queue_human(&env, "also check logout").await;
    assert!(session.send_new_prompts().await.unwrap());
    assert!(sent_prompt_ids(&mut rx).is_empty());
    assert!(!forwarded(&env, &queued).await);

    session
        .handle_batch(vec![msg(2, turn_ended())])
        .await
        .unwrap();
    assert_eq!(sent_prompt_ids(&mut rx), vec![queued.clone()]);
    assert!(forwarded(&env, &queued).await);
}

#[tokio::test]
async fn a_message_queued_before_the_turn_start_is_persisted_is_held_back() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let run_id = env.running_run().await;
    let (mut session, mut rx) = session_for(&env, run_id).await;
    assert!(session.send_new_prompts().await.unwrap());
    sent_prompt_ids(&mut rx);

    // The runtime is in the turn for the assignment, its `TurnStarted` not persisted yet.
    let queued = queue_human(&env, "quick follow-up").await;
    assert!(session.send_new_prompts().await.unwrap());

    assert!(sent_prompt_ids(&mut rx).is_empty());
    assert!(!forwarded(&env, &queued).await);
}

#[tokio::test]
async fn a_message_on_an_idle_run_is_forwarded_at_once() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let run_id = env.running_run().await;
    let (mut session, mut rx) = session_for(&env, run_id).await;
    assert!(session.send_new_prompts().await.unwrap());
    sent_prompt_ids(&mut rx);
    let batch = vec![msg(1, turn_started()), msg(2, turn_ended())];
    session.handle_batch(batch).await.unwrap();

    let queued = queue_human(&env, "next").await;
    assert!(session.send_new_prompts().await.unwrap());

    assert_eq!(sent_prompt_ids(&mut rx), vec![queued.clone()]);
    assert!(forwarded(&env, &queued).await);
}

#[tokio::test]
async fn a_forwarded_undelivered_message_is_sent_again_on_reconnect() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let run_id = env.running_run().await;
    let (mut first, mut first_rx) = session_for(&env, run_id).await;
    assert!(first.send_new_prompts().await.unwrap());
    let sent = sent_prompt_ids(&mut first_rx);

    // The new stream is busy (turn open) yet repeats what was forwarded before.
    let (mut second, mut second_rx) = session_for(&env, run_id).await;
    second.busy = true;
    assert!(second.send_new_prompts().await.unwrap());

    assert_eq!(sent_prompt_ids(&mut second_rx), sent);
}

#[tokio::test]
async fn a_reconnect_before_the_turn_start_is_persisted_keeps_new_messages_held_back() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.running_run().await;
    let (mut first, mut first_rx) = session_for(&env, run_id).await;
    assert!(first.send_new_prompts().await.unwrap());
    let sent = sent_prompt_ids(&mut first_rx);
    drop(first);
    let queued = queue_human(&env, "quick follow-up").await;

    // The runtime is in the turn for the assignment; no `TurnStarted` persisted.
    let (mut second, mut second_rx) = session_for(&env, run_id).await;
    second.busy = busy_on_connect(&pool, run_id, env.task.id).await.unwrap();
    assert!(second.send_new_prompts().await.unwrap());

    assert_eq!(sent_prompt_ids(&mut second_rx), sent);
    assert!(!forwarded(&env, &queued).await);
}

#[tokio::test]
async fn a_run_that_ends_during_a_turn_keeps_the_message_held_back() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let run_id = env.running_run().await;
    let (mut session, mut rx) = session_for(&env, run_id).await;
    assert!(session.send_new_prompts().await.unwrap());
    sent_prompt_ids(&mut rx);
    let batch = vec![msg(1, turn_started()), msg(2, report(WireOutcome::EndTurn))];
    session.handle_batch(batch).await.unwrap();
    let queued = queue_human(&env, "too late").await;

    assert!(!session.send_new_prompts().await.unwrap());

    assert!(sent_prompt_ids(&mut rx).is_empty());
    assert!(!forwarded(&env, &queued).await);
}

// --- metrics (`specs/telemetry/spec.md`) ---

/// An environment whose channel records into the returned probe.
async fn metrics_env() -> (Env, Probe) {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool).await;
    let probe = Probe::new();
    env.ctx.metrics = Arc::new(crate::Metrics::new(&probe.meter()));
    (env, probe)
}

fn timing(operation: RuntimeOperation, duration_ms: u64) -> agent_message::Body {
    agent_message::Body::RuntimeTiming(RuntimeTiming {
        operation: operation as i32,
        duration_ms,
        ok: true,
    })
}

fn turn_ended_with_usage() -> agent_message::Body {
    turn_event(wire::event::Kind::TurnEnded(wire::TurnEnded {
        stop_reason: wire::StopReason::EndTurn as i32,
        usage: vec![wire::Usage {
            model_id: "claude-sonnet-5-5".into(),
            input_tokens: 100,
            output_tokens: 20,
            ..wire::Usage::default()
        }],
        ..wire::TurnEnded::default()
    }))
}

fn tool_call(status: wire::ToolCallStatus) -> agent_message::Body {
    turn_event(wire::event::Kind::ToolCall(wire::ToolCall {
        tool_call_id: "call-1".into(),
        kind: wire::ToolKind::Execute as i32,
        origin: wire::ToolOrigin::Harness as i32,
        status: status as i32,
        ..wire::ToolCall::default()
    }))
}

fn tool_call_update(status: wire::ToolCallStatus) -> agent_message::Body {
    turn_event(wire::event::Kind::ToolCallUpdate(wire::ToolCallUpdate {
        tool_call_id: "call-1".into(),
        status: Some(status as i32),
        ..wire::ToolCallUpdate::default()
    }))
}

fn push_report() -> agent_message::Body {
    agent_message::Body::PushReport(PushReport {
        target: PushTarget::TaskBranch as i32,
        status: PushStatus::Ok as i32,
        ..PushReport::default()
    })
}

#[tokio::test]
async fn a_runtime_timing_takes_its_place_in_the_sequence_without_a_row() {
    let (env, probe) = metrics_env().await;
    let run_id = env.running_run().await;

    run_session(
        &env,
        run_id,
        vec![
            hello(),
            msg(1, timing(RuntimeOperation::Turn, 1500)),
            msg(2, text_event()),
        ],
    )
    .await
    .unwrap();

    let run = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
    assert_eq!(run.last_persisted_seq, 2);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM transcript_events")
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    assert_eq!(rows, 1, "only the text event is stored");
    assert_eq!(
        probe.total(
            "df.agent.runtime.duration",
            &[("operation", "turn"), ("project", "DF")]
        ),
        1.0
    );
}

#[tokio::test]
async fn every_persisted_message_is_recorded_once() {
    let (env, probe) = metrics_env().await;
    let run_id = env.running_run().await;
    let (mut session, _rx) = session_for(&env, run_id).await;
    let batch = vec![
        msg(1, tool_call(wire::ToolCallStatus::InProgress)),
        msg(2, tool_call_update(wire::ToolCallStatus::Completed)),
        msg(3, turn_ended_with_usage()),
        msg(4, push_report()),
        msg(5, timing(RuntimeOperation::Push, 40)),
        msg(6, report(WireOutcome::Completed)),
    ];

    session.handle_batch(batch.clone()).await.unwrap();
    // A reconnect replays the batch.
    session.handle_batch(batch).await.unwrap();

    let project = ("project", "DF");
    let call = [
        ("kind", "execute"),
        ("origin", "harness"),
        ("status", "completed"),
        project,
    ];
    assert_eq!(probe.total("df.agent.tool_calls", &[]), 1.0);
    assert_eq!(probe.total("df.agent.tool_calls", &call), 1.0);
    assert_eq!(
        probe.total("df.agent.turns", &[("stop_reason", "end_turn"), project]),
        1.0
    );
    assert_eq!(
        probe.total("df.agent.tokens", &[("token_type", "input"), project]),
        100.0
    );
    assert_eq!(
        probe.total("df.agent.tokens", &[("token_type", "output"), project]),
        20.0
    );
    assert_eq!(
        probe.total(
            "df.agent.pushes",
            &[("target", "task_branch"), ("status", "ok"), project]
        ),
        1.0
    );
    assert_eq!(
        probe.total("df.agent.runtime.duration", &[("operation", "push")]),
        1.0
    );
    assert_eq!(
        probe.total("df.runs.ended", &[("outcome", "completed"), project]),
        1.0
    );
    assert_eq!(
        probe.total("df.run.duration", &[("outcome", "completed")]),
        1.0
    );
}

#[tokio::test]
async fn a_rolled_back_batch_records_nothing_until_it_is_persisted() {
    let (env, probe) = metrics_env().await;
    let run_id = env.running_run().await;
    let (mut session, _rx) = session_for(&env, run_id).await;
    // The last statement of every batch fails, after the messages were applied.
    sqlx::raw_sql(
        "CREATE FUNCTION refuse() RETURNS trigger LANGUAGE plpgsql AS
               $$ BEGIN RAISE EXCEPTION 'refused'; END $$;
             CREATE TRIGGER refuse BEFORE UPDATE OF last_persisted_seq ON runs
               FOR EACH ROW EXECUTE FUNCTION refuse();",
    )
    .execute(&env.ctx.pool)
    .await
    .unwrap();
    let batch = vec![msg(1, report(WireOutcome::Completed))];

    assert!(session.handle_batch(batch.clone()).await.is_err());
    assert_eq!(probe.total("df.runs.ended", &[]), 0.0);

    sqlx::raw_sql("DROP TRIGGER refuse ON runs")
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    session.handle_batch(batch).await.unwrap();
    assert_eq!(
        probe.total("df.runs.ended", &[("outcome", "completed")]),
        1.0
    );
}

#[tokio::test]
async fn streams_are_counted_and_reconnects_noticed() {
    let (env, probe) = metrics_env().await;
    let run_id = env.running_run().await;

    run_session(&env, run_id, vec![hello()]).await.unwrap();
    assert_eq!(probe.total("df.agent.reconnects", &[]), 0.0);
    run_session(&env, run_id, vec![hello()]).await.unwrap();

    assert_eq!(
        probe.total("df.agent.reconnects", &[("project", "DF")]),
        1.0
    );
    assert_eq!(probe.total("df.agent.connections", &[]), 0.0);
}
