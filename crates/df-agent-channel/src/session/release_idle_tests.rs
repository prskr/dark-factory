//! A chat run that waits for a message in its keep-alive window gives up its slot when a queued
//! chat needs it (`ReleaseIdle`) and when its chat is archived (`specs/chats/spec.md`, "Idle chat
//! runs give up their slot" and "Archived chats are read-only").

use std::time::Duration;

use df_core::TaskId;
use df_db::{publish, queue_message};

use super::tests::hello;
use super::*;
use crate::testing::Env;

const WAIT: Duration = Duration::from_secs(10);

fn text_message(text: &str) -> Vec<df_proto::transcript::v1::ContentBlock> {
    vec![df_db::text_block(text.into(), false)]
}

/// A session of the idle run, with the control messages it sent.
struct Served {
    rx: mpsc::Receiver<Result<ControlMessage, Status>>,
    to_session: mpsc::Sender<Result<AgentMessage, Status>>,
    task: tokio::task::JoinHandle<Result<(), Status>>,
}

/// Starts the session of `run_id` (keep-alive 10 minutes, so only a notification releases it) and
/// returns once it listens for notifications.
async fn serve(env: &Env, run_id: df_core::RunId) -> Served {
    let row = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
    let (to_session, inbound) = mpsc::channel(8);
    let (tx, rx) = mpsc::channel(64);
    to_session.send(Ok(hello())).await.unwrap();
    let ctx = env.ctx.clone();
    let before = ctx.notifier.subscribers();
    let (stop, stops) = watch::channel(false);
    let task = tokio::spawn(async move {
        let _stop = stop;
        run(
            ctx,
            row,
            tokio_stream::wrappers::ReceiverStream::new(inbound),
            tx,
            stops,
        )
        .await
    });
    // The session subscribes after its handshake: wait for that condition, not for a time.
    tokio::time::timeout(WAIT, async {
        while env.ctx.notifier.subscribers() <= before {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the session listens");
    Served {
        rx,
        to_session,
        task,
    }
}

impl Served {
    /// The bodies of the control messages up to and including the first one `last` accepts.
    async fn until(
        &mut self,
        last: impl Fn(&control_message::Body) -> bool,
    ) -> Vec<control_message::Body> {
        let mut seen = Vec::new();
        loop {
            let message = tokio::time::timeout(WAIT, self.rx.recv())
                .await
                .expect("a control message in time")
                .expect("the stream stays open")
                .expect("no error status");
            let body = message.body.expect("a body");
            let done = last(&body);
            seen.push(body);
            if done {
                return seen;
            }
        }
    }

    async fn finish(self) {
        drop(self.to_session);
        self.task.await.unwrap().unwrap();
    }
}

fn is_release(body: &control_message::Body) -> bool {
    matches!(body, control_message::Body::Release(_))
}

fn is_prompts(body: &control_message::Body) -> bool {
    matches!(body, control_message::Body::Prompts(_))
}

/// An idle chat: its run waits for the next message and its first message is delivered.
async fn idle_chat(env: &Env) -> (TaskId, df_core::RunId) {
    let chat = env.chat().await;
    let run_id = env.insert_run(chat.id, 1, "awaiting_input").await;
    env.bind_pod(run_id);
    sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
        .bind(chat.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE task_messages SET delivered_at = now() WHERE task_id = $1")
        .bind(chat.id.as_uuid())
        .execute(&env.ctx.pool)
        .await
        .unwrap();
    (chat.id, run_id)
}

async fn state_and_task_status(
    env: &Env,
    run_id: df_core::RunId,
    task: TaskId,
) -> (RunState, String) {
    let run = get_run(&env.ctx.pool, run_id).await.unwrap().unwrap();
    let status = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(task.as_uuid())
        .fetch_one(&env.ctx.pool)
        .await
        .unwrap();
    (run.state, status)
}

#[tokio::test]
async fn an_idle_run_asked_to_release_finalizes_and_sends_release_and_the_chat_keeps_its_status() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let (chat, run_id) = idle_chat(&env).await;
    let mut served = serve(&env, run_id).await;

    publish(&env.ctx.pool, &Notification::ReleaseIdle { run_id })
        .await
        .unwrap();
    served.until(is_release).await;

    let (state, status) = state_and_task_status(&env, run_id, chat).await;
    assert_eq!((state, status.as_str()), (RunState::Finalizing, "waiting"));
    served.finish().await;
}

#[tokio::test]
async fn a_release_request_for_another_run_leaves_the_run_alone() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let (chat, run_id) = idle_chat(&env).await;
    let mut served = serve(&env, run_id).await;

    publish(
        &env.ctx.pool,
        &Notification::ReleaseIdle {
            run_id: df_core::RunId::new(),
        },
    )
    .await
    .unwrap();
    // The prompt that follows the request shows the session has handled it.
    queue_message(
        &env.ctx.pool,
        chat,
        df_core::Principal::human(df_core::UserId::new(), "Ada"),
        text_message("Hello again"),
    )
    .await
    .unwrap();
    let seen = served.until(is_prompts).await;

    assert!(!seen.iter().any(is_release), "{seen:?}");
    assert_eq!(
        state_and_task_status(&env, run_id, chat).await.0,
        RunState::AwaitingInput
    );
    served.finish().await;
}

// Scenario "Subtask ends while archived" with a run still in its keep-alive window.
#[tokio::test]
async fn archiving_an_idle_chat_releases_its_run_and_a_child_message_is_not_forwarded() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let (chat, run_id) = idle_chat(&env).await;
    let mut served = serve(&env, run_id).await;

    df_db::chats::archive_chat(&env.ctx.pool, chat, env.human())
        .await
        .unwrap();
    served.until(is_release).await;
    let (state, status) = state_and_task_status(&env, run_id, chat).await;
    assert_eq!((state, status.as_str()), (RunState::Finalizing, "waiting"));

    queue_message(
        &env.ctx.pool,
        chat,
        df_core::Principal::system("task.child_ended"),
        text_message("DF-20 completed"),
    )
    .await
    .unwrap();
    // The session is the one that would forward it: it ends with the stream, and sent nothing.
    drop(served.to_session);
    served.task.await.unwrap().unwrap();
    while let Ok(message) = served.rx.try_recv() {
        let body = message.unwrap().body.unwrap();
        assert!(!is_prompts(&body), "{body:?}");
    }
    let queued = df_db::list_undelivered_messages(&env.ctx.pool, chat)
        .await
        .unwrap();
    assert!(
        queued.iter().all(|m| m.forwarded_at.is_none()),
        "{queued:?}"
    );
}

// Scenario "Unarchive with waiting messages": the message is committed before the session applies
// `Finalize(Idle)` (the session starts after it and gets the notification published then), so
// the release must not be refused for it. The message waits and the next run delivers it.
#[tokio::test]
async fn a_message_queued_on_an_archived_chat_does_not_keep_the_idle_run_and_waits_for_unarchive() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool.clone()).await;
    let (chat, run_id) = idle_chat(&env).await;
    df_db::chats::archive_chat(&env.ctx.pool, chat, env.human())
        .await
        .unwrap();
    queue_message(
        &env.ctx.pool,
        chat,
        df_core::Principal::system("task.child_ended"),
        text_message("DF-20 completed"),
    )
    .await
    .unwrap();
    let mut served = serve(&env, run_id).await;

    publish(&env.ctx.pool, &Notification::ReleaseIdle { run_id })
        .await
        .unwrap();
    let seen = served.until(is_release).await;
    assert!(!seen.iter().any(is_prompts), "{seen:?}");

    let (state, status) = state_and_task_status(&env, run_id, chat).await;
    assert_eq!(state, RunState::Finalizing);
    // A message makes the archived chat `running` while its run is still connected.
    assert_eq!(status, "running");
    drop(served.to_session);
    served.task.await.unwrap().unwrap();
    let queued = df_db::list_undelivered_messages(&env.ctx.pool, chat)
        .await
        .unwrap();
    assert_eq!(queued.len(), 1, "{queued:?}");

    // The run ends: the chat is `queued`, and no run starts while it is archived.
    let end = RunEnd::failure(run_id, RunOutcome::EndTurn, "");
    df_orchestrator::runs::end_run(&pool, &end, &OrchestratorConfig::default())
        .await
        .unwrap();
    assert_eq!(state_and_task_status(&env, run_id, chat).await.1, "queued");
    let started = df_db::runs::start_queued_runs(&pool, 10, 100, 100)
        .await
        .unwrap();
    // (the fixture's own queued task may start)
    assert!(started.iter().all(|run| run.task_id != chat), "{started:?}");

    // Unarchiving starts a new run, which gets the message.
    df_db::chats::unarchive_chat(&pool, chat, env.human())
        .await
        .unwrap();
    let started = df_db::runs::start_queued_runs(&pool, 10, 100, 100)
        .await
        .unwrap();
    assert_eq!(
        started.iter().filter(|run| run.task_id == chat).count(),
        1,
        "{started:?}"
    );
    let prompts = pending_prompts(&pool, chat, false).await.unwrap();
    assert_eq!(prompts.len(), 1, "{prompts:?}");
}
