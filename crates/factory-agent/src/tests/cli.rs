//! CLI parsing, log format and `log_outbound` log fields.

use clap::Parser;

use super::logged;
use crate::cli::{Cli, Command};

#[test]
fn log_format_defaults_to_text_and_rejects_unknown_values() {
    use df_telemetry::logging::LogFormat;
    let cli = Cli::try_parse_from(["factory-agent", "run"]).unwrap();
    assert_eq!(cli.log_format, LogFormat::Text);
    let cli = Cli::try_parse_from(["factory-agent", "--log-format", "json", "run"]).unwrap();
    assert_eq!(cli.log_format, LogFormat::Json);
    // The error must name DF_LOG_FORMAT and the valid values. (Same error path as the env var;
    // setting env vars in a parallel test run is racy and `unsafe` is forbidden.)
    let err = Cli::try_parse_from(["factory-agent", "--log-format", "logfmt", "run"])
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("DF_LOG_FORMAT") && err.contains("text") && err.contains("json"),
        "{err}"
    );
}

#[test]
fn log_assigned_and_prompt_received_fields() {
    use df_proto::agent::v1::{Assign, Prompt};
    let (out, _guard) = df_telemetry::testing::capture("info");
    crate::log_assigned(&Assign {
        run_id: "r1".into(),
        task: Some(df_proto::agent::v1::TaskRef {
            task_key: "DF-1".into(),
            ..Default::default()
        }),
        ..Default::default()
    });
    crate::runner::log_prompt_received(&Prompt {
        prompt_id: "p1".into(),
        ..Default::default()
    });
    let lines: Vec<serde_json::Value> = out
        .text()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["fields"]["message"], "run assigned");
    assert_eq!(lines[0]["fields"]["run_id"], "r1");
    assert_eq!(lines[0]["fields"]["task_key"], "DF-1");
    assert_eq!(lines[1]["fields"]["message"], "prompt received");
    assert_eq!(lines[1]["fields"]["prompt_id"], "p1");
}

#[test]
fn log_outbound_cuts_long_chunk_text() {
    use df_proto::transcript::v1::{
        AgentMessageChunk, ContentBlock, TextContent, content_block, event::Kind,
    };
    let body = event_body(
        "t1",
        Kind::AgentMessageChunk(AgentMessageChunk {
            message_id: "m1".into(),
            content: Some(ContentBlock {
                kind: Some(content_block::Kind::Text(TextContent {
                    text: "x".repeat(5000),
                })),
                hidden: false,
            }),
        }),
    );
    let lines = logged("debug", &body);
    let f = &lines[0]["fields"];
    assert_eq!(
        f["text"].as_str().unwrap().len(),
        crate::runner::LOG_FIELD_MAX_BYTES
    );
    assert_eq!(f["text_truncated"], true);
    let short = logged(
        "debug",
        &event_body("t1", Kind::AgentMessageChunk(AgentMessageChunk::default())),
    );
    assert!(short[0]["fields"].get("text_truncated").is_none());
}

fn event_body(
    turn_id: &str,
    kind: df_proto::transcript::v1::event::Kind,
) -> df_proto::agent::v1::agent_message::Body {
    df_proto::agent::v1::agent_message::Body::TranscriptEvent(df_proto::transcript::v1::Event {
        schema_version: 1,
        occurred_at: None,
        turn_id: turn_id.to_string(),
        kind: Some(kind),
    })
}

fn tool_call_body(raw_input: &str, raw_output: &str) -> df_proto::agent::v1::agent_message::Body {
    use df_proto::transcript::v1::{ToolCall, ToolKind, ToolOrigin, event::Kind};
    event_body(
        "t1",
        Kind::ToolCall(ToolCall {
            tool_call_id: "c1".into(),
            title: "Run ls".into(),
            kind: ToolKind::Execute as i32,
            raw_input_json: raw_input.into(),
            raw_output_json: raw_output.into(),
            origin: ToolOrigin::Harness as i32,
            ..Default::default()
        }),
    )
}

#[test]
fn log_outbound_tool_call_fields_and_no_output() {
    let lines = logged("info", &tool_call_body(r#"{"cmd":"ls"}"#, "SECRET-OUTPUT"));
    assert_eq!(lines.len(), 1);
    let f = &lines[0]["fields"];
    assert_eq!(f["message"], "tool call");
    assert_eq!(f["turn_id"], "t1");
    assert_eq!(f["tool_call_id"], "c1");
    assert_eq!(f["title"], "Run ls");
    assert_eq!(f["kind"], "Execute");
    assert_eq!(f["origin"], "Harness");
    assert_eq!(f["input"], r#"{"cmd":"ls"}"#);
    assert!(f.get("input_truncated").is_none());
    assert!(!lines[0].to_string().contains("SECRET-OUTPUT"));
}

#[test]
fn log_outbound_cuts_long_tool_input_on_char_boundary() {
    let long = "ä".repeat(5 * 1024); // 10 KiB, 2 bytes per char
    let lines = logged("info", &tool_call_body(&long, ""));
    let f = &lines[0]["fields"];
    let input = f["input"].as_str().unwrap();
    assert!(input.len() <= crate::runner::LOG_FIELD_MAX_BYTES && !input.is_empty());
    assert_eq!(f["input_truncated"], true);
}

#[test]
fn log_outbound_tool_call_update_level_depends_on_status() {
    use df_proto::transcript::v1::{ToolCallStatus, ToolCallUpdate, event::Kind};
    let with_status = event_body(
        "t1",
        Kind::ToolCallUpdate(ToolCallUpdate {
            tool_call_id: "c1".into(),
            status: Some(ToolCallStatus::Completed as i32),
            raw_output_json: Some("SECRET-OUTPUT".into()),
            ..Default::default()
        }),
    );
    let lines = logged("info", &with_status);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["fields"]["message"], "tool call status changed");
    assert_eq!(lines[0]["fields"]["status"], "Completed");
    assert!(!lines[0].to_string().contains("SECRET-OUTPUT"));

    let without = event_body(
        "t1",
        Kind::ToolCallUpdate(ToolCallUpdate {
            tool_call_id: "c1".into(),
            ..Default::default()
        }),
    );
    assert!(logged("info", &without).is_empty());
    let lines = logged("debug", &without);
    assert_eq!(lines[0]["level"], "DEBUG");
}

#[test]
fn log_outbound_chunks_only_at_debug() {
    use df_proto::transcript::v1::{
        AgentMessageChunk, AgentThoughtChunk, ContentBlock, TextContent, content_block, event::Kind,
    };
    let text = Some(ContentBlock {
        kind: Some(content_block::Kind::Text(TextContent {
            text: "hello chunk".into(),
        })),
        hidden: false,
    });
    let message = event_body(
        "t1",
        Kind::AgentMessageChunk(AgentMessageChunk {
            message_id: "m1".into(),
            content: text.clone(),
        }),
    );
    let thought = event_body(
        "t1",
        Kind::AgentThoughtChunk(AgentThoughtChunk {
            message_id: "m2".into(),
            content: text,
        }),
    );
    assert!(logged("info", &message).is_empty());
    assert!(logged("info", &thought).is_empty());
    let lines = logged("debug", &message);
    assert_eq!(lines[0]["level"], "DEBUG");
    assert_eq!(lines[0]["fields"]["message_id"], "m1");
    assert_eq!(lines[0]["fields"]["text"], "hello chunk");
    assert_eq!(logged("debug", &thought)[0]["fields"]["message_id"], "m2");
}

#[test]
fn log_outbound_turn_plan_notice_and_reports() {
    use df_proto::agent::v1::{PushReport, PushStatus, RunOutcome, RunReport, agent_message::Body};
    use df_proto::transcript::v1::{
        Notice, NoticeLevel, Plan, PlanEntry, StopReason, TurnEnded, TurnStarted, Usage,
        UserMessage, event::Kind,
    };

    let f = |body: &Body| logged("info", body).remove(0);

    let v = f(&event_body(
        "t1",
        Kind::TurnStarted(TurnStarted {
            prompt_ids: vec!["p1".into(), "p2".into()],
        }),
    ));
    assert_eq!(
        (
            v["fields"]["message"].as_str(),
            v["fields"]["prompts"].as_u64()
        ),
        (Some("turn started"), Some(2))
    );

    let v = f(&event_body(
        "t1",
        Kind::UserMessage(UserMessage {
            prompt_id: "p1".into(),
            ..Default::default()
        }),
    ));
    assert_eq!(v["fields"]["message"], "user message delivered");
    assert_eq!(v["fields"]["prompt_id"], "p1");

    let v = f(&event_body(
        "t1",
        Kind::Plan(Plan {
            entries: vec![PlanEntry::default(); 3],
        }),
    ));
    assert_eq!(
        (
            v["fields"]["message"].as_str(),
            v["fields"]["entries"].as_u64()
        ),
        (Some("plan updated"), Some(3))
    );

    let ended = |detail: &str| {
        event_body(
            "t1",
            Kind::TurnEnded(TurnEnded {
                stop_reason: StopReason::EndTurn as i32,
                usage: vec![
                    Usage {
                        output_tokens: 10,
                        ..Default::default()
                    },
                    Usage {
                        output_tokens: 5,
                        ..Default::default()
                    },
                ],
                detail: detail.into(),
                ..Default::default()
            }),
        )
    };
    let v = f(&ended(""));
    assert_eq!(v["fields"]["stop_reason"], "EndTurn");
    assert_eq!(v["fields"]["output_tokens"], 15);
    assert!(v["fields"].get("detail").is_none());
    assert_eq!(f(&ended("slow down"))["fields"]["detail"], "slow down");

    let notice = |level: NoticeLevel| {
        event_body(
            "",
            Kind::Notice(Notice {
                level: level as i32,
                code: "x.y".into(),
                message: "m".into(),
            }),
        )
    };
    assert_eq!(f(&notice(NoticeLevel::Info))["level"], "INFO");
    assert_eq!(f(&notice(NoticeLevel::Warning))["level"], "WARN");
    let v = f(&notice(NoticeLevel::Error));
    assert_eq!(
        (v["level"].as_str(), v["fields"]["code"].as_str()),
        (Some("WARN"), Some("x.y"))
    );

    let v = f(&Body::PushReport(PushReport {
        r#ref: "refs/heads/b".into(),
        sha: "abc".into(),
        status: PushStatus::Ok as i32,
        ..Default::default()
    }));
    assert_eq!(v["fields"]["ref"], "refs/heads/b");
    assert_eq!(v["fields"]["sha"], "abc");
    assert_eq!(v["fields"]["status"], "Ok");

    let v = f(&Body::RunReport(RunReport {
        outcome: RunOutcome::Completed as i32,
        detail: "done".into(),
        ..Default::default()
    }));
    assert_eq!(v["fields"]["message"], "run finished");
    assert_eq!(v["fields"]["outcome"], "Completed");
    assert_eq!(v["fields"]["detail"], "done");
}

#[test]
fn parses_subcommands() {
    let cli = Cli::try_parse_from(["factory-agent", "run"]).unwrap();
    assert_eq!(cli.command, Command::Run);

    let cli = Cli::try_parse_from(["factory-agent", "mcp"]).unwrap();
    assert_eq!(
        cli.command,
        Command::Mcp {
            socket: std::path::PathBuf::from("/run/factory/runtime.sock")
        }
    );

    let cli = Cli::try_parse_from(["factory-agent", "git-credential", "get"]).unwrap();
    assert_eq!(
        cli.command,
        Command::GitCredential {
            operation: Some("get".to_string()),
            socket: std::path::PathBuf::from("/run/factory/runtime.sock")
        }
    );
}
