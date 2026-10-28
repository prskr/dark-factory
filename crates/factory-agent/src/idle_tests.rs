//! Runner tests for the idle wait after a normal turn end (`specs/acp-harness/spec.md`,
//! "Idle wait after a normal turn end"), driven by the fake ACP agent.

use std::path::PathBuf;
use std::time::Duration;

use df_proto::agent::v1::{
    AgentMessage, Assign, Budgets, ControlMessage, Harness, Prompt, Release, RunOutcome, RunPhase,
    ToolCallResult, ToolDescriptor, Workspace, agent_message, control_message,
};
use df_proto::transcript::v1::event::Kind as EventKind;
use tokio::sync::mpsc;

use crate::git::Git;
use crate::outbox::Outbox;
use crate::runner::{RunDriverParams, RunExecutionResult, run_session};
use crate::tests::setup_test_remote_repo;

const TWO_TURNS: &str = r#"
sessions:
  new:
    - - say: "first answer"
      - end: end_turn
    - - say: "second answer"
      - end: end_turn
"#;

/// A run in progress with the control plane's side of its channels.
struct IdleRun {
    control: mpsc::Sender<ControlMessage>,
    outbound: mpsc::Receiver<AgentMessage>,
    run: tokio::task::JoinHandle<RunExecutionResult>,
    /// Everything the runtime sent so far.
    seen: Vec<AgentMessage>,
    _temp: tempfile::TempDir,
}

fn workspace_bin(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug")
        .join(name)
}

/// The text of an agent message chunk, if the message is one.
fn chunk_text(msg: &AgentMessage) -> Option<&str> {
    match &msg.body {
        Some(agent_message::Body::TranscriptEvent(e)) => match &e.kind {
            Some(EventKind::AgentMessageChunk(c)) => match &c.content {
                Some(df_proto::transcript::v1::ContentBlock {
                    kind: Some(df_proto::transcript::v1::content_block::Kind::Text(t)),
                    ..
                }) => Some(t.text.as_str()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

fn text_prompt(id: &str) -> Prompt {
    Prompt {
        prompt_id: id.to_string(),
        author: None,
        content: vec![df_proto::transcript::v1::ContentBlock {
            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                df_proto::transcript::v1::TextContent {
                    text: "go".to_string(),
                },
            )),
            hidden: false,
        }],
        queued_at: None,
    }
}

/// Params of a run with `script` and one initial prompt, wired to the given channels. `None`
/// when the helper binaries are not built (same convention as the other runner tests).
fn params(
    temp: &tempfile::TempDir,
    script: &str,
    max_wall_clock_secs: Option<i64>,
    inbound_rx: mpsc::Receiver<ControlMessage>,
    outbound_tx: mpsc::Sender<AgentMessage>,
) -> Option<RunDriverParams> {
    let fake_bin = workspace_bin("fake-acp-agent");
    let mcp_bin = workspace_bin("factory-agent");
    if !fake_bin.exists() || !mcp_bin.exists() {
        return None;
    }
    let upstream = setup_test_remote_repo(temp.path());
    let script_file = temp.path().join("script.yaml");
    std::fs::write(&script_file, script).unwrap();
    let repo_dir = temp.path().join("repo");
    let runtime_dir = temp.path().join("runtime");
    let hooks_path = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&hooks_path).unwrap();
    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&runtime_dir, "run-idle", 10 * 1024 * 1024, 64 * 1024).unwrap(),
    ));
    let assign = Assign {
        run_id: "run-idle".to_string(),
        workspace: Some(Workspace {
            repo_url: upstream.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            ..Default::default()
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        script_file.to_string_lossy().to_string(),
                    ],
                    ..Default::default()
                },
            )),
            system_prompt: String::new(),
        }),
        tools: ["ask_question", "complete_task"]
            .map(|name| ToolDescriptor {
                name: name.to_string(),
                title: name.to_string(),
                description: name.to_string(),
                input_schema_json: "{}".to_string(),
                ..Default::default()
            })
            .to_vec(),
        budgets: max_wall_clock_secs.map(|s| Budgets {
            max_wall_clock: Some(df_proto::pbjson_types::Duration {
                seconds: s,
                nanos: 0,
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    Some(RunDriverParams {
        assign,
        credentials: None,
        initial_prompts: vec![text_prompt("prompt-1")],
        cancel_requested: false,
        home_dir: temp.path().join("home"),
        runtime_dir,
        hooks_path,
        mcp_command: mcp_bin,
        runtime_sock_path: temp.path().join("runtime.sock"),
        git: Git::new(&repo_dir).allow_protocols(["file", "https"]),
        repo_dir,
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
    })
}

/// Starts a run with `script` and the control plane's side of its channels.
fn start(script: &str, max_wall_clock_secs: Option<i64>) -> Option<IdleRun> {
    let temp = tempfile::tempdir().unwrap();
    let (control, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, outbound) = mpsc::channel(1000);
    let params = params(&temp, script, max_wall_clock_secs, inbound_rx, outbound_tx)?;
    let run = tokio::spawn(async move { run_session(params).await.unwrap() });
    Some(IdleRun {
        control,
        outbound,
        run,
        seen: Vec::new(),
        _temp: temp,
    })
}

impl IdleRun {
    async fn send(&self, body: control_message::Body) {
        self.control
            .send(ControlMessage {
                ack: 0,
                body: Some(body),
            })
            .await
            .unwrap();
    }

    /// Reads runtime messages until `pick` matches one, returning it.
    async fn next<T>(&mut self, pick: impl Fn(&agent_message::Body) -> Option<T>) -> T {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(60), self.outbound.recv())
                .await
                .expect("runtime message in time")
                .expect("runtime still sends");
            let found = msg.body.as_ref().and_then(&pick);
            self.seen.push(msg);
            if let Some(found) = found {
                return found;
            }
        }
    }

    async fn phase(&mut self, want: RunPhase) {
        self.next(|b| match b {
            agent_message::Body::PhaseChanged(p) if p.phase() == want => Some(()),
            _ => None,
        })
        .await;
    }

    async fn turn_ended(&mut self) {
        self.next(|b| match b {
            agent_message::Body::TranscriptEvent(e)
                if matches!(e.kind, Some(EventKind::TurnEnded(_))) =>
            {
                Some(())
            }
            _ => None,
        })
        .await;
    }

    /// Answers the next call of `tool` like the control plane does for `ask_question`
    /// (`await_input` for a blocking question) and `complete_task` (`end_run`).
    async fn answer_call(&mut self, tool: &str, await_input: bool, end_run: bool) {
        let call_id = self
            .next(|b| match b {
                agent_message::Body::ToolCallRequest(r) if r.tool_name == tool => {
                    Some(r.call_id.clone())
                }
                _ => None,
            })
            .await;
        self.send(control_message::Body::ToolCallResult(ToolCallResult {
            call_id,
            await_input_after_turn: await_input,
            end_run_after_turn: end_run,
            ..Default::default()
        }))
        .await;
    }

    async fn finish(self) -> RunExecutionResult {
        tokio::time::timeout(Duration::from_secs(60), self.run)
            .await
            .expect("run ends in time")
            .unwrap()
    }
}

#[tokio::test]
async fn follow_up_while_idle_starts_the_next_turn_in_the_same_session() {
    let Some(mut run) = start(TWO_TURNS, None) else {
        return;
    };
    run.phase(RunPhase::AwaitingInput).await;
    run.send(control_message::Body::Prompt(text_prompt("prompt-2")))
        .await;
    run.phase(RunPhase::Running).await;
    run.turn_ended().await;
    run.phase(RunPhase::AwaitingInput).await;
    run.send(control_message::Body::Release(Release {})).await;
    let seen = std::mem::take(&mut run.seen);
    let result = run.finish().await;
    assert_eq!(result.outcome, RunOutcome::EndTurn);
    // The scripted second turn ran, in the one session that was created.
    let said: Vec<&str> = seen.iter().filter_map(chunk_text).collect();
    // The fake agent plays its scripted turns in order within one session; a second session
    // would start over with "first answer".
    assert_eq!(said, ["first answer", "second answer"]);
}

const BLOCKING_QUESTION: &str = r#"
sessions:
  new:
    - - mcp:
          server: factory
          tool: ask_question
          args: { text: "Which one?", blocking: true }
      - end: end_turn
    - - say: "thanks"
      - end: end_turn
"#;

#[tokio::test]
async fn release_after_a_blocking_question_ends_the_run_blocked() {
    let Some(mut run) = start(BLOCKING_QUESTION, None) else {
        return;
    };
    run.answer_call("ask_question", true, false).await;
    run.phase(RunPhase::AwaitingInput).await;
    run.send(control_message::Body::Release(Release {})).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::Blocked);
}

#[tokio::test]
async fn an_answer_within_the_window_starts_the_next_turn_and_the_flag_is_forgotten() {
    let Some(mut run) = start(BLOCKING_QUESTION, None) else {
        return;
    };
    run.answer_call("ask_question", true, false).await;
    run.phase(RunPhase::AwaitingInput).await;
    run.send(control_message::Body::Prompt(text_prompt("prompt-2")))
        .await;
    run.phase(RunPhase::Running).await;
    run.turn_ended().await;
    run.phase(RunPhase::AwaitingInput).await;
    run.send(control_message::Body::Release(Release {})).await;
    // The second turn asked nothing blocking.
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn cancel_while_awaiting_an_answer_ends_the_run_cancelled() {
    let Some(mut run) = start(BLOCKING_QUESTION, None) else {
        return;
    };
    run.answer_call("ask_question", true, false).await;
    run.phase(RunPhase::AwaitingInput).await;
    run.send(control_message::Body::Cancel(
        df_proto::agent::v1::Cancel::default(),
    ))
    .await;
    assert_eq!(run.finish().await.outcome, RunOutcome::Cancelled);
}

#[tokio::test]
async fn a_blocking_question_and_completion_in_one_turn_complete_the_run() {
    let script = r#"
sessions:
  new:
    - - mcp: { server: factory, tool: ask_question, args: { text: "Which?", blocking: true } }
      - mcp: { server: factory, tool: complete_task, args: { summary: "done" } }
      - end: end_turn
"#;
    let Some(mut run) = start(script, None) else {
        return;
    };
    run.answer_call("ask_question", true, false).await;
    run.answer_call("complete_task", false, true).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::Completed);
}

#[tokio::test]
async fn release_while_idle_ends_the_run_with_end_turn() {
    let Some(mut run) = start(TWO_TURNS, None) else {
        return;
    };
    run.phase(RunPhase::AwaitingInput).await;
    run.send(control_message::Body::Release(Release {})).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn cancel_while_idle_ends_the_run_cancelled() {
    let Some(mut run) = start(TWO_TURNS, None) else {
        return;
    };
    run.phase(RunPhase::AwaitingInput).await;
    run.send(control_message::Body::Cancel(
        df_proto::agent::v1::Cancel::default(),
    ))
    .await;
    assert_eq!(run.finish().await.outcome, RunOutcome::Cancelled);
}

#[tokio::test]
async fn wall_clock_budget_ending_while_idle_is_end_turn_not_limit_reached() {
    // Real time is unavoidable: the budget is a wall-clock deadline of the harness run; 3 s is
    // the shortest that leaves room for the first turn.
    let Some(mut run) = start(TWO_TURNS, Some(3)) else {
        return;
    };
    run.phase(RunPhase::AwaitingInput).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn a_lost_control_plane_stream_does_not_end_the_wait() {
    use crate::connection::{ConnectionManager, ReconnectPolicy};
    use crate::test_harness::{MockControlPlane, TestPki};

    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).unwrap();
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let tls_dir = temp.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).unwrap();
    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir,
        data_dir: temp.path().join("data"),
        run_dir: temp.path().join("conn-runtime"),
    };
    let conn_outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&config.run_dir, "run-idle", 1024 * 1024, 64 * 1024).unwrap(),
    ));
    let policy = ReconnectPolicy {
        initial_delay: Duration::from_millis(20),
        max_delay: Duration::from_millis(100),
        backoff_factor: 1.5,
    };
    let handle = ConnectionManager::new(config, conn_outbox, policy).spawn();

    let hello_ack = || ControlMessage {
        ack: 0,
        body: Some(control_message::Body::HelloAck(
            df_proto::agent::v1::HelloAck {
                run_id: "run-idle".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            },
        )),
    };
    // Wait on conditions, not time: every step below waits for the message that proves it.
    let wait = Duration::from_secs(60);
    let hello = async |server: &mut MockControlPlane| {
        tokio::time::timeout(wait, async {
            while let Some(msg) = server.recv_agent_message().await {
                if matches!(msg.body, Some(agent_message::Body::Hello(_))) {
                    return;
                }
            }
        })
        .await
        .expect("Hello in time");
    };
    hello(&mut server).await;
    server.send_control_message(hello_ack()).await.unwrap();

    let Some(driver) = params(
        &temp,
        TWO_TURNS,
        None,
        handle.inbound_rx,
        handle.outbound_tx.clone(),
    ) else {
        return;
    };
    let run = tokio::spawn(async move { run_session(driver).await.unwrap() });

    // The runtime is idle once it reports AWAITING_INPUT.
    tokio::time::timeout(wait, async {
        while let Some(msg) = server.recv_agent_message().await {
            if matches!(
                &msg.body,
                Some(agent_message::Body::PhaseChanged(p)) if p.phase() == RunPhase::AwaitingInput
            ) {
                return;
            }
        }
    })
    .await
    .expect("AWAITING_INPUT in time");

    // The stream drops while idle; the runtime reconnects and the run is still waiting.
    server.drop_active_stream().await;
    hello(&mut server).await;
    server.send_control_message(hello_ack()).await.unwrap();

    // The wait survived: a prompt after the reconnect starts the scripted second turn. Replayed
    // messages of the first turn cannot satisfy this, only the second turn's answer can.
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::Prompt(text_prompt("prompt-2"))),
        })
        .await
        .unwrap();
    tokio::time::timeout(wait, async {
        while let Some(msg) = server.recv_agent_message().await {
            if chunk_text(&msg) == Some("second answer") {
                return;
            }
        }
    })
    .await
    .expect("second answer in time");

    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::Release(Release {})),
        })
        .await
        .unwrap();
    let result = tokio::time::timeout(wait, run).await.unwrap().unwrap();
    assert_eq!(result.outcome, RunOutcome::EndTurn);
    server.shutdown();
}
