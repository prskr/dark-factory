//! Run outcomes and `run_session` scenarios.

#![allow(
    clippy::too_many_lines,
    reason = "scenario tests set up and check a whole run"
)]

use super::RunScenario;

#[test]
fn test_determine_run_outcome_priority() {
    use crate::harness::{
        RateLimitInfo, RunOutcomeDecision, TurnOutcomeInput, determine_run_outcome,
    };
    use df_proto::agent::v1::RunOutcome;
    use df_proto::transcript::v1::StopReason;

    // 1. Cancelled has highest priority
    let input = TurnOutcomeInput {
        cancelled: true,
        end_run_after_turn: true,
        has_queued_prompts: true,
        ..Default::default()
    };
    assert_eq!(
        determine_run_outcome(&input),
        RunOutcomeDecision::Finalize {
            outcome: RunOutcome::Cancelled,
            detail: "Run cancelled by control plane".to_string(),
            retry_after: None,
        }
    );

    // 2. complete_task called has priority over queued prompts
    let input_complete = TurnOutcomeInput {
        end_run_after_turn: true,
        has_queued_prompts: true,
        ..Default::default()
    };
    assert_eq!(
        determine_run_outcome(&input_complete),
        RunOutcomeDecision::Finalize {
            outcome: RunOutcome::Completed,
            detail: "Task completed".to_string(),
            retry_after: None,
        }
    );

    // 3. Provider rate limit
    let input_rl = TurnOutcomeInput {
        rate_limited: Some(RateLimitInfo {
            retry_after_seconds: Some(3600),
            detail: "Subscription quota exceeded".to_string(),
        }),
        has_queued_prompts: true,
        ..Default::default()
    };
    assert_eq!(
        determine_run_outcome(&input_rl),
        RunOutcomeDecision::Finalize {
            outcome: RunOutcome::RateLimited,
            detail: "Subscription quota exceeded".to_string(),
            retry_after: Some(std::time::Duration::from_secs(3600)),
        }
    );

    // 4. Budgets: wall clock
    let input_wc = TurnOutcomeInput {
        wall_clock_exceeded: true,
        has_queued_prompts: true,
        ..Default::default()
    };
    assert_eq!(
        determine_run_outcome(&input_wc),
        RunOutcomeDecision::Finalize {
            outcome: RunOutcome::LimitReached,
            detail: "Wall clock budget exceeded".to_string(),
            retry_after: None,
        }
    );

    // 5. Budgets: output tokens
    let input_tok = TurnOutcomeInput {
        output_tokens_exceeded: true,
        has_queued_prompts: true,
        ..Default::default()
    };
    assert_eq!(
        determine_run_outcome(&input_tok),
        RunOutcomeDecision::Finalize {
            outcome: RunOutcome::LimitReached,
            detail: "Output token budget exceeded".to_string(),
            retry_after: None,
        }
    );

    // 6. StopReason: MaxTokens / MaxTurnRequests
    let input_mt = TurnOutcomeInput {
        stop_reason: Some(StopReason::MaxTokens),
        has_queued_prompts: true,
        ..Default::default()
    };
    assert_eq!(
        determine_run_outcome(&input_mt),
        RunOutcomeDecision::Finalize {
            outcome: RunOutcome::LimitReached,
            detail: "Stop reason limit reached".to_string(),
            retry_after: None,
        }
    );

    // 7. Harness crashed
    let input_crash = TurnOutcomeInput {
        harness_crashed: true,
        has_queued_prompts: true,
        ..Default::default()
    };
    assert_eq!(
        determine_run_outcome(&input_crash),
        RunOutcomeDecision::Finalize {
            outcome: RunOutcome::Failed,
            detail: "Harness crashed or exited with error".to_string(),
            retry_after: None,
        }
    );

    // 8. Turn limit reached when next turn would start
    let input_turn_limit = TurnOutcomeInput {
        has_queued_prompts: true,
        turn_limit_reached: true,
        ..Default::default()
    };
    assert_eq!(
        determine_run_outcome(&input_turn_limit),
        RunOutcomeDecision::Finalize {
            outcome: RunOutcome::LimitReached,
            detail: "Turn budget reached".to_string(),
            retry_after: None,
        }
    );

    // 9. Queued prompts continue next turn
    let input_continue = TurnOutcomeInput {
        has_queued_prompts: true,
        turn_limit_reached: false,
        ..Default::default()
    };
    assert_eq!(
        determine_run_outcome(&input_continue),
        RunOutcomeDecision::ContinueNextTurn
    );

    // 10. A normal turn end without queued prompts waits for input instead of ending the run
    let input_end_turn = TurnOutcomeInput {
        has_queued_prompts: false,
        ..Default::default()
    };
    assert_eq!(
        determine_run_outcome(&input_end_turn),
        RunOutcomeDecision::AwaitInput
    );
}

#[tokio::test]
async fn test_scenario_outcome_end_turn_when_agent_asks_question() {
    use df_proto::agent::v1::RunOutcome;

    let script = r#"
sessions:
  new:
    - - say: "Which database should we use?"
      - end: end_turn
"#;
    let Some(scenario) = RunScenario::new("run-end-turn", script) else {
        return;
    };
    let (result, _) = scenario.release_up_front().run(|_, _| {}).await;
    assert_eq!(result.outcome, RunOutcome::EndTurn);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::EndTurn as i32
    );
}

#[tokio::test]
async fn test_scenario_outcome_complete_task_called() {
    use df_proto::agent::v1::{
        ControlMessage, Prompt, RunOutcome, ToolCallResult, ToolDescriptor, agent_message,
        control_message,
    };

    let (logs, _log_guard) = df_telemetry::testing::capture("info");

    let script = r#"
sessions:
  new:
    - - mcp:
          server: factory
          tool: complete_task
          args:
            summary: "Work is fully done"
      - say: "Finished and called complete_task"
      - end: end_turn
"#;
    let Some(scenario) = RunScenario::new("run-complete-task", script) else {
        return;
    };
    let tools = vec![ToolDescriptor {
        name: "complete_task".to_string(),
        title: "Complete Task".to_string(),
        description: "Mark task complete".to_string(),
        input_schema_json: "{}".to_string(),
        output_schema_json: None,
        annotations: None,
    }];

    // Mock control plane: answer complete_task with end_run_after_turn = true
    // AND queue an extra prompt to verify complete_task overrides queued prompts!
    let answer_complete_task =
        |msg: &df_proto::agent::v1::AgentMessage,
         control: &tokio::sync::mpsc::Sender<ControlMessage>| {
            if let Some(agent_message::Body::ToolCallRequest(req)) = &msg.body
                && req.tool_name == "complete_task"
            {
                // Send tool result with end_run_after_turn = true
                let res = ControlMessage {
                    ack: 0,
                    body: Some(control_message::Body::ToolCallResult(ToolCallResult {
                        call_id: req.call_id.clone(),
                        is_error: false,
                        content: vec![df_proto::transcript::v1::ContentBlock {
                            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                                df_proto::transcript::v1::TextContent {
                                    text: "Task marked completed".to_string(),
                                },
                            )),
                            hidden: false,
                        }],
                        structured_content_json: String::new(),
                        end_run_after_turn: true,
                        await_input_after_turn: false,
                    })),
                };
                control.try_send(res).unwrap();

                // Also queue a prompt to verify COMPLETED takes precedence over queued prompt
                let prompt_msg = ControlMessage {
                    ack: 0,
                    body: Some(control_message::Body::Prompt(Prompt {
                        prompt_id: "extra-prompt-queued".to_string(),
                        author: None,
                        content: vec![],
                        queued_at: None,
                    })),
                };
                control.try_send(prompt_msg).unwrap();
            }
        };
    let (result, _) = scenario
        .prompt("Complete the task")
        .tools(tools)
        .run(answer_complete_task)
        .await;
    assert_eq!(result.outcome, RunOutcome::Completed);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::Completed as i32
    );

    // Session events appear in order; no prompt content, tool output or credentials are logged.
    let log = logs.text();
    let messages: Vec<String> = log
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["fields"]["message"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    let mut pos = 0;
    for expected in [
        "harness started",
        "harness session ready",
        "turn started",
        "user message delivered",
        "tool call",
        "tool call status changed",
        "turn ended",
        "run finished",
    ] {
        let found = messages[pos..].iter().position(|m| m == expected);
        assert!(
            found.is_some(),
            "missing {expected:?} after {pos} in {messages:?}"
        );
        pos += found.unwrap() + 1;
    }
    // "One turn": the initial prompt is received before the turn starts. (The extra prompt
    // arrives mid-turn, so only the first "prompt received" has a fixed position.)
    let at = |m: &str| {
        messages
            .iter()
            .position(|x| x == m)
            .unwrap_or_else(|| panic!("missing {m:?} in {messages:?}"))
    };
    assert!(at("prompt received") < at("turn started"));
    // "Run start and end": `run assigned` is logged by `main`, which this test does not reach;
    // its fields are covered by `log_assigned_and_prompt_received_fields`.
    assert!(!log.contains("Complete the task"), "prompt text logged");
    assert!(!log.contains("Task marked completed"), "tool output logged");
}

#[tokio::test]
async fn test_scenario_outcome_provider_rate_limited() {
    use df_proto::agent::v1::RunOutcome;

    let script = r#"
sessions:
  new:
    - - end:
          rate_limited:
            retry_after_seconds: 3600
"#;
    let Some(scenario) = RunScenario::new("run-rate-limited", script) else {
        return;
    };
    let (result, _) = scenario.empty_prompt().run(|_, _| {}).await;
    assert_eq!(result.outcome, RunOutcome::RateLimited);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::RateLimited as i32
    );
    assert!(result.run_report.retry_after.is_some());
}

#[tokio::test]
async fn test_scenario_outcome_turn_limit() {
    use df_proto::agent::v1::{
        Budgets, ControlMessage, Prompt, RunOutcome, agent_message, control_message,
    };

    // 2 turns in script, but 3 prompts will be queued
    let script = r#"
sessions:
  new:
    - - say: "Turn 1 answer"
      - end: end_turn
    - - say: "Turn 2 answer"
      - end: end_turn
"#;
    let Some(scenario) = RunScenario::new("run-turn-limit", script) else {
        return;
    };
    // Limit to max 2 turns
    let budgets = Budgets {
        max_wall_clock: None,
        max_output_tokens: None,
        max_turns: Some(2),
    };
    // Queue prompts for turn 2 and a potential turn 3 during execution
    let mut turn_count = 0;
    let queue_prompt =
        move |msg: &df_proto::agent::v1::AgentMessage,
              control: &tokio::sync::mpsc::Sender<ControlMessage>| {
            if let Some(agent_message::Body::TranscriptEvent(ev)) = &msg.body
                && let Some(df_proto::transcript::v1::event::Kind::TurnStarted(_)) = &ev.kind
            {
                turn_count += 1;
                // During turn 1 send the prompt for turn 2, during turn 2 the one for turn 3.
                if turn_count <= 2 {
                    control
                        .try_send(ControlMessage {
                            ack: 0,
                            body: Some(control_message::Body::Prompt(Prompt {
                                prompt_id: format!("prompt-{}", turn_count + 1),
                                author: None,
                                content: vec![],
                                queued_at: None,
                            })),
                        })
                        .unwrap();
                }
            }
        };
    let (result, _) = scenario
        .empty_prompt()
        .budgets(budgets)
        .run(queue_prompt)
        .await;
    assert_eq!(result.outcome, RunOutcome::LimitReached);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::LimitReached as i32
    );
    assert!(result.run_report.detail.contains("Turn budget"));
}

#[tokio::test]
async fn test_scenario_outcome_wall_clock_budget_exceeded() {
    use df_proto::agent::v1::{Budgets, RunOutcome};
    use df_proto::pbjson_types::Duration;

    // Agent hangs until cancelled
    let script = r#"
sessions:
  new:
    - - end: hang
"#;
    let Some(scenario) = RunScenario::new("run-wall-clock-limit", script) else {
        return;
    };
    // Limit wall clock to 200 ms
    let budgets = Budgets {
        max_wall_clock: Some(Duration {
            seconds: 0,
            nanos: 200_000_000,
        }),
        max_output_tokens: None,
        max_turns: None,
    };
    let (result, _) = scenario
        .empty_prompt()
        .budgets(budgets)
        .run(|_, _| {})
        .await;
    assert_eq!(result.outcome, RunOutcome::LimitReached);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::LimitReached as i32
    );
    assert!(result.run_report.detail.contains("Wall clock budget"));
}

#[tokio::test]
async fn test_scenario_outcome_cancelled_by_control_plane() {
    use df_proto::agent::v1::{Cancel, ControlMessage, RunOutcome, control_message};
    use tokio::sync::mpsc;

    // Agent hangs until cancelled
    let script = r#"
sessions:
  new:
    - - end: hang
"#;
    let Some(scenario) = RunScenario::new("run-cancel", script) else {
        return;
    };
    let scenario = scenario.empty_prompt();
    let (inbound_tx, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, mut outbound_rx) = mpsc::channel(100);
    // Keep `scenario` alive: its temp directory backs the run.
    let params = scenario.params(inbound_rx, outbound_tx);

    // After 100ms, send Cancel control message
    let cp_task = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        inbound_tx
            .send(ControlMessage {
                ack: 0,
                body: Some(control_message::Body::Cancel(Cancel {
                    reason: "User requested abort".to_string(),
                })),
            })
            .await
            .expect("runner inbound open");

        while let Some(_msg) = outbound_rx.recv().await {}
    });

    let result = crate::runner::run_session(params)
        .await
        .expect("run_session should succeed");
    assert_eq!(result.outcome, RunOutcome::Cancelled);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::Cancelled as i32
    );
    cp_task.abort();
}

#[tokio::test]
async fn test_scenario_outcome_harness_crashed() {
    use df_proto::agent::v1::RunOutcome;

    // Agent executes exit(1) crash
    let script = r#"
sessions:
  new:
    - - end: crash
"#;
    let Some(scenario) = RunScenario::new("run-crash", script) else {
        return;
    };
    let (result, _) = scenario.empty_prompt().run(|_, _| {}).await;
    assert_eq!(result.outcome, RunOutcome::Failed);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::Failed as i32
    );
}
