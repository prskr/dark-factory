//! The agent port end to end: a real tonic server with mTLS, a scripted runtime client, the
//! fakes of `df-core` and a PostgreSQL database per test.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use df_agent_channel::testing::{ClientCert, Env, POD_SAN, RuntimeClient, TestPki, TestServer};
use df_agent_channel::{ChannelConfig, ChannelContext};
use df_core::{PeerIdentity, RunId, RunOutcome, RunState};
use df_proto::agent::v1::RunOutcome as WireOutcome;
use df_proto::agent::v1::{
    AgentMessage, CredentialRequest, GitCredentialRequest, PhaseChanged, PushReport, PushStatus,
    PushTarget, RunPhase, RunReport, ToolCallRequest, agent_message, control_message,
    credential_grant, credential_request,
};
use df_proto::transcript::v1::{
    AgentMessageChunk, ContentBlock, Event, TextContent, TurnEnded, UserMessage, content_block,
    event,
};
use sqlx::PgPool;
use tonic::Code;

struct Harness {
    env: Env,
    pki: TestPki,
    client: ClientCert,
    server: TestServer,
    run_id: RunId,
}

impl Harness {
    async fn start(pool: PgPool) -> anyhow::Result<Self> {
        let env = Env::new(pool).await;
        let run_id = env.running_run().await;
        let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
        let client = pki.pod_client(POD_SAN).map_err(anyhow::Error::from_boxed)?;
        let server = TestServer::start(env.ctx.clone(), &pki).await?;
        Ok(Self {
            env,
            pki,
            client,
            server,
            run_id,
        })
    }

    async fn connect(&self) -> anyhow::Result<RuntimeClient> {
        let channel = self.server.channel(&self.pki, Some(&self.client)).await?;
        Ok(RuntimeClient::open(channel).await?)
    }

    /// Connects and completes the handshake; returns everything the server sent after Hello.
    async fn handshake(&self) -> anyhow::Result<(RuntimeClient, Vec<control_message::Body>)> {
        let mut runtime = self.connect().await?;
        let ack = runtime.hello().await?;
        let mut bodies = vec![ack.body.expect("HelloAck")];
        // HelloAck, HarnessCredentials, Assign, then one Prompt for the assignment.
        let rest = runtime
            .recv_until(|m| matches!(m.body, Some(control_message::Body::Prompt(_))))
            .await?;
        bodies.extend(rest.into_iter().filter_map(|m| m.body));
        Ok((runtime, bodies))
    }
}

fn text(t: &str) -> ContentBlock {
    ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent { text: t.into() })),
    }
}

fn agent_text(t: &str) -> agent_message::Body {
    agent_message::Body::TranscriptEvent(Event {
        schema_version: 1,
        occurred_at: None,
        turn_id: "t1".into(),
        kind: Some(event::Kind::AgentMessageChunk(AgentMessageChunk {
            message_id: "m1".into(),
            content: Some(text(t)),
        })),
    })
}

fn user_message(prompt: &df_proto::agent::v1::Prompt) -> agent_message::Body {
    agent_message::Body::TranscriptEvent(Event {
        schema_version: 1,
        occurred_at: None,
        turn_id: "t1".into(),
        kind: Some(event::Kind::UserMessage(UserMessage {
            prompt_id: prompt.prompt_id.clone(),
            author: prompt.author.clone(),
            content: prompt.content.clone(),
        })),
    })
}

fn run_report(outcome: WireOutcome) -> agent_message::Body {
    agent_message::Body::RunReport(RunReport {
        outcome: outcome as i32,
        harness_session_id: "session-1".into(),
        ..RunReport::default()
    })
}

async fn count(pool: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

/// Waits until `check` holds; the database is the observable of every asynchronous effect.
async fn eventually<F, Fut>(mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..200 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("condition not reached in time");
}

// --- 1. Agent port and identity -------------------------------------------------------------

#[tokio::test]
async fn client_with_a_certificate_of_the_instance_ca_completes_the_handshake() -> anyhow::Result<()>
{
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool).await?;
    let mut runtime = h.connect().await?;
    let ack = runtime.hello().await?;
    assert!(
        matches!(ack.body, Some(control_message::Body::HelloAck(a)) if a.run_id == h.run_id.to_string())
    );
    Ok(())
}

#[tokio::test]
async fn tls_handshake_fails_without_a_client_certificate() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool).await?;
    // TLS 1.3 reports a rejected client certificate on first use of the connection.
    let outcome = match h.server.channel(&h.pki, None).await {
        Err(_) => Err(()),
        Ok(channel) => RuntimeClient::open(channel)
            .await
            .map(|_| ())
            .map_err(|_| ()),
    };
    assert!(outcome.is_err());
    Ok(())
}

#[tokio::test]
async fn tls_handshake_fails_with_a_certificate_from_another_ca() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool).await?;
    let rogue = TestPki::rogue_client(POD_SAN).map_err(anyhow::Error::from_boxed)?;
    let outcome = match h.server.channel(&h.pki, Some(&rogue)).await {
        Err(_) => Err(()),
        Ok(channel) => RuntimeClient::open(channel)
            .await
            .map(|_| ())
            .map_err(|_| ()),
    };
    assert!(outcome.is_err());
    Ok(())
}

#[tokio::test]
async fn message_over_16_mib_fails_with_resource_exhausted() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool).await?;
    let (mut runtime, _) = h.handshake().await?;
    let huge = "x".repeat(20 * 1024 * 1024);
    runtime.send(agent_text(&huge)).await;
    let status = runtime.recv().await.expect_err("stream must fail");
    assert_eq!(status.code(), Code::ResourceExhausted);
    Ok(())
}

#[tokio::test]
async fn valid_certificate_from_another_ip_is_unauthenticated() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let env = Env::new(pool).await;
    let run_id = env.insert_run(env.task.id, 1, "connecting").await;
    // The provider only knows the pod at another address: a stolen key used elsewhere.
    env.provider.bind_identity(
        PeerIdentity {
            uri_san: POD_SAN.into(),
            peer_ip: IpAddr::V4(Ipv4Addr::new(10, 9, 9, 9)),
        },
        run_id,
    );
    let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
    let client = pki.pod_client(POD_SAN).map_err(anyhow::Error::from_boxed)?;
    let server = TestServer::start(env.ctx.clone(), &pki).await?;
    let channel = server.channel(&pki, Some(&client)).await?;
    let err = RuntimeClient::open(channel)
        .await
        .err()
        .expect("must be rejected");
    assert_eq!(err.code(), Code::Unauthenticated);
    Ok(())
}

#[tokio::test]
async fn pod_of_a_finished_run_is_unauthenticated() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool.clone()).await?;
    sqlx::query("UPDATE runs SET state = 'succeeded' WHERE id = $1")
        .bind(h.run_id.as_uuid())
        .execute(&pool)
        .await?;
    let err = h.connect().await.err().expect("must be rejected");
    assert_eq!(
        err.downcast_ref::<tonic::Status>().map(tonic::Status::code),
        Some(Code::Unauthenticated)
    );
    Ok(())
}

// --- 2. Protocol session --------------------------------------------------------------------

#[tokio::test]
async fn first_connect_receives_ack_credentials_assign_and_the_assignment_prompt()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool).await?;
    let (_runtime, bodies) = h.handshake().await?;

    let [
        control_message::Body::HelloAck(ack),
        control_message::Body::HarnessCredentials(creds),
        control_message::Body::Assign(assign),
        control_message::Body::Prompt(prompt),
    ] = bodies.as_slice()
    else {
        anyhow::bail!("unexpected handshake: {bodies:?}");
    };
    assert_eq!(
        (ack.run_id.as_str(), ack.last_persisted_seq),
        (h.run_id.to_string().as_str(), 0)
    );
    assert_eq!(creds.env["CLAUDE_CODE_OAUTH_TOKEN"], "claude-secret-value");
    assert_eq!(
        assign.workspace.as_ref().expect("workspace").task_branch,
        "factory/task/DF-1"
    );
    assert_eq!(assign.tools.len(), 5);
    let Some(content_block::Kind::Text(t)) = &prompt.content[0].kind else {
        anyhow::bail!("no text")
    };
    assert!(t.text.starts_with("# Fix login"));
    Ok(())
}

#[tokio::test]
async fn a_first_message_other_than_hello_is_a_failed_precondition() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool).await?;
    let mut runtime = h.connect().await?;
    runtime.send(agent_text("too early")).await;
    assert_eq!(
        runtime.recv().await.expect_err("closed").code(),
        Code::FailedPrecondition
    );
    Ok(())
}

#[tokio::test]
async fn replayed_messages_are_dropped_and_the_ack_covers_the_batch() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool.clone()).await?;
    let (mut first, _) = h.handshake().await?;
    for i in 1..=7 {
        first.send(agent_text(&format!("m{i}"))).await;
    }
    first.recv_until(|m| m.ack == 7).await?;
    drop(first);

    let mut second = h.connect().await?;
    let ack = second.hello().await?;
    assert!(
        matches!(ack.body, Some(control_message::Body::HelloAck(a)) if a.last_persisted_seq == 7)
    );
    second
        .recv_until(|m| matches!(m.body, Some(control_message::Body::Prompt(_))))
        .await?;
    // The runtime replays from its outbox: 5-9 while 5-7 are already stored.
    for seq in 5..=9 {
        second
            .send_raw(AgentMessage {
                seq,
                body: Some(agent_text(&format!("m{seq}"))),
            })
            .await;
    }
    second.recv_until(|m| m.ack == 9).await?;
    assert_eq!(
        count(&pool, "SELECT count(*) FROM transcript_events").await,
        9
    );
    Ok(())
}

#[tokio::test]
async fn a_sequence_gap_closes_the_stream_with_data_loss() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool.clone()).await?;
    let (mut runtime, _) = h.handshake().await?;
    for i in 1..=10 {
        runtime.send(agent_text(&format!("m{i}"))).await;
    }
    runtime.recv_until(|m| m.ack == 10).await?;
    runtime
        .send_raw(AgentMessage {
            seq: 12,
            body: Some(agent_text("skipped 11")),
        })
        .await;
    let status = runtime.recv().await.expect_err("closed");
    assert_eq!(status.code(), Code::DataLoss);
    assert_eq!(
        count(&pool, "SELECT count(*) FROM transcript_events").await,
        10
    );
    Ok(())
}

#[tokio::test]
async fn reconnect_to_another_replica_resends_assign_and_unconfirmed_prompts() -> anyhow::Result<()>
{
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool).await?;
    let (first, bodies_a) = h.handshake().await?;
    drop(first);

    let replica_b = TestServer::start(h.env.other_replica().await, &h.pki).await?;
    let channel = replica_b.channel(&h.pki, Some(&h.client)).await?;
    let mut runtime = RuntimeClient::open(channel).await?;
    runtime.hello().await?;
    let rest = runtime
        .recv_until(|m| matches!(m.body, Some(control_message::Body::Prompt(_))))
        .await?;
    let bodies_b: Vec<_> = rest.into_iter().filter_map(|m| m.body).collect();

    // Same Assign and same prompt as replica A sent.
    assert_eq!(bodies_a[2], bodies_b[1]);
    assert_eq!(bodies_a[3], bodies_b[2]);
    Ok(())
}

#[tokio::test]
async fn prompt_queued_during_a_run_is_delivered_and_not_resent_once_confirmed()
-> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool.clone()).await?;
    let (mut runtime, bodies) = h.handshake().await?;
    let control_message::Body::Prompt(assignment) = &bodies[3] else {
        anyhow::bail!("no prompt")
    };

    // The assignment prompt is confirmed by the transcript and its turn ends, then a follow-up
    // is queued.
    runtime
        .send(agent_message::Body::TranscriptEvent(Event {
            schema_version: 1,
            occurred_at: None,
            turn_id: "t1".into(),
            kind: Some(event::Kind::UserMessage(UserMessage {
                prompt_id: assignment.prompt_id.clone(),
                author: assignment.author.clone(),
                content: assignment.content.clone(),
            })),
        }))
        .await;
    runtime
        .send(agent_message::Body::TranscriptEvent(Event {
            schema_version: 1,
            occurred_at: None,
            turn_id: "t1".into(),
            kind: Some(event::Kind::TurnEnded(TurnEnded::default())),
        }))
        .await;
    runtime.recv_until(|m| m.ack == 2).await?;
    df_db::queue_message(
        &pool,
        h.env.task.id,
        df_core::Principal::system("test"),
        vec![text("also add a test")],
    )
    .await?;
    let msgs = runtime
        .recv_until(|m| matches!(m.body, Some(control_message::Body::Prompt(_))))
        .await?;
    let Some(control_message::Body::Prompt(follow_up)) = msgs.last().and_then(|m| m.body.clone())
    else {
        anyhow::bail!("no follow-up prompt");
    };
    assert_ne!(follow_up.prompt_id, assignment.prompt_id);

    // After a reconnect only the unconfirmed follow-up is sent again.
    drop(runtime);
    let (_again, bodies) = h.handshake().await?;
    let prompts: Vec<_> = bodies
        .iter()
        .filter_map(|b| match b {
            control_message::Body::Prompt(p) => Some(p.prompt_id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(prompts, [follow_up.prompt_id]);
    Ok(())
}

#[tokio::test]
async fn cancel_is_sent_when_the_wall_clock_budget_and_grace_are_over() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut env = Env::new(pool).await;
    let mut config = ChannelConfig::default();
    config.runs.max_wall_clock_seconds = 0;
    config.runs.cancel_grace_seconds = 0;
    env.ctx.config = std::sync::Arc::new(config);
    let run_id = env.running_run().await;
    let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
    let client = pki.pod_client(POD_SAN).map_err(anyhow::Error::from_boxed)?;
    let server = TestServer::start(env.ctx.clone(), &pki).await?;
    let mut runtime = RuntimeClient::open(server.channel(&pki, Some(&client)).await?).await?;
    runtime.hello().await?;
    runtime
        .recv_until(|m| matches!(m.body, Some(control_message::Body::Cancel(_))))
        .await?;
    let _ = run_id;
    Ok(())
}

#[tokio::test]
async fn a_message_with_an_unknown_body_keeps_its_place_in_the_sequence() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool.clone()).await?;
    let (mut runtime, _) = h.handshake().await?;
    // seq 1 carries a body variant this version does not know (decoded as no body).
    runtime.send_raw(AgentMessage { seq: 1, body: None }).await;
    runtime
        .send_raw(AgentMessage {
            seq: 2,
            body: Some(agent_text("after")),
        })
        .await;
    runtime.recv_until(|m| m.ack == 2).await?;
    assert_eq!(
        count(&pool, "SELECT count(*) FROM transcript_events").await,
        1
    );
    Ok(())
}

#[tokio::test]
async fn replayed_events_do_not_move_their_blobs_again() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    use df_proto::transcript::v1::{BlobData, ImageContent, blob_data};
    let h = Harness::start(pool.clone()).await?;
    let image = agent_message::Body::TranscriptEvent(Event {
        schema_version: 1,
        occurred_at: None,
        turn_id: "t".into(),
        kind: Some(event::Kind::AgentMessageChunk(AgentMessageChunk {
            message_id: "m".into(),
            content: Some(ContentBlock {
                kind: Some(content_block::Kind::Image(ImageContent {
                    mime_type: "image/png".into(),
                    data: Some(BlobData {
                        source: Some(blob_data::Source::Inline(bytes::Bytes::from_static(b"png"))),
                    }),
                })),
            }),
        })),
    });
    let (mut first, _) = h.handshake().await?;
    first.send(image.clone()).await;
    first.recv_until(|m| m.ack == 1).await?;
    assert_eq!(count(&pool, "SELECT count(*) FROM blobs").await, 1);
    drop(first);

    // After a reconnect the runtime replays seq 1; its blob must not be written again.
    sqlx::query("DELETE FROM blobs").execute(&pool).await?;
    let mut second = h.connect().await?;
    second.hello().await?;
    second
        .send_raw(AgentMessage {
            seq: 1,
            body: Some(image),
        })
        .await;
    second
        .send_raw(AgentMessage {
            seq: 2,
            body: Some(agent_text("next")),
        })
        .await;
    second.recv_until(|m| m.ack == 2).await?;
    assert_eq!(count(&pool, "SELECT count(*) FROM blobs").await, 0);
    Ok(())
}

#[tokio::test]
async fn rotated_tls_material_is_served_without_a_restart() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool).await?;
    let mut before = h.connect().await?;
    before.hello().await?;

    // cert-manager renews: new CA, new server certificate.
    let renewed = TestPki::new().map_err(anyhow::Error::from_boxed)?;
    h.server.rotate(&renewed)?;

    let new_client = renewed
        .pod_client(POD_SAN)
        .map_err(anyhow::Error::from_boxed)?;
    let channel = h.server.channel(&renewed, Some(&new_client)).await?;
    let mut after = RuntimeClient::open(channel).await?;
    after.hello().await?;

    // The old CA is no longer accepted, and the stream opened earlier is unaffected.
    let outcome = match h.server.channel(&renewed, Some(&h.client)).await {
        Err(_) => Err(()),
        Ok(channel) => RuntimeClient::open(channel)
            .await
            .map(|_| ())
            .map_err(|_| ()),
    };
    assert!(outcome.is_err());
    before.send(agent_text("still connected")).await;
    before.recv_until(|m| m.ack == 1).await?;
    Ok(())
}

#[tokio::test]
async fn shutdown_does_not_wait_for_open_streams() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let mut h = Harness::start(pool).await?;
    let mut runtime = h.connect().await?;
    runtime.hello().await?;

    assert!(h.server.stop(Duration::from_secs(10)).await);
    Ok(())
}

// --- 3. Phases and run end ------------------------------------------------------------------

async fn run_state(pool: &PgPool, run_id: RunId) -> RunState {
    let state: String = sqlx::query_scalar("SELECT state FROM runs WHERE id = $1")
        .bind(run_id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap();
    state.parse().unwrap()
}

async fn task_status(pool: &PgPool, h: &Harness) -> String {
    sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(h.env.task.id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn reported_phases_drive_the_run_state() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool.clone()).await?;
    let (mut runtime, _) = h.handshake().await?;
    // Hello moved the run out of `connecting`.
    assert_eq!(run_state(&pool, h.run_id).await, RunState::Preparing);

    let seq = runtime
        .send(agent_message::Body::PhaseChanged(PhaseChanged {
            phase: RunPhase::Running as i32,
            detail: String::new(),
        }))
        .await;
    runtime.recv_until(|m| m.ack == seq).await?;
    assert_eq!(run_state(&pool, h.run_id).await, RunState::Running);
    Ok(())
}

#[tokio::test]
async fn run_report_maps_every_outcome_to_run_state_and_task_status() -> anyhow::Result<()> {
    let table = [
        (
            WireOutcome::Completed,
            RunState::Succeeded,
            RunOutcome::Completed,
            "completed",
        ),
        (
            WireOutcome::EndTurn,
            RunState::Succeeded,
            RunOutcome::EndTurn,
            "waiting",
        ),
        (
            WireOutcome::LimitReached,
            RunState::Succeeded,
            RunOutcome::LimitReached,
            "limit_reached",
        ),
        (
            WireOutcome::RateLimited,
            RunState::Succeeded,
            RunOutcome::RateLimited,
            "rate_limited",
        ),
        (
            WireOutcome::Cancelled,
            RunState::Cancelled,
            RunOutcome::Cancelled,
            "waiting",
        ),
        (
            WireOutcome::Failed,
            RunState::Failed,
            RunOutcome::Failed,
            "failed",
        ),
    ];
    for (wire, state, outcome, status) in table {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let h = Harness::start(pool.clone()).await?;
        let (mut runtime, _) = h.handshake().await?;
        // The assignment prompt is confirmed, so nothing is pending at run end.
        sqlx::query("UPDATE task_messages SET delivered_at = now()")
            .execute(&pool)
            .await?;
        let seq = runtime.send(run_report(wire)).await;
        runtime.recv_until(|m| m.ack == seq).await?;

        let (db_state, db_outcome, session): (String, String, String) =
            sqlx::query_as("SELECT state, outcome, harness_session_id FROM runs WHERE id = $1")
                .bind(h.run_id.as_uuid())
                .fetch_one(&pool)
                .await?;
        assert_eq!(
            (db_state.as_str(), db_outcome.as_str()),
            (state.as_str(), outcome.as_str()),
            "{wire:?}"
        );
        assert_eq!(session, "session-1");
        assert_eq!(task_status(&pool, &h).await, status, "{wire:?}");
        let task_session: String =
            sqlx::query_scalar("SELECT harness_session_id FROM tasks WHERE id = $1")
                .bind(h.env.task.id.as_uuid())
                .fetch_one(&pool)
                .await?;
        assert_eq!(task_session, "session-1");
        eventually(|| async {
            h.env
                .provider
                .calls()
                .contains(&df_core::testing::SandboxCall::Teardown(h.run_id))
        })
        .await;
    }
    Ok(())
}

#[tokio::test]
async fn message_queued_while_finalizing_requeues_the_task() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool.clone()).await?;
    let (mut runtime, bodies) = h.handshake().await?;
    let control_message::Body::Prompt(assignment) = &bodies[3] else {
        anyhow::bail!("no prompt")
    };
    sqlx::query("UPDATE task_messages SET delivered_at = now() WHERE id = $1")
        .bind(assignment.prompt_id.parse::<uuid::Uuid>()?)
        .execute(&pool)
        .await?;
    df_db::queue_message(
        &pool,
        h.env.task.id,
        df_core::Principal::system("test"),
        vec![text("one more thing")],
    )
    .await?;

    let seq = runtime.send(run_report(WireOutcome::EndTurn)).await;
    runtime.recv_until(|m| m.ack == seq).await?;
    assert_eq!(task_status(&pool, &h).await, "queued");
    Ok(())
}

// --- 5. Tools and credentials over the stream -----------------------------------------------

#[tokio::test]
async fn tool_call_and_credential_requests_are_answered_on_the_stream() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool.clone()).await?;
    let (mut runtime, _) = h.handshake().await?;
    let call_id = uuid::Uuid::now_v7().to_string();
    runtime
        .send(agent_message::Body::ToolCallRequest(ToolCallRequest {
            call_id: call_id.clone(),
            tool_name: "report_progress".into(),
            arguments_json: r#"{"summary":"tests pass"}"#.into(),
        }))
        .await;
    let msgs = runtime
        .recv_until(|m| matches!(m.body, Some(control_message::Body::ToolCallResult(_))))
        .await?;
    let Some(control_message::Body::ToolCallResult(result)) =
        msgs.last().and_then(|m| m.body.clone())
    else {
        anyhow::bail!("no result");
    };
    assert_eq!(
        (result.call_id.as_str(), result.is_error),
        (call_id.as_str(), false)
    );

    for (path, expect_grant) in [("peter/dark-factory.git", true), ("peter/other.git", false)] {
        runtime
            .send(agent_message::Body::CredentialRequest(CredentialRequest {
                request_id: path.into(),
                kind: Some(credential_request::Kind::Git(GitCredentialRequest {
                    protocol: "https".into(),
                    host: "git.example.org".into(),
                    path: path.into(),
                })),
            }))
            .await;
        let msgs = runtime
            .recv_until(|m| matches!(m.body, Some(control_message::Body::CredentialGrant(_))))
            .await?;
        let Some(control_message::Body::CredentialGrant(grant)) =
            msgs.last().and_then(|m| m.body.clone())
        else {
            anyhow::bail!("no grant");
        };
        assert_eq!(
            matches!(grant.result, Some(credential_grant::Result::Git(_))),
            expect_grant,
            "{path}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn missing_harness_secret_fails_the_run_naming_the_reference() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let h = Harness::start(pool.clone()).await?;
    h.env.secrets.insert_error(
        "claude",
        "token",
        df_core::SecretError::NotFound {
            secret_name: "claude".into(),
            secret_key: "token".into(),
        },
    );
    let mut runtime = h.connect().await?;
    let err = runtime.hello().await.expect_err("run must fail");
    assert_eq!(err.code(), Code::FailedPrecondition);

    let (state, detail): (String, String) =
        sqlx::query_as("SELECT state, detail FROM runs WHERE id = $1")
            .bind(h.run_id.as_uuid())
            .fetch_one(&pool)
            .await?;
    assert_eq!(state, "failed");
    assert!(detail.contains("`claude`"), "{detail}");
    assert!(!detail.contains("claude-secret-value"));
    assert_eq!(task_status(&pool, &h).await, "failed");
    Ok(())
}

/// Collects everything the `tracing` macros emit while it is the default subscriber.
#[derive(Clone, Default)]
struct LogCapture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn secret_values_never_appear_in_logs() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let logs = LogCapture::default();
    let writer = logs.clone();
    let _guard = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish(),
    );

    let h = Harness::start(pool.clone()).await?;
    let (mut runtime, _) = h.handshake().await?;
    for (id, path) in [
        ("ok", "peter/dark-factory.git"),
        ("denied", "peter/other.git"),
    ] {
        runtime
            .send(agent_message::Body::CredentialRequest(CredentialRequest {
                request_id: id.into(),
                kind: Some(credential_request::Kind::Git(GitCredentialRequest {
                    protocol: "https".into(),
                    host: "git.example.org".into(),
                    path: path.into(),
                })),
            }))
            .await;
        runtime
            .recv_until(|m| matches!(m.body, Some(control_message::Body::CredentialGrant(_))))
            .await?;
    }
    // Failing secret reads log too.
    h.env.secrets.insert_error(
        "claude",
        "token",
        df_core::SecretError::Backend("secret store unreachable".into()),
    );
    let mut second = h.connect().await?;
    second
        .hello()
        .await
        .expect_err("run must fail without the secret");

    let captured = String::from_utf8(logs.0.lock().expect("log buffer").clone())?;
    assert!(!captured.is_empty(), "the test must capture something");
    for secret in ["claude-secret-value", "forge-secret-value"] {
        assert!(!captured.contains(secret), "log contains `{secret}`");
    }
    Ok(())
}

// --- 6. End to end --------------------------------------------------------------------------

#[tokio::test]
async fn task_to_follow_up_run_end_to_end() -> anyhow::Result<()> {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    use df_agent_channel::Scheduler;
    use df_core::SandboxEvent;
    use df_core::testing::ForgeCall;

    let env = Env::new(pool.clone()).await;
    let pki = TestPki::new().map_err(anyhow::Error::from_boxed)?;
    let client = pki.pod_client(POD_SAN).map_err(anyhow::Error::from_boxed)?;
    let server = TestServer::start(env.ctx.clone(), &pki).await?;
    let scheduler = Scheduler::new(env.ctx.clone());
    // The production loop starts and provisions the runs.
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let looping = scheduler.clone();
    let scheduler_task = tokio::spawn(async move {
        looping
            .run(async {
                let _ = stopped.await;
            })
            .await;
    });
    let provisions = |env: &Env| {
        env.provider
            .calls()
            .iter()
            .filter(|c| matches!(c, df_core::testing::SandboxCall::Provision(_)))
            .count()
    };

    // Run 1: scheduler -> provider Started -> runtime connects.
    eventually(|| async { provisions(&env) == 1 }).await;
    let run1 = provisioned_run(&env);
    env.bind_pod(run1);
    scheduler
        .handle_event(SandboxEvent::Started {
            run_id: run1,
            pod_ip: "127.0.0.1".parse()?,
        })
        .await?;

    let mut runtime = RuntimeClient::open(server.channel(&pki, Some(&client)).await?).await?;
    runtime.hello().await?;
    let msgs = runtime
        .recv_until(|m| matches!(m.body, Some(control_message::Body::Prompt(_))))
        .await?;
    let Some(control_message::Body::Prompt(assignment)) = msgs.last().and_then(|m| m.body.clone())
    else {
        anyhow::bail!("no assignment prompt");
    };
    runtime.send(user_message(&assignment)).await;
    runtime.send(agent_text("on it")).await;
    runtime
        .send(agent_message::Body::ToolCallRequest(ToolCallRequest {
            call_id: uuid::Uuid::now_v7().to_string(),
            tool_name: "comment_on_issue".into(),
            arguments_json: r#"{"body":"working on it"}"#.into(),
        }))
        .await;
    runtime
        .recv_until(|m| matches!(m.body, Some(control_message::Body::ToolCallResult(_))))
        .await?;
    let push = runtime
        .send(agent_message::Body::PushReport(PushReport {
            target: PushTarget::TaskBranch as i32,
            status: PushStatus::Ok as i32,
            commits_ahead_of_base: 2,
            ..PushReport::default()
        }))
        .await;
    runtime.recv_until(|m| m.ack == push).await?;
    let end = runtime.send(run_report(WireOutcome::EndTurn)).await;
    runtime.recv_until(|m| m.ack == end).await?;

    eventually(|| async { env_task_status(&pool, &env).await == "waiting" }).await;
    // The pull request is opened beside the stream, not before the run ends.
    eventually(|| async {
        count(
            &pool,
            "SELECT count(*) FROM tasks WHERE pr_number IS NOT NULL",
        )
        .await
            == 1
    })
    .await;

    // Follow-up message: the scheduler starts run 2 with a continuation.
    df_db::queue_message(
        &pool,
        env.task.id,
        df_core::Principal::system("test"),
        vec![text("next step")],
    )
    .await?;
    eventually(|| async { provisions(&env) == 2 }).await;
    assert_eq!(env_task_status(&pool, &env).await, "running");
    let run2: uuid::Uuid = sqlx::query_scalar("SELECT id FROM runs WHERE number = 2")
        .fetch_one(&pool)
        .await?;
    let run2 = RunId::from_uuid(run2);
    env.bind_pod(run2);
    scheduler
        .handle_event(SandboxEvent::Started {
            run_id: run2,
            pod_ip: "127.0.0.1".parse()?,
        })
        .await?;

    let mut runtime = RuntimeClient::open(server.channel(&pki, Some(&client)).await?).await?;
    runtime.hello().await?;
    let msgs = runtime
        .recv_until(|m| matches!(m.body, Some(control_message::Body::Prompt(_))))
        .await?;
    let assign = msgs
        .iter()
        .find_map(|m| match &m.body {
            Some(control_message::Body::Assign(a)) => Some(a.clone()),
            _ => None,
        })
        .expect("Assign");
    let continuation = assign.continuation.expect("continuation");
    assert_eq!(continuation.harness_session_id, "session-1");
    assert!(!continuation.summary.is_empty());

    // Only one pull request was ever created, and the provider saw two sandboxes.
    let creates = env
        .forge
        .calls()
        .iter()
        .filter(|c| matches!(c, ForgeCall::CreatePullRequest { .. }))
        .count();
    assert_eq!(creates, 1);
    assert_eq!(provisions(&env), 2);
    let _ = stop.send(());
    scheduler_task.await?;
    Ok(())
}

fn provisioned_run(env: &Env) -> RunId {
    env.provider
        .calls()
        .into_iter()
        .find_map(|c| match c {
            df_core::testing::SandboxCall::Provision(spec) => Some(spec.run_id),
            _ => None,
        })
        .expect("a sandbox was provisioned")
}

async fn env_task_status(pool: &PgPool, env: &Env) -> String {
    sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(env.task.id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[allow(dead_code)]
fn _context_type_is_public(_: &ChannelContext) {}
