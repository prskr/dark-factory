//! Runner tests for the idle wait after a normal turn end (`specs/acp-harness/spec.md`,
//! "Idle wait after a normal turn end"), driven by the fake ACP agent.

use std::time::Duration;

use df_proto::agent::v1::{
    AgentMessage, Budgets, ControlMessage, Release, RunOutcome, RunPhase, ToolCallResult,
    ToolDescriptor, agent_message, control_message,
};
use df_proto::transcript::v1::event::Kind as EventKind;
use tokio::sync::mpsc;

use super::{RunScenario, prompts_body, text_prompt};
use crate::outbox::Outbox;
use crate::runner::{RunExecutionResult, Timeouts, run_session};

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
    _scenario: RunScenario,
}

/// Reads agent messages until the `Hello`.
async fn next_hello(server: &mut crate::test_harness::MockControlPlane) {
    while let Some(msg) = server.recv_agent_message().await {
        if matches!(msg.body, Some(agent_message::Body::Hello(_))) {
            return;
        }
    }
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

/// A run with `script` and one initial prompt. `None` when the helper binaries are not built
/// (same convention as the other runner tests).
fn scenario(script: &str, max_wall_clock_secs: Option<i64>) -> Option<RunScenario> {
    let tools = ["ask_question", "complete_task"].map(|name| ToolDescriptor {
        name: name.to_string(),
        title: name.to_string(),
        description: name.to_string(),
        input_schema_json: "{}".to_string(),
        ..Default::default()
    });
    let scenario = RunScenario::new("run-idle", script)?
        .prompt("go")
        .tools(tools.to_vec());
    Some(match max_wall_clock_secs {
        Some(seconds) => scenario.budgets(Budgets {
            max_wall_clock: Some(df_proto::pbjson_types::Duration { seconds, nanos: 0 }),
            ..Default::default()
        }),
        None => scenario,
    })
}

/// Starts a run with `script` and the control plane's side of its channels.
fn start(script: &str, max_wall_clock_secs: Option<i64>) -> Option<IdleRun> {
    Some(launch(scenario(script, max_wall_clock_secs)?))
}

/// Starts the run of `scenario`.
fn launch(scenario: RunScenario) -> IdleRun {
    launch_with_outbound_capacity(scenario, 1000)
}

/// Starts the run of `scenario` with an outbound channel of `capacity` messages.
fn launch_with_outbound_capacity(scenario: RunScenario, capacity: usize) -> IdleRun {
    let (control, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, outbound) = mpsc::channel(capacity);
    let params = scenario.params(inbound_rx, outbound_tx);
    let run = tokio::spawn(async move { run_session(params).await.unwrap() });
    IdleRun {
        control,
        outbound,
        run,
        seen: Vec::new(),
        _scenario: scenario,
    }
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
async fn a_prompt_batch_while_idle_starts_one_turn_with_all_its_prompt_ids() {
    let Some(mut run) = start(TWO_TURNS, None) else {
        return;
    };
    run.phase(RunPhase::AwaitingInput).await;
    run.send(prompts_body(vec![
        text_prompt("prompt-2", "go"),
        text_prompt("prompt-3", "go"),
    ]))
    .await;

    let prompt_ids = run
        .next(|body| match body {
            agent_message::Body::TranscriptEvent(event) => match &event.kind {
                Some(EventKind::TurnStarted(turn)) => Some(turn.prompt_ids.clone()),
                _ => None,
            },
            _ => None,
        })
        .await;
    assert_eq!(prompt_ids, ["prompt-2", "prompt-3"]);
    run.turn_ended().await;
    run.phase(RunPhase::AwaitingInput).await;
    run.send(control_message::Body::Release(Release {})).await;

    let turns = run
        .seen
        .iter()
        .filter(|message| {
            matches!(
                &message.body,
                Some(agent_message::Body::TranscriptEvent(event))
                    if matches!(event.kind, Some(EventKind::TurnStarted(_)))
            )
        })
        .count();
    assert_eq!(turns, 2);
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn follow_up_while_idle_starts_the_next_turn_in_the_same_session() {
    let Some(mut run) = start(TWO_TURNS, None) else {
        return;
    };
    run.phase(RunPhase::AwaitingInput).await;
    run.send(prompts_body(vec![text_prompt("prompt-2", "go")]))
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
    run.send(prompts_body(vec![text_prompt("prompt-2", "go")]))
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
        layout: crate::config::Layout {
            tls_dir,
            data_dir: temp.path().join("data"),
            run_dir: temp.path().join("conn-runtime"),
        },
        landlock: df_core::LandlockMode::Enforce,
    };
    let conn_outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&config.layout.run_dir, "run-idle", 1024 * 1024, 64 * 1024).unwrap(),
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
        tokio::time::timeout(wait, next_hello(server))
            .await
            .expect("Hello in time");
    };
    hello(&mut server).await;
    server.send_control_message(hello_ack()).await.unwrap();

    let Some(scenario) = scenario(TWO_TURNS, None) else {
        return;
    };
    let driver = scenario.params(handle.inbound_rx, handle.outbound_tx.clone());
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
            body: Some(prompts_body(vec![text_prompt("prompt-2", "go")])),
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

/// A turn that stays open until `session/cancel`, then two turns that report what their prompt
/// said.
const INTERRUPTIBLE: &str = r#"
sessions:
  new:
    - - say: "working"
      - end: hang
    - - when:
          prompt_contains: "was interrupted by Peter Kurfer."
          steps:
            - say: "saw interrupt line"
      - when:
          prompt_contains: "other API"
          steps:
            - say: "saw message"
      - end: end_turn
"#;

/// An `Interrupt` for the first turn.
fn interrupt(by: &str) -> control_message::Body {
    interrupt_of_turn(by, "turn_1")
}

fn interrupt_of_turn(by: &str, turn_id: &str) -> control_message::Body {
    control_message::Body::Interrupt(df_proto::agent::v1::Interrupt {
        by: by.into(),
        turn_id: turn_id.into(),
    })
}

/// The `TurnEnded` events seen so far.
fn turn_endings(seen: &[AgentMessage]) -> Vec<&df_proto::transcript::v1::TurnEnded> {
    seen.iter()
        .filter_map(|message| match &message.body {
            Some(agent_message::Body::TranscriptEvent(event)) => match &event.kind {
                Some(EventKind::TurnEnded(ended)) => Some(ended),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Waits until the agent's turn is open ("working" was said).
async fn turn_is_open(run: &mut IdleRun) {
    run.next(|b| match b {
        agent_message::Body::TranscriptEvent(e) => match &e.kind {
            Some(EventKind::AgentMessageChunk(_)) => Some(()),
            _ => None,
        },
        _ => None,
    })
    .await;
}

#[tokio::test]
async fn interrupt_ends_the_turn_and_the_run_waits_for_input() {
    let Some(mut run) = start(INTERRUPTIBLE, None) else {
        return;
    };
    turn_is_open(&mut run).await;
    run.send(interrupt("Peter Kurfer")).await;
    run.turn_ended().await;
    run.phase(RunPhase::AwaitingInput).await;

    let ended = turn_endings(&run.seen);
    assert_eq!(ended.len(), 1);
    let ended = ended.first().unwrap();
    assert_eq!(
        ended.stop_reason(),
        df_proto::transcript::v1::StopReason::Cancelled
    );
    assert_eq!(ended.detail, "Interrupted by Peter Kurfer");
    assert_eq!(ended.interrupted_by, "Peter Kurfer");

    run.send(control_message::Body::Release(Release {})).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn the_turn_after_an_interrupt_starts_with_the_interrupt_line_before_the_queued_prompt() {
    let Some(mut run) = start(INTERRUPTIBLE, None) else {
        return;
    };
    turn_is_open(&mut run).await;
    run.send(prompts_body(vec![text_prompt(
        "prompt-2",
        "use the other API",
    )]))
    .await;
    run.send(interrupt("Peter Kurfer")).await;
    // The queued prompt starts the next turn right away, without an idle wait.
    run.turn_ended().await;
    run.turn_ended().await;
    run.phase(RunPhase::AwaitingInput).await;
    let said: Vec<&str> = run.seen.iter().filter_map(chunk_text).collect();
    assert_eq!(said, ["working", "saw interrupt line", "saw message"]);

    run.send(control_message::Body::Release(Release {})).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn an_interrupted_turn_counts_toward_max_turns_and_is_no_failure() {
    let script = r#"
sessions:
  new:
    - - say: "working"
      - end: hang
    - - say: "second"
      - end: end_turn
"#;
    let Some(scenario) = scenario(script, None) else {
        return;
    };
    let mut run = launch(scenario.budgets(Budgets {
        max_turns: Some(1),
        ..Default::default()
    }));
    turn_is_open(&mut run).await;
    run.send(prompts_body(vec![text_prompt("prompt-2", "go")]))
        .await;
    run.send(interrupt("Peter Kurfer")).await;
    // One turn is the whole budget: the queued prompt does not start another one.
    assert_eq!(run.finish().await.outcome, RunOutcome::LimitReached);
}

#[tokio::test]
async fn interrupt_while_idle_changes_nothing() {
    let Some(mut run) = start(TWO_TURNS, None) else {
        return;
    };
    run.phase(RunPhase::AwaitingInput).await;
    run.send(interrupt("Peter Kurfer")).await;
    run.send(prompts_body(vec![text_prompt("prompt-2", "go")]))
        .await;
    run.phase(RunPhase::Running).await;
    run.turn_ended().await;
    run.phase(RunPhase::AwaitingInput).await;
    let said: Vec<&str> = run.seen.iter().filter_map(chunk_text).collect();
    assert_eq!(said, ["first answer", "second answer"]);
    assert!(
        turn_endings(&run.seen)
            .iter()
            .all(|e| e.interrupted_by.is_empty())
    );
    run.send(control_message::Body::Release(Release {})).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn an_open_tool_call_gets_a_cancelled_result_and_a_late_result_is_dropped() {
    let script = r#"
sessions:
  new:
    - - mcp: { server: factory, tool: ask_question, args: { text: "Which?" } }
      - end: hang
"#;
    let Some(mut run) = start(script, None) else {
        return;
    };
    let call_id = run
        .next(|b| match b {
            agent_message::Body::ToolCallRequest(r) => Some(r.call_id.clone()),
            _ => None,
        })
        .await;
    run.send(interrupt("Peter Kurfer")).await;
    run.turn_ended().await;
    run.phase(RunPhase::AwaitingInput).await;

    // The harness's call returned with the cancelled result.
    let tool_results: Vec<String> = run
        .seen
        .iter()
        .filter_map(|message| match &message.body {
            Some(agent_message::Body::TranscriptEvent(event))
                if !matches!(event.kind, Some(EventKind::TurnEnded(_))) =>
            {
                Some(format!("{event:?}"))
            }
            _ => None,
        })
        .filter(|event| event.contains("Interrupted by Peter Kurfer"))
        .collect();
    assert!(
        !tool_results.is_empty(),
        "no cancelled tool result in the transcript"
    );

    // The late result finds no open call; the run keeps waiting.
    run.send(control_message::Body::ToolCallResult(ToolCallResult {
        call_id,
        ..Default::default()
    }))
    .await;
    run.send(control_message::Body::Release(Release {})).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn an_interrupt_sent_while_the_turn_start_events_go_out_still_ends_the_turn() {
    let Some(scenario) = scenario(INTERRUPTIBLE, None) else {
        return;
    };
    // A channel of one message makes the runtime wait for room while it sends the turn-start
    // events: `TurnStarted` fits after the `RUNNING` phase was read, the `UserMessage` after it
    // waits. That is the window in which the control plane, which persisted `TurnStarted`
    // first, can already send `Interrupt`: the runtime is before `await_prompt`.
    let mut run = launch_with_outbound_capacity(scenario, 1);
    run.phase(RunPhase::Running).await;
    run.send(interrupt("Peter Kurfer")).await;
    // Barrier: the inbound loop has taken the message from the channel (which frees its slot)
    // and handled it; the runtime has not moved, it still waits for room.
    tokio::time::timeout(Duration::from_secs(60), async {
        while run.control.capacity() < 100 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the inbound loop takes the interrupt");
    run.turn_ended().await;
    run.phase(RunPhase::AwaitingInput).await;

    let ended = turn_endings(&run.seen);
    assert_eq!(ended.first().unwrap().interrupted_by, "Peter Kurfer");
    run.send(control_message::Body::Release(Release {})).await;
    // The small channel needs a reader until the runtime is done.
    while run.outbound.recv().await.is_some() {}
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn a_harness_that_fails_the_cancel_ends_the_turn_as_an_error_without_claiming_an_interrupt() {
    let script = r#"
sessions:
  new:
    - - say: "working"
      - end: hang_then_crash
"#;
    let Some(mut run) = start(script, None) else {
        return;
    };
    turn_is_open(&mut run).await;
    run.send(interrupt("Peter Kurfer")).await;
    run.turn_ended().await;

    let ended = turn_endings(&run.seen);
    let ended = ended.first().unwrap();
    assert_eq!(
        ended.stop_reason(),
        df_proto::transcript::v1::StopReason::Error
    );
    assert!(ended.interrupted_by.is_empty(), "{ended:?}");
    assert_eq!(run.finish().await.outcome, RunOutcome::Failed);
}

#[tokio::test]
async fn a_harness_that_ignores_the_cancel_ends_the_turn_as_an_error_after_the_timeout() {
    let script = r#"
sessions:
  new:
    - - say: "working"
      - end: hang_forever
"#;
    let Some(scenario) = scenario(script, None) else {
        return;
    };
    // The harness never answers: shorten the waits that would otherwise take 5 s each.
    let mut run = launch(scenario.timeouts(Timeouts {
        cancel_response: Duration::from_millis(100),
        harness_exit_grace: Duration::from_millis(100),
    }));
    turn_is_open(&mut run).await;
    run.send(interrupt("Peter Kurfer")).await;
    run.turn_ended().await;

    let ended = turn_endings(&run.seen);
    let ended = ended.first().unwrap();
    assert_eq!(
        ended.stop_reason(),
        df_proto::transcript::v1::StopReason::Error
    );
    assert!(ended.interrupted_by.is_empty(), "{ended:?}");
    assert_eq!(run.finish().await.outcome, RunOutcome::Failed);
}

#[tokio::test]
async fn a_fresh_run_reports_the_running_phase_before_its_first_turn_starts() {
    let Some(mut run) = start(TWO_TURNS, None) else {
        return;
    };
    run.turn_ended().await;

    let position = |wanted: fn(&agent_message::Body) -> bool| {
        run.seen
            .iter()
            .position(|message| message.body.as_ref().is_some_and(wanted))
    };
    let running = position(
        |b| matches!(b, agent_message::Body::PhaseChanged(p) if p.phase() == RunPhase::Running),
    );
    let turn_started = position(
        |b| matches!(b, agent_message::Body::TranscriptEvent(e) if matches!(e.kind, Some(EventKind::TurnStarted(_)))),
    );
    assert!(running.is_some(), "no Running phase was reported");
    assert!(running < turn_started, "{running:?} {turn_started:?}");
    run.phase(RunPhase::AwaitingInput).await;
    run.send(control_message::Body::Release(Release {})).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn an_interrupt_for_another_turn_is_ignored_and_the_turn_goes_on() {
    let Some(mut run) = start(INTERRUPTIBLE, None) else {
        return;
    };
    turn_is_open(&mut run).await;
    // Stale: it names a turn that is not the open one. The inbound loop handles the messages in
    // order, so the turn would end with "Stale" if it were honored.
    run.send(interrupt_of_turn("Stale", "turn_9")).await;
    run.send(interrupt("Peter Kurfer")).await;
    run.turn_ended().await;
    run.phase(RunPhase::AwaitingInput).await;

    let ended = turn_endings(&run.seen);
    assert_eq!(ended.len(), 1);
    assert_eq!(ended.first().unwrap().interrupted_by, "Peter Kurfer");
    run.send(control_message::Body::Release(Release {})).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn a_late_blocking_question_result_after_an_interrupt_does_not_make_the_release_blocked() {
    let script = r#"
sessions:
  new:
    - - mcp: { server: factory, tool: ask_question, args: { text: "Which?", blocking: true } }
      - end: hang
"#;
    let Some(mut run) = start(script, None) else {
        return;
    };
    let call_id = run
        .next(|b| match b {
            agent_message::Body::ToolCallRequest(r) => Some(r.call_id.clone()),
            _ => None,
        })
        .await;
    run.send(interrupt("Peter Kurfer")).await;
    run.turn_ended().await;
    run.phase(RunPhase::AwaitingInput).await;

    // The control plane answered the question before it learned of the interrupt.
    run.send(control_message::Body::ToolCallResult(ToolCallResult {
        call_id,
        await_input_after_turn: true,
        ..Default::default()
    }))
    .await;
    run.send(control_message::Body::Release(Release {})).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}

#[tokio::test]
async fn a_turn_that_completes_as_the_cancel_arrives_is_not_recorded_as_interrupted() {
    let script = r#"
sessions:
  new:
    - - say: "working"
      - end: hang_then_end_turn
"#;
    let Some(mut run) = start(script, None) else {
        return;
    };
    turn_is_open(&mut run).await;
    run.send(interrupt("Peter Kurfer")).await;
    run.turn_ended().await;
    run.phase(RunPhase::AwaitingInput).await;

    let ended = turn_endings(&run.seen);
    let ended = ended.first().unwrap();
    assert_eq!(
        ended.stop_reason(),
        df_proto::transcript::v1::StopReason::EndTurn
    );
    assert!(ended.interrupted_by.is_empty(), "{ended:?}");
    run.send(control_message::Body::Release(Release {})).await;
    assert_eq!(run.finish().await.outcome, RunOutcome::EndTurn);
}
