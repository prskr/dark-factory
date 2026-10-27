//! Unit and integration tests for `factory-agent`.

use clap::Parser;
use df_proto::agent::v1::{
    AgentMessage, ControlMessage, Hello, HelloAck, agent_message, control_message,
};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

use crate::cli::{Cli, Command};
use crate::connection::{TlsFiles, connect_channel};
use crate::test_harness::{MockControlPlane, TestPki};

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

/// Logs `body` through `log_outbound` at `filter` and returns the parsed JSON lines.
fn logged(filter: &str, body: &df_proto::agent::v1::agent_message::Body) -> Vec<serde_json::Value> {
    let (out, _guard) = df_telemetry::testing::capture(filter);
    crate::runner::log_outbound(body);
    out.text()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
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

#[tokio::test]
async fn test_mtls_connect_success_with_server_name_verification() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki generation failed");

    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start failed");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    pki.write_client_tls_dir(temp_dir.path())
        .expect("write certs");
    let tls_files = TlsFiles::load_from_dir(temp_dir.path()).expect("load certs");

    let addr_str = format!("https://{}", server.addr);
    let mut client = connect_channel(&addr_str, server_name, &tls_files)
        .await
        .expect("client connect failed");

    let (out_tx, out_rx) = tokio::sync::mpsc::channel(10);
    let out_stream = ReceiverStream::new(out_rx);

    let response = client
        .connect(out_stream)
        .await
        .expect("connect rpc failed");
    let mut in_stream = response.into_inner();

    // 1. Client sends Hello message
    let hello_msg = AgentMessage {
        seq: 0,
        body: Some(agent_message::Body::Hello(Hello {
            runtime_version: "0.1.0".to_string(),
            capabilities: vec!["fake-cap".to_string()],
            previous_run_id: String::new(),
        })),
    };
    out_tx.send(hello_msg.clone()).await.expect("send hello");

    // 2. Server receives Hello
    let received_by_server = server
        .recv_agent_message()
        .await
        .expect("server received message");
    assert_eq!(received_by_server.seq, 0);
    match received_by_server.body {
        Some(agent_message::Body::Hello(h)) => {
            assert_eq!(h.runtime_version, "0.1.0");
            assert_eq!(h.capabilities, vec!["fake-cap".to_string()]);
        }
        other => panic!("expected Hello, got {other:?}"),
    }

    // 3. Server sends HelloAck
    let ack_msg = ControlMessage {
        ack: 0,
        body: Some(control_message::Body::HelloAck(HelloAck {
            run_id: "run-42".to_string(),
            last_persisted_seq: 0,
            capabilities: vec![],
        })),
    };
    server
        .send_control_message(ack_msg)
        .await
        .expect("server send ack");

    // 4. Client receives HelloAck
    let received_by_client = in_stream
        .next()
        .await
        .expect("client received item")
        .expect("client ok");
    match received_by_client.body {
        Some(control_message::Body::HelloAck(ack)) => {
            assert_eq!(ack.run_id, "run-42");
        }
        other => panic!("expected HelloAck, got {other:?}"),
    }

    server.shutdown();
}

#[tokio::test]
async fn test_mtls_connect_fails_on_server_name_mismatch() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki generation failed");

    let server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start failed");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    pki.write_client_tls_dir(temp_dir.path())
        .expect("write certs");
    let tls_files = TlsFiles::load_from_dir(temp_dir.path()).expect("load certs");

    let addr_str = format!("https://{}", server.addr);
    // Configured client server_name does NOT match the certificate SAN
    let wrong_server_name = "rogue-server.darkfactory.local";

    let connect_res = connect_channel(&addr_str, wrong_server_name, &tls_files).await;
    match connect_res {
        Err(e) => {
            // Expected connection / TLS handshake failure
            let err_str = e.to_string();
            assert!(
                err_str.contains("transport error")
                    || err_str.contains("certificate")
                    || err_str.contains("handshake")
                    || err_str.contains("dns")
                    || err_str.contains("InvalidDnsName"),
                "unexpected error message: {err_str}"
            );
        }
        Ok(mut client) => {
            // In case lazy handshake happened, RPC must fail
            let (_tx, rx) = tokio::sync::mpsc::channel::<AgentMessage>(1);
            let rpc_res = client.connect(ReceiverStream::new(rx)).await;
            assert!(
                rpc_res.is_err(),
                "RPC should fail when server name mismatches"
            );
        }
    }

    server.shutdown();
}

#[tokio::test]
async fn test_mtls_connect_fails_on_untrusted_ca() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki generation failed");

    // Server presents cert signed by rogue CA
    let (rogue_cert, rogue_key) = pki
        .generate_server_cert_for_name(server_name)
        .expect("rogue cert");
    let server = MockControlPlane::start(&rogue_cert, &rogue_key, &pki.ca_cert_pem)
        .await
        .expect("server start failed");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    pki.write_client_tls_dir(temp_dir.path())
        .expect("write certs");
    let tls_files = TlsFiles::load_from_dir(temp_dir.path()).expect("load certs");

    let addr_str = format!("https://{}", server.addr);
    let connect_res = connect_channel(&addr_str, server_name, &tls_files).await;
    match connect_res {
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                err_str.contains("transport error")
                    || err_str.contains("certificate")
                    || err_str.contains("handshake")
                    || err_str.contains("UnknownIssuer"),
                "unexpected error message: {err_str}"
            );
        }
        Ok(mut client) => {
            let (_tx, rx) = tokio::sync::mpsc::channel::<AgentMessage>(1);
            let rpc_res = client.connect(ReceiverStream::new(rx)).await;
            assert!(rpc_res.is_err(), "RPC should fail when CA is untrusted");
        }
    }

    server.shutdown();
}

#[tokio::test]
async fn test_mtls_connect_fails_on_untrusted_client() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki generation failed");

    let server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start failed");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    pki.write_client_tls_dir(temp_dir.path())
        .expect("write certs");

    // Replace client cert/key with untrusted ones signed by an unknown rogue CA
    let (rogue_client_cert, rogue_client_key) =
        TestPki::generate_untrusted_client().expect("rogue client");
    std::fs::write(temp_dir.path().join("tls.crt"), rogue_client_cert).expect("write rogue cert");
    std::fs::write(temp_dir.path().join("tls.key"), rogue_client_key).expect("write rogue key");

    let tls_files = TlsFiles::load_from_dir(temp_dir.path()).expect("load certs");

    let addr_str = format!("https://{}", server.addr);
    let connect_res = connect_channel(&addr_str, server_name, &tls_files).await;
    match connect_res {
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                err_str.contains("transport error")
                    || err_str.contains("certificate")
                    || err_str.contains("handshake")
                    || err_str.contains("UnknownIssuer"),
                "unexpected error message: {err_str}"
            );
        }
        Ok(mut client) => {
            let (_tx, rx) = tokio::sync::mpsc::channel::<AgentMessage>(1);
            let rpc_res = client.connect(ReceiverStream::new(rx)).await;
            assert!(
                rpc_res.is_err(),
                "RPC should fail when client certificate is untrusted"
            );
        }
    }

    server.shutdown();
}

#[tokio::test]
async fn test_outbox_seq_assignment_and_disk_recovery() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let runtime_dir = temp_dir.path().join("runtime");

    // 1. Initial run: append 3 messages
    {
        let mut outbox =
            crate::outbox::Outbox::open(&runtime_dir, "run-100", 1024 * 1024, 64 * 1024)
                .expect("open outbox");

        let seq1 = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                        detail: "Starting".to_string(),
                    },
                )),
            })
            .expect("append 1");
        assert_eq!(seq1, 1);

        let seq2 = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Running as i32,
                        detail: "Work in progress".to_string(),
                    },
                )),
            })
            .expect("append 2");
        assert_eq!(seq2, 2);

        let seq3 = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Finalizing as i32,
                        detail: "Wrapping up".to_string(),
                    },
                )),
            })
            .expect("append 3");
        assert_eq!(seq3, 3);
    } // Outbox dropped (simulating process restart)

    // 2. Restart recovery: re-open same outbox directory
    {
        let mut outbox =
            crate::outbox::Outbox::open(&runtime_dir, "run-100", 1024 * 1024, 64 * 1024)
                .expect("reopen outbox");
        assert_eq!(outbox.run_state().run_id, "run-100");
        assert_eq!(outbox.run_state().next_seq, 4);

        // Read unacknowledged after seq 1 (should return seq 2 and 3)
        let unacked = outbox.read_unacknowledged(1).expect("read unacked");
        assert_eq!(unacked.len(), 2);
        assert_eq!(unacked[0].seq, 2);
        assert_eq!(unacked[1].seq, 3);

        // Append after restart: next seq is 4
        let seq4 = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Finalizing as i32,
                        detail: "Done".to_string(),
                    },
                )),
            })
            .expect("append 4");
        assert_eq!(seq4, 4);
    }
}

#[tokio::test]
async fn test_outbox_ack_trimming_deletes_segments() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let runtime_dir = temp_dir.path().join("runtime");

    // Use tiny segment size (15 bytes) so each message creates a new segment file
    let mut outbox =
        crate::outbox::Outbox::open(&runtime_dir, "run-101", 1024 * 1024, 15).expect("open outbox");

    for i in 1..=5 {
        let seq = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Running as i32,
                        detail: format!("Step {i}"),
                    },
                )),
            })
            .expect("append");
        assert_eq!(seq, i);
    }

    // Verify multiple segment files exist
    let outbox_dir = runtime_dir.join("outbox");
    let seg_count = std::fs::read_dir(&outbox_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
        .count();
    assert!(
        seg_count >= 4,
        "expected at least 4 segments, got {seg_count}"
    );

    // Acknowledge up to seq 3
    outbox.acknowledge(3).expect("ack 3");

    // Segments with max seq <= 3 should be deleted
    let remaining_unacked = outbox.read_unacknowledged(0).expect("read unacked");
    assert_eq!(remaining_unacked.len(), 2);
    assert_eq!(remaining_unacked[0].seq, 4);
    assert_eq!(remaining_unacked[1].seq, 5);

    let remaining_seg_count = std::fs::read_dir(&outbox_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
        .count();
    assert!(
        remaining_seg_count < seg_count,
        "segment files should have been trimmed: before={seg_count}, after={remaining_seg_count}"
    );
}

#[tokio::test]
async fn test_outbox_bounded_backpressure() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let runtime_dir = temp_dir.path().join("runtime");

    // Bound outbox to 120 bytes, with segment size 40 bytes
    let mut outbox =
        crate::outbox::Outbox::open(&runtime_dir, "run-102", 120, 40).expect("open outbox");

    // Append 3 messages (~35 bytes each) until outbox is nearly full (3 * 35 = 105 bytes)
    outbox
        .append(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                    detail: "M1".repeat(15),
                },
            )),
        })
        .expect("append 1");

    outbox
        .append(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                    detail: "M2".repeat(15),
                },
            )),
        })
        .expect("append 2");

    outbox
        .append(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                    detail: "M3".repeat(15),
                },
            )),
        })
        .expect("append 3");

    // Outbox size should now be near or exceeding remaining space for a new message
    let outbox_arc = std::sync::Arc::new(tokio::sync::Mutex::new(outbox));
    let outbox_clone = std::sync::Arc::clone(&outbox_arc);

    // Spawning a task to append Msg4 via append_with_backpressure, which will wait on backpressure without holding the outbox lock
    let append_handle = tokio::spawn(async move {
        crate::outbox::Outbox::append_with_backpressure(
            &outbox_clone,
            AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Running as i32,
                        detail: "M4".repeat(15),
                    },
                )),
            },
        )
        .await
    });

    // Give it a moment to block
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Acknowledge msg 1 and 2 to free space
    {
        let mut ob = outbox_arc.lock().await;
        ob.acknowledge(2).expect("ack 2");
    }

    // Now append should unblock and complete successfully
    let res = tokio::time::timeout(std::time::Duration::from_secs(2), append_handle)
        .await
        .expect("append should not timeout")
        .expect("join ok")
        .expect("append succeeded");

    assert_eq!(res, 4);
}

#[tokio::test]
async fn test_outbox_discard_on_new_run() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let runtime_dir = temp_dir.path().join("runtime");

    // Run 1: append messages
    {
        let mut outbox =
            crate::outbox::Outbox::open(&runtime_dir, "run-old", 1024 * 1024, 64 * 1024)
                .expect("open outbox");
        outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                        detail: "Old run message".to_string(),
                    },
                )),
            })
            .expect("append");
        assert_eq!(outbox.read_unacknowledged(0).unwrap().len(), 1);
    }

    // Run 2: open with different run id
    {
        let mut outbox =
            crate::outbox::Outbox::open(&runtime_dir, "run-new", 1024 * 1024, 64 * 1024)
                .expect("open outbox with new run_id");
        assert_eq!(outbox.run_state().run_id, "run-new");
        assert_eq!(outbox.run_state().next_seq, 1);
        assert_eq!(outbox.read_unacknowledged(0).unwrap().len(), 0);

        // Appending to new run gets seq 1
        let seq = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                        detail: "New run message".to_string(),
                    },
                )),
            })
            .expect("append in new run");
        assert_eq!(seq, 1);
    }
}

#[tokio::test]
async fn test_scenario_control_plane_restart_and_outbox_replay() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    // Pre-populate outbox with 20 messages
    let outbox = {
        let mut ob =
            crate::outbox::Outbox::open(&runtime_dir, "run-restart", 1024 * 1024, 64 * 1024)
                .expect("open outbox");
        for i in 1..=20 {
            ob.append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Running as i32,
                        detail: format!("Task step {i}"),
                    },
                )),
            })
            .expect("append");
        }
        std::sync::Arc::new(tokio::sync::Mutex::new(ob))
    };

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir,
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(config, outbox, reconnect_policy);
    let handle = cm.spawn();

    // 1. Control plane receives Hello
    let hello = server.recv_agent_message().await.expect("recv hello");
    assert_eq!(hello.seq, 0);

    // 2. Control plane responds with HelloAck where last_persisted_seq = 15
    server
        .send_control_message(ControlMessage {
            ack: 15,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-restart".to_string(),
                last_persisted_seq: 15,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    // 3. Control plane receives replayed messages 16 through 20
    for expected_seq in 16..=20 {
        let replayed = server
            .recv_agent_message()
            .await
            .expect("recv replayed message");
        assert_eq!(replayed.seq, expected_seq);
    }

    // 4. Runtime sends message 21
    handle
        .send(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Finalizing as i32,
                    detail: "Continuing with 21".to_string(),
                },
            )),
        })
        .await
        .expect("send 21");

    let msg21 = server.recv_agent_message().await.expect("recv 21");
    assert_eq!(msg21.seq, 21);

    handle.shutdown();
    server.shutdown();
}

#[tokio::test]
async fn test_scenario_leftover_from_crashed_run_emits_notice() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    // Outbox holds messages from a crashed run
    let outbox = {
        let mut ob =
            crate::outbox::Outbox::open(&runtime_dir, "run-crashed", 1024 * 1024, 64 * 1024)
                .expect("open outbox");
        ob.append(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Running as i32,
                    detail: "Crashed run left behind".to_string(),
                },
            )),
        })
        .expect("append");
        std::sync::Arc::new(tokio::sync::Mutex::new(ob))
    };

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir,
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(
        config,
        std::sync::Arc::clone(&outbox),
        reconnect_policy,
    );
    let handle = cm.spawn();

    // 1. Control plane receives Hello with previous_run_id = "run-crashed"
    let hello = server.recv_agent_message().await.expect("recv hello");
    match hello.body {
        Some(agent_message::Body::Hello(h)) => {
            assert_eq!(h.previous_run_id, "run-crashed");
        }
        other => panic!("expected Hello, got {other:?}"),
    }

    // 2. Control plane acknowledges with a new run_id
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-fresh-assignment".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    // 3. Runtime detects mismatch, discards old outbox, and emits Notice with code "runtime.outbox_discarded"
    let notice_msg = server
        .recv_agent_message()
        .await
        .expect("recv notice message");
    assert_eq!(notice_msg.seq, 1);
    match notice_msg.body {
        Some(agent_message::Body::TranscriptEvent(ev)) => match ev.kind {
            Some(df_proto::transcript::v1::event::Kind::Notice(n)) => {
                assert_eq!(n.code, "runtime.outbox_discarded");
                assert_eq!(
                    n.level,
                    df_proto::transcript::v1::NoticeLevel::Warning as i32
                );
            }
            other => panic!("expected Notice, got {other:?}"),
        },
        other => panic!("expected TranscriptEvent, got {other:?}"),
    }

    // Verify outbox state reflects the new run_id
    assert_eq!(
        outbox.lock().await.run_state().run_id,
        "run-fresh-assignment"
    );

    handle.shutdown();
    server.shutdown();
}

#[tokio::test]
async fn test_scenario_certificate_rotated() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::outbox::Outbox::open(&runtime_dir, "run-cert", 1024 * 1024, 64 * 1024)
            .expect("open outbox"),
    ));

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir: tls_dir.clone(),
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(config, outbox, reconnect_policy);
    let handle = cm.spawn();

    // 1. First connection
    let _ = server.recv_agent_message().await.expect("recv first hello");
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-cert".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    assert_eq!(
        server
            .connection_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );

    // 2. Rotate certificate files on disk (simulating cert-manager csi-driver replacing files)
    let (rotated_cert, rotated_key) = pki.generate_rotated_client_cert().expect("rotated cert");
    std::fs::write(tls_dir.join("tls.crt"), rotated_cert).expect("write rotated cert");
    std::fs::write(tls_dir.join("tls.key"), rotated_key).expect("write rotated key");

    // 3. Drop active stream
    server.drop_active_stream().await;

    // 4. ConnectionManager reconnects with the new certificate files
    let hello2 = server
        .recv_agent_message()
        .await
        .expect("recv second hello after reconnect");
    assert_eq!(hello2.seq, 0);

    assert_eq!(
        server
            .connection_count
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );

    handle.shutdown();
    server.shutdown();
}

#[tokio::test]
async fn test_scenario_short_outage_events_queue_in_outbox() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::outbox::Outbox::open(&runtime_dir, "run-outage", 1024 * 1024, 64 * 1024)
            .expect("open outbox"),
    ));

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir: tls_dir.clone(),
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(
        config,
        std::sync::Arc::clone(&outbox),
        reconnect_policy,
    );
    let handle = cm.spawn();

    // Initial connection
    let _ = server.recv_agent_message().await.expect("recv hello");
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-outage".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    // Server drops stream, causing an outage
    server.drop_active_stream().await;

    // Give connection loop a moment to observe the stream drop
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // During the outage, the runtime queues messages
    handle
        .send(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Running as i32,
                    detail: "Queued during outage 1".to_string(),
                },
            )),
        })
        .await
        .expect("queue 1");

    handle
        .send(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Running as i32,
                    detail: "Queued during outage 2".to_string(),
                },
            )),
        })
        .await
        .expect("queue 2");

    // Reconnect happens, Hello is received (skipping any in-flight message before the drop)
    loop {
        let msg = server.recv_agent_message().await.expect("recv message");
        if matches!(msg.body, Some(agent_message::Body::Hello(_))) {
            break;
        }
    }

    // Server acknowledges with last_persisted_seq = 0
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-outage".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack after outage");

    // Both queued events are received in order
    let q1 = server.recv_agent_message().await.expect("recv q1");
    assert_eq!(q1.seq, 1);
    let q2 = server.recv_agent_message().await.expect("recv q2");
    assert_eq!(q2.seq, 2);

    handle.shutdown();
    server.shutdown();
}

#[tokio::test]
async fn test_scenario_prompt_resent_after_reconnect_delivered_once() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::outbox::Outbox::open(&runtime_dir, "run-idemp", 1024 * 1024, 64 * 1024)
            .expect("open outbox"),
    ));

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir: tls_dir.clone(),
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(config, outbox, reconnect_policy);
    let mut handle = cm.spawn();

    // 1. Initial connection
    let _ = server.recv_agent_message().await.expect("recv hello");
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-idemp".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    // 2. Control plane sends prompt-1
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::Prompt(df_proto::agent::v1::Prompt {
                prompt_id: "prompt-1".to_string(),
                author: None,
                content: vec![df_proto::transcript::v1::ContentBlock {
                    kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                        df_proto::transcript::v1::TextContent {
                            text: "Do something".to_string(),
                        },
                    )),
                }],
                queued_at: None,
            })),
        })
        .await
        .expect("send prompt-1");

    // 3. Runtime receives prompt-1
    let received_prompt = handle.recv().await.expect("handle received prompt-1");
    match received_prompt.body {
        Some(control_message::Body::Prompt(p)) => assert_eq!(p.prompt_id, "prompt-1"),
        other => panic!("expected Prompt, got {other:?}"),
    }

    // 4. Stream drops
    server.drop_active_stream().await;

    // 5. Reconnect happens, Hello is received
    let _ = server
        .recv_agent_message()
        .await
        .expect("recv hello after reconnect");

    // Server re-sends HelloAck
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-idemp".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack after reconnect");

    // 6. Control plane re-sends prompt-1
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::Prompt(df_proto::agent::v1::Prompt {
                prompt_id: "prompt-1".to_string(),
                author: None,
                content: vec![df_proto::transcript::v1::ContentBlock {
                    kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                        df_proto::transcript::v1::TextContent {
                            text: "Do something".to_string(),
                        },
                    )),
                }],
                queued_at: None,
            })),
        })
        .await
        .expect("send duplicate prompt-1");

    // 7. Control plane sends new prompt-2
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::Prompt(df_proto::agent::v1::Prompt {
                prompt_id: "prompt-2".to_string(),
                author: None,
                content: vec![df_proto::transcript::v1::ContentBlock {
                    kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                        df_proto::transcript::v1::TextContent {
                            text: "Do next thing".to_string(),
                        },
                    )),
                }],
                queued_at: None,
            })),
        })
        .await
        .expect("send prompt-2");

    // 8. Runtime MUST receive prompt-2 directly, ignoring the duplicate prompt-1!
    let next_prompt = tokio::time::timeout(std::time::Duration::from_secs(2), handle.recv())
        .await
        .expect("did not timeout waiting for prompt")
        .expect("received message");

    match next_prompt.body {
        Some(control_message::Body::Prompt(p)) => {
            assert_eq!(
                p.prompt_id, "prompt-2",
                "duplicate prompt-1 should have been filtered"
            );
        }
        other => panic!("expected prompt-2, got {other:?}"),
    }

    handle.shutdown();
    server.shutdown();
}

#[tokio::test]
async fn test_scenario_pending_tool_call_resent_after_reconnect() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::outbox::Outbox::open(&runtime_dir, "run-tool", 1024 * 1024, 64 * 1024)
            .expect("open outbox"),
    ));

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir: tls_dir.clone(),
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(config, outbox, reconnect_policy);
    let mut handle = cm.spawn();

    // 1. Initial connection
    let _ = server.recv_agent_message().await.expect("recv hello");
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-tool".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    // 2. Runtime sends ToolCallRequest
    handle
        .send(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::ToolCallRequest(
                df_proto::agent::v1::ToolCallRequest {
                    call_id: "call-xyz".to_string(),
                    tool_name: "test_tool".to_string(),
                    arguments_json: "{}".to_string(),
                },
            )),
        })
        .await
        .expect("send tool call");

    let received_call = server
        .recv_agent_message()
        .await
        .expect("server recv tool call");
    assert_eq!(received_call.seq, 1);

    // 3. Control plane acknowledges persistence (ack = 1), but drops before sending ToolCallResult!
    server
        .send_control_message(ControlMessage { ack: 1, body: None })
        .await
        .expect("send ack");

    // Drop active stream
    server.drop_active_stream().await;

    // 4. Reconnect happens, Hello is received
    let _ = server
        .recv_agent_message()
        .await
        .expect("recv hello after drop");

    // Control plane acknowledges Hello with last_persisted_seq = 1
    server
        .send_control_message(ControlMessage {
            ack: 1,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-tool".to_string(),
                last_persisted_seq: 1,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack after reconnect");

    // 5. Runtime MUST re-send the pending ToolCallRequest with the exact same call_id!
    let resent_call = server
        .recv_agent_message()
        .await
        .expect("server received re-sent tool call");
    match resent_call.body {
        Some(agent_message::Body::ToolCallRequest(req)) => {
            assert_eq!(req.call_id, "call-xyz");
            assert_eq!(req.tool_name, "test_tool");
        }
        other => panic!("expected ToolCallRequest, got {other:?}"),
    }

    // 6. Control plane responds with ToolCallResult
    server
        .send_control_message(ControlMessage {
            ack: resent_call.seq,
            body: Some(control_message::Body::ToolCallResult(
                df_proto::agent::v1::ToolCallResult {
                    call_id: "call-xyz".to_string(),
                    is_error: false,
                    content: vec![],
                    structured_content_json: "{\"status\":\"ok\"}".to_string(),
                    end_run_after_turn: false,
                },
            )),
        })
        .await
        .expect("send tool result");

    // 7. Runtime receives result
    let res = handle.recv().await.expect("recv tool call result");
    match res.body {
        Some(control_message::Body::ToolCallResult(r)) => {
            assert_eq!(r.call_id, "call-xyz");
            assert_eq!(r.structured_content_json, "{\"status\":\"ok\"}");
        }
        other => panic!("expected ToolCallResult, got {other:?}"),
    }

    handle.shutdown();
    server.shutdown();
}

#[test]
fn test_git_config_denylist() {
    use crate::git::is_config_entry_denied;

    // Denied execution / programs
    assert!(is_config_entry_denied("core.hooksPath", "/tmp/hooks"));
    assert!(is_config_entry_denied("core.fsmonitor", "true"));
    assert!(is_config_entry_denied("core.sshCommand", "ssh -i id_rsa"));
    assert!(is_config_entry_denied("core.gitProxy", "proxy"));
    assert!(is_config_entry_denied("core.askPass", "pass"));
    assert!(is_config_entry_denied("core.editor", "vim"));
    assert!(is_config_entry_denied("core.pager", "less"));
    assert!(is_config_entry_denied("sequence.editor", "code"));
    assert!(is_config_entry_denied("diff.external", "ext-diff"));
    assert!(is_config_entry_denied("diff.rust.textconv", "cat"));
    assert!(is_config_entry_denied("diff.custom.command", "custom-diff"));
    assert!(is_config_entry_denied(
        "merge.custom.driver",
        "custom-merge"
    ));
    assert!(is_config_entry_denied("pager.log", "cat"));
    assert!(is_config_entry_denied("filter.clean.clean", "clean-cmd"));
    assert!(is_config_entry_denied("gpg.program", "gpg"));
    assert!(is_config_entry_denied("alias.evil", "!rm -rf /"));
    assert!(!is_config_entry_denied("alias.st", "status"));

    // Denied traffic / credentials
    assert!(is_config_entry_denied(
        "remote.origin.url",
        "https://evil.org"
    ));
    assert!(is_config_entry_denied(
        "url.https://evil.insteadOf",
        "https://good"
    ));
    assert!(is_config_entry_denied("branch.main.remote", "origin"));
    assert!(is_config_entry_denied("branch.main.pushRemote", "origin"));
    assert!(is_config_entry_denied("credential.helper", "store"));
    assert!(is_config_entry_denied("http.proxy", "http://evil"));
    assert!(is_config_entry_denied("protocol.allow", "always"));

    // Denied loading / runtime owned
    assert!(is_config_entry_denied("include.path", "/etc/passwd"));
    assert!(is_config_entry_denied("includeIf.gitdir:foo.path", "/bar"));
    assert!(is_config_entry_denied("user.name", "Attacker"));
    assert!(is_config_entry_denied("user.email", "attacker@evil.org"));
    assert!(is_config_entry_denied("submodule.recurse", "true"));
    assert!(is_config_entry_denied("lfs.url", "https://evil"));
    assert!(is_config_entry_denied("core.worktree", "/root"));
    assert!(is_config_entry_denied("core.bare", "false"));
    assert!(is_config_entry_denied("extensions.preciousObjects", "true"));
    assert!(is_config_entry_denied("commit.gpgSign", "true"));
    assert!(is_config_entry_denied("tag.gpgSign", "true"));

    // Allowed entries
    assert!(!is_config_entry_denied("rebase.autoStash", "true"));
    assert!(!is_config_entry_denied("diff.algorithm", "histogram"));
    assert!(!is_config_entry_denied("pull.rebase", "merges"));
    assert!(!is_config_entry_denied("merge.conflictStyle", "zdiff3"));
    assert!(!is_config_entry_denied("rerere.enabled", "true"));
}

#[tokio::test]
async fn test_planted_configuration_scenario() {
    use crate::git::{GitConfigParams, rewrite_git_config};
    use df_proto::agent::v1::GitConfigEntry;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_dir = temp_dir.path().join("repo");
    let git_dir = repo_dir.join(".git");
    std::fs::create_dir_all(&git_dir).expect("create .git");

    // Plant malicious configuration into .git/config
    let planted_config = r#"
[core]
	repositoryformatversion = 0
	filemode = true
	bare = false
	fsmonitor = true
[remote "origin"]
	url = https://malicious.example.org/planted.git
	fetch = +refs/heads/*:refs/remotes/origin/*
[url "https://malicious.org"]
	insteadOf = https://example.com
[include]
	path = /tmp/evil.inc
[credential]
	helper = store
"#;
    std::fs::write(git_dir.join("config"), planted_config).expect("write planted config");

    // Plant .git/config.worktree
    std::fs::write(git_dir.join("config.worktree"), "[core]\nworktree = /tmp\n")
        .expect("write worktree config");
    assert!(git_dir.join("config.worktree").exists());

    let hooks_path = std::path::Path::new("/opt/factory/hooks-empty");
    let params = GitConfigParams {
        repo_url: "https://git.example.org/peter/dark-factory.git",
        commit_author_name: "Agent Bot",
        commit_author_email: "bot@darkfactory.local",
        hooks_path,
        credential_helper: "/opt/factory/factory-agent git-credential",
        extra_config: &[
            GitConfigEntry {
                key: "rebase.autoStash".to_string(),
                value: "true".to_string(),
            },
            // Planted entry in extra_config should also be stripped by denylist
            GitConfigEntry {
                key: "core.sshCommand".to_string(),
                value: "malicious-ssh".to_string(),
            },
        ],
    };

    rewrite_git_config(&repo_dir, &params).expect("rewrite config");

    // 1. .git/config.worktree MUST be deleted
    assert!(!git_dir.join("config.worktree").exists());

    // 2. Read new .git/config
    let new_content = std::fs::read_to_string(git_dir.join("config")).expect("read config");

    // Must contain the expected repo_url from Assign
    assert!(new_content.contains("url = \"https://git.example.org/peter/dark-factory.git\""));
    // Must contain the expected author identity
    assert!(new_content.contains("name = \"Agent Bot\""));
    assert!(new_content.contains("email = \"bot@darkfactory.local\""));
    // Must contain runtime credential helper
    assert!(new_content.contains("helper = \"/opt/factory/factory-agent git-credential\""));
    // Must contain hooksPath
    assert!(new_content.contains("hooksPath = /opt/factory/hooks-empty"));
    // Must contain allowed extra config
    assert!(new_content.contains("autoStash = \"true\""));

    // MUST NOT contain any planted or denylisted entries
    assert!(!new_content.contains("fsmonitor"));
    assert!(!new_content.contains("insteadOf"));
    assert!(!new_content.contains("include"));
    assert!(!new_content.contains("store"));
    assert!(!new_content.contains("malicious"));
    assert!(!new_content.contains("sshCommand"));
}

#[tokio::test]
async fn test_malicious_hook_scenario() {
    use crate::git::Git;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("local");
    let empty_hooks_dir = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks_dir).expect("create empty hooks dir");

    // 1. Initialize bare remote
    let out = tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await
        .expect("git init bare");
    assert!(out.status.success());

    // 2. Clone locally using standard git
    let out = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            local_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .expect("git clone");
    assert!(out.status.success());

    // 3. Make an initial commit
    let file_path = local_dir.join("README.md");
    std::fs::write(&file_path, "test").expect("write file");
    let _ = tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["config", "user.name", "Test User"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["config", "user.email", "test@example.com"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["add", "README.md"])
        .output()
        .await;
    let out = tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["commit", "-m", "init"])
        .output()
        .await
        .expect("git commit");
    assert!(out.status.success());

    // 4. Plant an executable malicious hook at .git/hooks/pre-push that fails with code 99
    let hooks_dir = local_dir.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).expect("create hooks dir");
    let pre_push_path = hooks_dir.join("pre-push");
    let hook_content = "#!/bin/sh\nexit 99\n";
    std::fs::write(&pre_push_path, hook_content).expect("write hook");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&pre_push_path)
            .expect("meta")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&pre_push_path, perms).expect("set permissions");
    }

    // 5. Normal git push would fail with status 99 because of the pre-push hook!
    let normal_push = tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await
        .expect("normal push");
    assert!(
        !normal_push.status.success(),
        "normal push must fail because hook exits 99"
    );

    // 6. Hardened Git with empty hooks directory MUST ignore the malicious hook and succeed!
    let hardened_git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks_dir);

    let hardened_push = hardened_git
        .push("origin", "HEAD:main", false)
        .await
        .expect("hardened push");
    assert!(
        hardened_push.status.success(),
        "hardened push must succeed, ignoring malicious hook. Stderr: {}",
        String::from_utf8_lossy(&hardened_push.stderr)
    );
}

#[tokio::test]
async fn test_non_https_remote_scenario() {
    use crate::git::Git;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_dir = temp_dir.path().join("repo");
    std::fs::create_dir_all(&repo_dir).expect("create repo dir");

    // Construct Git with production defaults (only https allowed)
    let git = Git::new(&repo_dir);

    // 1. Attempt file:// protocol
    let file_result = git.git_output(&["ls-remote", "file:///dev/null"]).await;
    assert!(file_result.is_err(), "file:// protocol should be refused");
    let err_str = file_result.unwrap_err().to_string();
    assert!(
        err_str.contains("transport 'file' not allowed") || err_str.contains("not allowed"),
        "expected transport 'file' not allowed error, got: {err_str}"
    );

    // 2. Attempt ssh:// protocol
    let ssh_result = git
        .git_output(&["ls-remote", "ssh://git@example.com/repo.git"])
        .await;
    assert!(ssh_result.is_err(), "ssh:// protocol should be refused");
    let err_str = ssh_result.unwrap_err().to_string();
    assert!(
        err_str.contains("transport 'ssh' not allowed") || err_str.contains("not allowed"),
        "expected transport 'ssh' not allowed error, got: {err_str}"
    );
}

#[tokio::test]
async fn test_runtime_socket_permissions_mode_0600() {
    use crate::runtime_socket::{RuntimeHandler, RuntimeSocketServer};

    struct DummyHandler;
    #[tonic::async_trait]
    impl RuntimeHandler for DummyHandler {
        async fn handle_git_credential(
            &self,
            _p: &str,
            _h: &str,
            _path: &str,
        ) -> (Option<String>, Option<String>, Option<String>) {
            (None, None, None)
        }
    }

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let socket_path = temp_dir.path().join("runtime.sock");

    let server = RuntimeSocketServer::bind(&socket_path, std::sync::Arc::new(DummyHandler))
        .expect("bind server");
    let _handle = server.spawn();

    assert!(socket_path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&socket_path)
            .expect("metadata")
            .permissions();
        assert_eq!(
            perms.mode() & 0o777,
            0o600,
            "runtime socket must have mode 0600"
        );
    }
}

#[tokio::test]
async fn test_git_credential_client_get_store_erase() {
    use crate::runtime_socket::{RuntimeHandler, RuntimeSocketServer, run_git_credential_stream};

    struct TestHandler {
        deny: bool,
    }

    #[tonic::async_trait]
    impl RuntimeHandler for TestHandler {
        async fn handle_git_credential(
            &self,
            protocol: &str,
            host: &str,
            path: &str,
        ) -> (Option<String>, Option<String>, Option<String>) {
            assert_eq!(protocol, "https");
            assert_eq!(host, "git.example.org");
            assert_eq!(path, "peter/dark-factory.git");
            if self.deny {
                (None, None, Some("access denied".to_string()))
            } else {
                (
                    Some("bot".to_string()),
                    Some("secret-token-xyz".to_string()),
                    None,
                )
            }
        }
    }

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let socket_path = temp_dir.path().join("runtime.sock");

    let server = RuntimeSocketServer::bind(
        &socket_path,
        std::sync::Arc::new(TestHandler { deny: false }),
    )
    .expect("bind server");
    let _handle = server.spawn();

    // 1. Test "get" with valid credentials
    let input = b"protocol=https\nhost=git.example.org\npath=peter/dark-factory.git\n\n";
    let mut output = Vec::new();
    run_git_credential_stream(&input[..], &mut output, &socket_path, Some("get"))
        .await
        .expect("run get");

    let out_str = String::from_utf8(output).expect("utf8 output");
    assert!(out_str.contains("username=bot\n"));
    assert!(out_str.contains("password=secret-token-xyz\n"));

    // 2. Test "store" is a no-op
    let store_input =
        b"protocol=https\nhost=git.example.org\nusername=bot\npassword=secret-token-xyz\n\n";
    let mut store_output = Vec::new();
    run_git_credential_stream(
        &store_input[..],
        &mut store_output,
        &socket_path,
        Some("store"),
    )
    .await
    .expect("run store");
    assert!(store_output.is_empty(), "store must be a no-op");

    // 3. Test "erase" is a no-op
    let erase_input = b"protocol=https\nhost=git.example.org\n\n";
    let mut erase_output = Vec::new();
    run_git_credential_stream(
        &erase_input[..],
        &mut erase_output,
        &socket_path,
        Some("erase"),
    )
    .await
    .expect("run erase");
    assert!(erase_output.is_empty(), "erase must be a no-op");

    // 4. Test "get" when request is denied
    let denied_socket = temp_dir.path().join("denied.sock");
    let denied_server = RuntimeSocketServer::bind(
        &denied_socket,
        std::sync::Arc::new(TestHandler { deny: true }),
    )
    .expect("bind denied server");
    let _denied_handle = denied_server.spawn();

    let mut denied_output = Vec::new();
    run_git_credential_stream(&input[..], &mut denied_output, &denied_socket, Some("get"))
        .await
        .expect("run denied get");
    assert!(
        denied_output.is_empty(),
        "denied credentials must output nothing"
    );
}

fn scan_directory_for_secret(dir: &std::path::Path, secret: &[u8]) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(scan_directory_for_secret(&path, secret));
            } else if path.is_file()
                && let Ok(content) = std::fs::read(&path)
                && content.windows(secret.len()).any(|w| w == secret)
            {
                found.push(path);
            }
        }
    }
    found
}

#[tokio::test]
async fn test_scenario_clone_from_local_https_test_server_using_credential_helper() {
    use crate::runtime_socket::{RuntimeHandler, RuntimeSocketServer};
    use crate::test_harness::{LocalHttpsGitServer, TestPki};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let git_root = temp_dir.path().join("git_root");
    let repo_dir = git_root.join("repo.git");
    let data_dir = temp_dir.path().join("data");
    let runtime_dir = temp_dir.path().join("runtime");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks dir");

    // 1. Initialize bare git repository in git_root
    std::fs::create_dir_all(&git_root).expect("create git_root");
    let out = tokio::process::Command::new("git")
        .args(["init", "--bare", "-b", "main", repo_dir.to_str().unwrap()])
        .output()
        .await
        .expect("git init bare");
    assert!(out.status.success());

    // Enable receivepack / uploadpack
    let _ = tokio::process::Command::new("git")
        .current_dir(&repo_dir)
        .args(["config", "http.receivepack", "true"])
        .output()
        .await;

    // Seed initial commit into bare repo
    let seed_dir = temp_dir.path().join("seed");
    let out = tokio::process::Command::new("git")
        .args([
            "clone",
            repo_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .expect("clone seed");
    assert!(out.status.success());

    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed User"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await;

    std::fs::write(seed_dir.join("hello.txt"), "Dark Factory Iteration 1").expect("write file");
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "hello.txt"])
        .output()
        .await;
    let out = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "initial commit"])
        .output()
        .await
        .expect("seed commit");
    assert!(out.status.success());
    let out = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await
        .expect("seed push");
    assert!(out.status.success());

    // 2. Start LocalHttpsGitServer with required Basic auth
    let pki = TestPki::generate("localhost").expect("pki");
    let expected_user = "bot".to_string();
    let expected_pass = "super-secret-token-42".to_string();
    let git_server = LocalHttpsGitServer::start(
        &pki,
        git_root.clone(),
        Some((expected_user.clone(), expected_pass.clone())),
    )
    .expect("start https git server");

    // Write CA certificate to file for Git to trust
    let ca_path = temp_dir.path().join("ca.crt");
    std::fs::write(&ca_path, &pki.ca_cert_pem).expect("write ca.crt");

    // 3. Start RuntimeSocketServer on runtime_dir/runtime.sock
    let socket_path = runtime_dir.join("runtime.sock");
    struct TestCredentialProvider {
        user: String,
        pass: String,
    }
    #[tonic::async_trait]
    impl RuntimeHandler for TestCredentialProvider {
        async fn handle_git_credential(
            &self,
            _p: &str,
            _h: &str,
            _path: &str,
        ) -> (Option<String>, Option<String>, Option<String>) {
            (Some(self.user.clone()), Some(self.pass.clone()), None)
        }
    }

    let socket_server = RuntimeSocketServer::bind(
        &socket_path,
        std::sync::Arc::new(TestCredentialProvider {
            user: expected_user,
            pass: expected_pass.clone(),
        }),
    )
    .expect("bind socket");
    let _socket_handle = socket_server.spawn();

    // 4. Create python credential helper that communicates with runtime socket
    let helper_path = temp_dir.path().join("git-credential-helper.py");
    let python_helper_code = r#"#!/usr/bin/env python3
import sys, socket, json, struct

operation = sys.argv[-1] if len(sys.argv) > 1 else "get"
if operation not in ("get",):
    sys.exit(0)

sock_path = sys.argv[1] if len(sys.argv) > 2 else os.environ.get("DF_RUNTIME_SOCK", "")

fields = {}
for line in sys.stdin:
    line = line.strip()
    if not line:
        break
    if "=" in line:
        k, v = line.split("=", 1)
        fields[k.strip()] = v.strip()

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sock_path)

req = {
    "type": "git_credential",
    "protocol": fields.get("protocol", ""),
    "host": fields.get("host", ""),
    "path": fields.get("path", "")
}
payload = json.dumps(req).encode("utf-8")
s.sendall(struct.pack(">I", len(payload)) + payload)

len_bytes = s.recv(4)
if len(len_bytes) == 4:
    resp_len = struct.unpack(">I", len_bytes)[0]
    data = b""
    while len(data) < resp_len:
        chunk = s.recv(resp_len - len(data))
        if not chunk:
            break
        data += chunk
    resp = json.loads(data.decode("utf-8"))
    username = resp.get("username")
    password = resp.get("password")
    if username and password:
        sys.stdout.write(f"username={username}\npassword={password}\n")
        sys.stdout.flush()
"#;
    std::fs::write(&helper_path, python_helper_code).expect("write helper script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&helper_path).expect("meta").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&helper_path, perms).expect("set permissions");
    }

    // 5. Clone repository using hardened Git with credential helper
    let target_clone_dir = data_dir.join("repo");
    let repo_url = format!("https://localhost:{}/repo.git", git_server.addr.port());
    let helper_cmd = format!("{} {}", helper_path.display(), socket_path.display());

    let clone_output = tokio::process::Command::new("git")
        .arg("-c")
        .arg(format!("core.hooksPath={}", empty_hooks.display()))
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("credential.helper=")
        .arg("-c")
        .arg(format!("credential.helper={helper_cmd}"))
        .arg("-c")
        .arg("protocol.allow=never")
        .arg("-c")
        .arg("protocol.https.allow=always")
        .arg("-c")
        .arg(format!("http.sslCAInfo={}", ca_path.display()))
        .arg("-c")
        .arg("submodule.recurse=false")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["clone", &repo_url, target_clone_dir.to_str().unwrap()])
        .output()
        .await
        .expect("git clone");

    assert!(
        clone_output.status.success(),
        "git clone with credential helper must succeed. Stderr: {}",
        String::from_utf8_lossy(&clone_output.stderr)
    );

    // Verify cloned content
    assert!(target_clone_dir.join("hello.txt").exists());
    let file_content = std::fs::read_to_string(target_clone_dir.join("hello.txt")).unwrap();
    assert_eq!(file_content, "Dark Factory Iteration 1");

    // 6. Verify: NO password lands under /data
    let leaked_files = scan_directory_for_secret(&data_dir, expected_pass.as_bytes());
    assert!(
        leaked_files.is_empty(),
        "password must never land under /data, but found in: {leaked_files:?}"
    );

    // 7. Verify scenario: Denied request fails git clone
    let denied_clone_dir = data_dir.join("denied_repo");
    let denied_socket_path = runtime_dir.join("denied_runtime.sock");
    struct DeniedProvider;
    #[tonic::async_trait]
    impl RuntimeHandler for DeniedProvider {
        async fn handle_git_credential(
            &self,
            _p: &str,
            _h: &str,
            _path: &str,
        ) -> (Option<String>, Option<String>, Option<String>) {
            (None, None, Some("access denied".to_string()))
        }
    }
    let denied_socket_server =
        RuntimeSocketServer::bind(&denied_socket_path, std::sync::Arc::new(DeniedProvider))
            .expect("bind denied socket");
    let _denied_handle = denied_socket_server.spawn();

    let denied_helper_cmd = format!("{} {}", helper_path.display(), denied_socket_path.display());
    let denied_output = tokio::process::Command::new("git")
        .arg("-c")
        .arg(format!("core.hooksPath={}", empty_hooks.display()))
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("credential.helper=")
        .arg("-c")
        .arg(format!("credential.helper={denied_helper_cmd}"))
        .arg("-c")
        .arg("protocol.allow=never")
        .arg("-c")
        .arg("protocol.https.allow=always")
        .arg("-c")
        .arg(format!("http.sslCAInfo={}", ca_path.display()))
        .arg("-c")
        .arg("submodule.recurse=false")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["clone", &repo_url, denied_clone_dir.to_str().unwrap()])
        .output()
        .await
        .expect("git clone denied");

    assert!(
        !denied_output.status.success(),
        "git clone must fail when credentials are denied"
    );

    git_server.shutdown();
}

#[tokio::test]
async fn test_scenario_prepare_first_run() {
    use crate::git::Git;
    use crate::workspace::prepare_workspace;
    use df_proto::agent::v1::Workspace;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("data").join("repo");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // 1. Initialize bare remote repository
    let out = tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await
        .expect("git init bare");
    assert!(out.status.success());

    // Make an initial commit on main
    let seed_dir = temp_dir.path().join("seed");
    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await;
    std::fs::write(seed_dir.join("init.txt"), "Initial").expect("write file");
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "init.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "init"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await;

    // 2. Prepare workspace on empty volume
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    let res = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
    .await
    .expect("prepare workspace");

    assert!(!res.diverged);
    assert!(res.push_report.is_none());

    // Branch must be factory/task/DF-1
    let branch = git
        .git_output(&["branch", "--show-current"])
        .await
        .expect("current branch");
    assert_eq!(branch, "factory/task/DF-1");

    // Must be at origin/main
    let local_head = git.current_head().await.expect("head");
    let remote_main = git
        .git_output(&["rev-parse", "origin/main"])
        .await
        .expect("origin/main");
    assert_eq!(local_head, remote_main);

    // Verified blobless clone config
    let config =
        std::fs::read_to_string(local_dir.join(".git").join("config")).expect("read config");
    assert!(config.contains("partialclonefilter = blob:none"));
}

#[tokio::test]
async fn test_scenario_prepare_volume_lost_restore_wip() {
    use crate::git::Git;
    use crate::workspace::prepare_workspace;
    use df_proto::agent::v1::Workspace;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("data").join("repo");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // 1. Initialize bare remote repository
    let _ = tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await;

    let seed_dir = temp_dir.path().join("seed");
    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await;

    // Create main commit
    std::fs::write(seed_dir.join("main.txt"), "Main content").expect("write file");
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "main commit"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await;

    // Create factory/task/DF-1 branch on remote
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/task/DF-1"])
        .output()
        .await;
    std::fs::write(seed_dir.join("task.txt"), "Task work").expect("write file");
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "task.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "task commit"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await;

    // Create factory/wip/DF-1 branch on remote
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/wip/DF-1"])
        .output()
        .await;
    std::fs::write(seed_dir.join("wip_uncommitted.txt"), "WIP draft").expect("write file");
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "wip_uncommitted.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "wip commit"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/wip/DF-1"])
        .output()
        .await;

    // 2. Volume lost after earlier runs: prepare on empty local_dir
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    let res = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
    .await
    .expect("prepare workspace");

    assert!(!res.diverged);

    // Task branch checked out
    let branch = git
        .git_output(&["branch", "--show-current"])
        .await
        .expect("branch");
    assert_eq!(branch, "factory/task/DF-1");

    // Committed task file is present
    assert!(local_dir.join("task.txt").exists());

    // WIP files restored into the working tree!
    assert!(
        local_dir.join("wip_uncommitted.txt").exists(),
        "WIP uncommitted file should be restored"
    );
    let content = std::fs::read_to_string(local_dir.join("wip_uncommitted.txt")).unwrap();
    assert_eq!(content, "WIP draft");
}

#[tokio::test]
async fn test_scenario_prepare_unpushed_commits_and_fast_forward() {
    use crate::git::Git;
    use crate::workspace::prepare_workspace;
    use df_proto::agent::v1::{PushStatus, PushTarget, Workspace};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("data").join("repo");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // 1. Initialize bare remote repository with main
    let _ = tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await;

    let seed_dir = temp_dir.path().join("seed");
    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await;

    std::fs::write(seed_dir.join("main.txt"), "Main").expect("write file");
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "main"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await;

    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    // First prepare (fresh clone)
    let _ = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
    .await
    .expect("initial prepare");

    // 2. Previous run made a commit locally on the task branch but was killed before pushing!
    let _ = git.run_cmd(&["config", "commit.gpgsign", "false"]).await;
    let _ = git.run_cmd(&["config", "user.name", "Agent Bot"]).await;
    let _ = git
        .run_cmd(&["config", "user.email", "bot@example.com"])
        .await;

    std::fs::write(local_dir.join("agent_work.txt"), "Done work").expect("write file");
    let _ = git.run_cmd(&["add", "agent_work.txt"]).await;
    let _ = git.run_cmd(&["commit", "-m", "agent committed"]).await;

    let unpushed_sha = git.current_head().await.expect("head");

    // 3. Second run's prepare on the existing workspace MUST push the unpushed commits!
    let res = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
    .await
    .expect("second prepare");

    assert!(!res.diverged);
    assert!(
        res.push_report.is_some(),
        "must emit PushReport for unpushed commits"
    );
    let report = res.push_report.unwrap();
    assert_eq!(report.target, PushTarget::TaskBranch as i32);
    assert_eq!(report.status, PushStatus::Ok as i32);
    assert_eq!(report.sha, unpushed_sha);
    assert_eq!(report.commits_ahead_of_base, 1);

    // Verify remote task branch now matches local SHA
    let remote_sha = git
        .git_output(&["rev-parse", "origin/factory/task/DF-1"])
        .await
        .expect("remote sha");
    assert_eq!(remote_sha, unpushed_sha);

    // 4. Remote moved ahead: seed_dir pushes another commit to factory/task/DF-1
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["fetch", "origin"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "factory/task/DF-1"])
        .output()
        .await;
    std::fs::write(seed_dir.join("remote_ahead.txt"), "From remote").expect("write file");
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "remote_ahead.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "remote ahead commit"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await;

    // Third prepare: local is behind remote, must fast-forward merge cleanly
    let res = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
    .await
    .expect("third prepare ff");

    assert!(!res.diverged);
    assert!(res.push_report.is_none());
    assert!(
        local_dir.join("remote_ahead.txt").exists(),
        "fast-forward merge must bring in remote file"
    );
}

#[tokio::test]
async fn test_scenario_prepare_diverged() {
    use crate::git::Git;
    use crate::workspace::prepare_workspace;
    use df_proto::agent::v1::Workspace;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("data").join("repo");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // 1. Initialize bare remote repository with main
    let _ = tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await;

    let seed_dir = temp_dir.path().join("seed");
    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await;

    std::fs::write(seed_dir.join("main.txt"), "Main").expect("write file");
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "main"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await;

    // Push initial task branch
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/task/DF-1"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await;

    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    // Prepare fresh workspace
    let _ = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
    .await
    .expect("prepare");

    // 2. Local creates a commit A
    let _ = git.run_cmd(&["config", "commit.gpgsign", "false"]).await;
    let _ = git.run_cmd(&["config", "user.name", "Agent Bot"]).await;
    let _ = git
        .run_cmd(&["config", "user.email", "bot@example.com"])
        .await;
    std::fs::write(local_dir.join("local.txt"), "Local only").expect("write local");
    let _ = git.run_cmd(&["add", "local.txt"]).await;
    let _ = git.run_cmd(&["commit", "-m", "local commit"]).await;
    let local_sha = git.current_head().await.expect("local head");

    // 3. Remote creates a conflicting commit B
    std::fs::write(seed_dir.join("remote.txt"), "Remote only").expect("write remote");
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "remote.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "remote commit"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await;

    // 4. Prepare on existing workspace MUST detect divergence:
    // Both have commits the other lacks!
    let res = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
    .await
    .expect("prepare diverged");

    assert!(res.diverged, "workspace must be marked diverged");
    assert!(
        res.push_report.is_none(),
        "no push should happen when diverged"
    );
    assert_eq!(res.notices.len(), 1);
    assert_eq!(res.notices[0].code, "workspace.diverged");

    // Neither branch is reset: local HEAD is still local_sha
    let head = git.current_head().await.expect("head");
    assert_eq!(
        head, local_sha,
        "local branch must not be reset on divergence"
    );
}

#[test]
fn test_is_secret_file_denylist() {
    use crate::workspace::is_secret_file;

    // Denied files
    assert!(is_secret_file(".env"));
    assert!(is_secret_file(".env.local"));
    assert!(is_secret_file(".env.production"));
    assert!(is_secret_file("config/.env"));
    assert!(is_secret_file("id_rsa"));
    assert!(is_secret_file("id_ed25519"));
    assert!(is_secret_file(".ssh/id_rsa"));
    assert!(is_secret_file("cert.pem"));
    assert!(is_secret_file("server.key"));
    assert!(is_secret_file("bundle.pfx"));
    assert!(is_secret_file("client.p12"));
    assert!(is_secret_file("secret.pkcs12"));

    // Allowed files
    assert!(!is_secret_file("src/main.rs"));
    assert!(!is_secret_file("README.md"));
    assert!(!is_secret_file("package.json"));
    assert!(!is_secret_file(".gitignore"));
    assert!(!is_secret_file("Cargo.toml"));
    assert!(!is_secret_file("environment.rs"));
    assert!(!is_secret_file("keyboard.js"));
}

#[tokio::test]
async fn test_scenario_push_after_turn_with_commits_and_rejection() {
    use crate::git::Git;
    use crate::workspace::push_after_turn;
    use df_proto::agent::v1::{PushStatus, PushTarget, Workspace};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("local");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // 1. Initialize bare remote repository
    let _ = tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await;

    // Make initial commit on main and factory/task/DF-1
    let seed_dir = temp_dir.path().join("seed");
    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await;

    std::fs::write(seed_dir.join("main.txt"), "Main").expect("write main");
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "main"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await;

    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/task/DF-1"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await;

    // Clone locally
    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            local_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);
    let _ = git.run_cmd(&["config", "commit.gpgsign", "false"]).await;
    let _ = git.run_cmd(&["config", "user.name", "Bot"]).await;
    let _ = git
        .run_cmd(&["config", "user.email", "bot@example.com"])
        .await;
    let _ = git.run_cmd(&["checkout", "factory/task/DF-1"]).await;

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    let mut last_pushed_sha = Some(git.current_head().await.unwrap());

    // 2. Turn 1 produces 2 new commits
    std::fs::write(local_dir.join("c1.txt"), "1").unwrap();
    let _ = git.run_cmd(&["add", "c1.txt"]).await;
    let _ = git.run_cmd(&["commit", "-m", "commit 1"]).await;

    std::fs::write(local_dir.join("c2.txt"), "2").unwrap();
    let _ = git.run_cmd(&["add", "c2.txt"]).await;
    let _ = git.run_cmd(&["commit", "-m", "commit 2"]).await;

    let res = push_after_turn(&git, &workspace, &mut last_pushed_sha)
        .await
        .expect("push after turn");

    assert!(res.is_some(), "turn with commits must emit PushReport");
    let report = res.unwrap();
    assert_eq!(report.target, PushTarget::TaskBranch as i32);
    assert_eq!(report.status, PushStatus::Ok as i32);
    assert_eq!(report.commits_ahead_of_base, 2);

    // Calling again without new commits returns None
    let res2 = push_after_turn(&git, &workspace, &mut last_pushed_sha)
        .await
        .expect("push after turn no commits");
    assert!(res2.is_none());

    // 3. Remote moves ahead non-fast-forward:
    // Seed repo pushes a commit
    std::fs::write(seed_dir.join("seed_change.txt"), "seed").unwrap();
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "seed_change.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "seed pushed"])
        .output()
        .await;
    let seed_push = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "--force", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await
        .expect("seed push");
    assert!(seed_push.status.success(), "seed force push must succeed");

    // Local makes another commit
    std::fs::write(local_dir.join("c3.txt"), "3").unwrap();
    let _ = git.run_cmd(&["add", "c3.txt"]).await;
    let _ = git.run_cmd(&["commit", "-m", "commit 3"]).await;

    // Push after turn must be rejected as non-fast-forward
    let res3 = push_after_turn(&git, &workspace, &mut last_pushed_sha)
        .await
        .expect("push after turn rejected");

    assert!(res3.is_some());
    let report3 = res3.unwrap();
    assert_eq!(
        report3.status,
        PushStatus::RejectedNonFastForward as i32,
        "push must be rejected as non-fast-forward"
    );
}

#[tokio::test]
async fn test_scenario_finalize_uncommitted_work_and_secret_exclusion() {
    use crate::git::Git;
    use crate::workspace::finalize_workspace;
    use df_proto::agent::v1::{RunOutcome, Workspace};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("local");
    let runtime_dir = temp_dir.path().join("runtime");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // Initialize bare remote repo with main and factory/task/DF-1
    let _ = tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await;

    let seed_dir = temp_dir.path().join("seed");
    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await;

    std::fs::write(seed_dir.join("main.txt"), "Initial main").unwrap();
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "init"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/task/DF-1"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await;

    // Clone into local_dir
    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            local_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);
    let _ = git.run_cmd(&["config", "commit.gpgsign", "false"]).await;
    let _ = git.run_cmd(&["config", "user.name", "Bot"]).await;
    let _ = git
        .run_cmd(&["config", "user.email", "bot@example.com"])
        .await;
    let _ = git.run_cmd(&["checkout", "factory/task/DF-1"]).await;

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    // 1. Create uncommitted modifications, an untracked normal file, and untracked secret files (.env, id_rsa)
    std::fs::write(local_dir.join("main.txt"), "Modified in turn").unwrap();
    std::fs::write(local_dir.join("feature.txt"), "New feature uncommitted").unwrap();
    std::fs::write(local_dir.join(".env"), "SECRET_API_KEY=xyz").unwrap();
    std::fs::write(local_dir.join("id_rsa"), "-----BEGIN RSA PRIVATE KEY-----").unwrap();

    let status_before = git.git_status().await.unwrap();

    let mut last_pushed_sha = Some(git.current_head().await.unwrap());

    // 2. Finalize workspace
    let finalize_params = crate::workspace::FinalizeParams {
        repo_dir: &local_dir,
        runtime_dir: &runtime_dir,
        workspace: &workspace,
        run_id: "run-101",
        outcome: RunOutcome::Completed,
        detail: "Task completed",
        harness_session_id: "session-1",
        hooks_path: &empty_hooks,
        credential_helper: "/opt/factory/factory-agent git-credential",
        wip_size_cap_bytes: 10 * 1024 * 1024,
        diverged: false,
    };
    let res = finalize_workspace(&git, &finalize_params, &mut last_pushed_sha)
        .await
        .expect("finalize");

    // RunReport workspace check
    let state = res.run_report.workspace.expect("workspace state");
    assert!(
        !state.wip_sha.is_empty(),
        "wip_sha must be present when worktree had changes"
    );

    // 3. Verify git status in workspace is completely UNCHANGED
    let status_after = git.git_status().await.unwrap();
    assert_eq!(
        status_before, status_after,
        "git status in workspace must not be modified by finalize"
    );

    // 4. Verify contents of the WIP commit tree
    let wip_files = git
        .git_output(&["ls-tree", "-r", "--name-only", &state.wip_sha])
        .await
        .unwrap();
    let file_list: Vec<&str> = wip_files.lines().map(str::trim).collect();

    assert!(file_list.contains(&"main.txt"));
    assert!(file_list.contains(&"feature.txt"));
    assert!(
        !file_list.contains(&".env"),
        ".env MUST be excluded from WIP snapshot"
    );
    assert!(
        !file_list.contains(&"id_rsa"),
        "id_rsa MUST be excluded from WIP snapshot"
    );
}

#[tokio::test]
async fn test_scenario_finalize_clean_worktree_deletes_remote_wip() {
    use crate::git::Git;
    use crate::workspace::finalize_workspace;
    use df_proto::agent::v1::{PushStatus, PushTarget, RunOutcome, Workspace};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("local");
    let runtime_dir = temp_dir.path().join("runtime");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // Bare remote repo
    let _ = tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await;

    let seed_dir = temp_dir.path().join("seed");
    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await;

    std::fs::write(seed_dir.join("main.txt"), "Main").unwrap();
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "init"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/task/DF-1"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await;

    // Push an existing factory/wip/DF-1 branch to remote
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/wip/DF-1"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/wip/DF-1"])
        .output()
        .await;

    // Clone into local_dir
    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            local_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);
    let _ = git.run_cmd(&["config", "commit.gpgsign", "false"]).await;
    let _ = git.run_cmd(&["config", "user.name", "Bot"]).await;
    let _ = git
        .run_cmd(&["config", "user.email", "bot@example.com"])
        .await;
    let _ = git.run_cmd(&["checkout", "factory/task/DF-1"]).await;

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    let mut last_pushed_sha = Some(git.current_head().await.unwrap());

    // Worktree is clean!
    assert!(git.is_clean().await.unwrap());

    let finalize_params = crate::workspace::FinalizeParams {
        repo_dir: &local_dir,
        runtime_dir: &runtime_dir,
        workspace: &workspace,
        run_id: "run-102",
        outcome: RunOutcome::Completed,
        detail: "Clean finish",
        harness_session_id: "session-2",
        hooks_path: &empty_hooks,
        credential_helper: "/opt/factory/factory-agent git-credential",
        wip_size_cap_bytes: 10 * 1024 * 1024,
        diverged: false,
    };
    let res = finalize_workspace(&git, &finalize_params, &mut last_pushed_sha)
        .await
        .expect("finalize clean");

    let state = res.run_report.workspace.expect("state");
    assert!(state.wip_sha.is_empty(), "clean worktree has no wip_sha");

    // Deleted report emitted
    let deleted_report = res.push_reports.iter().find(|r| {
        r.target == PushTarget::WipBranch as i32 && r.status == PushStatus::Deleted as i32
    });
    assert!(
        deleted_report.is_some(),
        "must emit PushReport with DELETED"
    );

    // Remote WIP branch should be gone
    let has_remote_wip = git
        .run_cmd(&["rev-parse", "--verify", "origin/factory/wip/DF-1"])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(
        !has_remote_wip,
        "remote WIP branch must be deleted on clean worktree"
    );
}

#[tokio::test]
async fn test_scenario_finalize_wip_size_cap_exceeded() {
    use crate::git::Git;
    use crate::workspace::finalize_workspace;
    use df_proto::agent::v1::{RunOutcome, Workspace};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("local");
    let runtime_dir = temp_dir.path().join("runtime");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    let _ = tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await;

    let seed_dir = temp_dir.path().join("seed");
    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await;

    std::fs::write(seed_dir.join("main.txt"), "Main").unwrap();
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "init"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/task/DF-1"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await;

    let _ = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            local_dir.to_str().unwrap(),
        ])
        .output()
        .await;
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);
    let _ = git.run_cmd(&["config", "commit.gpgsign", "false"]).await;
    let _ = git.run_cmd(&["config", "user.name", "Bot"]).await;
    let _ = git
        .run_cmd(&["config", "user.email", "bot@example.com"])
        .await;
    let _ = git.run_cmd(&["checkout", "factory/task/DF-1"]).await;

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    // Create a large uncommitted file exceeding size cap of 500 bytes
    let large_data = vec![b'x'; 5000];
    std::fs::write(local_dir.join("large_file.bin"), large_data).unwrap();

    let mut last_pushed_sha = Some(git.current_head().await.unwrap());

    let finalize_params = crate::workspace::FinalizeParams {
        repo_dir: &local_dir,
        runtime_dir: &runtime_dir,
        workspace: &workspace,
        run_id: "run-103",
        outcome: RunOutcome::Completed,
        detail: "Size cap test",
        harness_session_id: "session-3",
        hooks_path: &empty_hooks,
        credential_helper: "/opt/factory/factory-agent git-credential",
        wip_size_cap_bytes: 500, // 500 bytes cap!
        diverged: false,
    };
    let res = finalize_workspace(&git, &finalize_params, &mut last_pushed_sha)
        .await
        .expect("finalize size cap");

    let state = res.run_report.workspace.expect("state");
    assert!(
        state.wip_sha.is_empty(),
        "wip_sha should be empty when size cap is exceeded"
    );
    assert_eq!(res.notices.len(), 1);
    assert_eq!(res.notices[0].code, "wip.size_cap_exceeded");
}

#[tokio::test]
async fn test_scenario_token_stays_off_the_volume() {
    use crate::harness::prepare_harness_environment;
    use df_proto::agent::v1::{AcpHarness, Assign, Harness, HarnessCredentials, harness::Kind};
    use std::collections::HashMap;

    let temp = tempfile::tempdir().unwrap();
    let data_dir = temp.path().join("data");
    let home_dir = data_dir.join("home");
    let repo_dir = data_dir.join("repo");
    let runtime_dir = data_dir.join("runtime");

    std::fs::create_dir_all(&home_dir).unwrap();
    std::fs::create_dir_all(&repo_dir).unwrap();
    std::fs::create_dir_all(&runtime_dir).unwrap();

    let secret_token = "CLAUDE_SECRET_TOKEN_XYZ_1234567890_NEVER_PERSIST";
    let mut creds_env = HashMap::new();
    creds_env.insert(
        "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
        secret_token.to_string(),
    );
    let credentials = HarnessCredentials { env: creds_env };

    let mut acp_env = HashMap::new();
    acp_env.insert("FOO".to_string(), "bar".to_string());

    let assign = Assign {
        run_id: "run-token-test".to_string(),
        task: None,
        workspace: None,
        harness: Some(Harness {
            system_prompt: "You are Dark Factory Bot".to_string(),
            kind: Some(Kind::Acp(AcpHarness {
                command: vec!["fake-acp-agent".to_string()],
                env: acp_env,
                model: None,
                settings_json: r#"{"theme": "dark"}"#.to_string(),
            })),
        }),
        tools: vec![],
        budgets: None,
        continuation: None,
    };

    let env = prepare_harness_environment(&assign, Some(&credentials), &home_dir).unwrap();

    // Environment contains the credentials
    assert_eq!(
        env.get("CLAUDE_CODE_OAUTH_TOKEN").map(|s| s.as_str()),
        Some(secret_token)
    );
    assert_eq!(env.get("FOO").map(|s| s.as_str()), Some("bar"));

    // Settings written to /data/home/.claude/settings.json
    let settings_path = home_dir.join(".claude").join("settings.json");
    assert!(settings_path.exists());
    assert_eq!(
        std::fs::read_to_string(&settings_path).unwrap(),
        r#"{"theme": "dark"}"#
    );

    // System prompt written to /data/home/.claude/CLAUDE.md
    let claude_md_path = home_dir.join(".claude").join("CLAUDE.md");
    assert!(claude_md_path.exists());
    assert_eq!(
        std::fs::read_to_string(&claude_md_path).unwrap(),
        "You are Dark Factory Bot"
    );

    // Verify token NEVER lands on disk under /data
    fn scan_dir_for_secret(dir: &std::path::Path, secret: &str) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    scan_dir_for_secret(&path, secret);
                } else if path.is_file() {
                    let content = std::fs::read(&path).unwrap_or_default();
                    let content_str = String::from_utf8_lossy(&content);
                    assert!(
                        !content_str.contains(secret),
                        "Secret token found in file: {}",
                        path.display()
                    );
                }
            }
        }
    }

    scan_dir_for_secret(&data_dir, secret_token);
}

#[tokio::test]
async fn test_scenario_harness_resume_success() {
    use crate::harness::{build_acp_prompt_blocks, setup_session};
    use crate::test_harness::MockAcpAgent;
    use agent_client_protocol::schema::v1::McpServer;
    use agent_client_protocol::{Channel, Client};
    use df_proto::agent::v1::Continuation;

    let mock_agent = MockAcpAgent::new()
        .with_load_session(true)
        .with_existing_session("sess-existing-123");

    let agent_builder = mock_agent.build();
    let (transport, peer) = Channel::duplex();

    let agent_task = tokio::spawn(async move {
        agent_builder.connect_to(peer).await.unwrap();
    });

    let client = Client.builder();
    let client_task = tokio::spawn(async move {
        client
            .connect_with(transport, async move |cx| {
                let temp = tempfile::tempdir().unwrap();
                let repo_dir = temp.path().join("repo");
                let mcp_cmd = std::path::PathBuf::from("/opt/factory/factory-agent");
                let mcp_sock = std::path::PathBuf::from("/run/factory/runtime.sock");

                let continuation = Continuation {
                    harness_session_id: "sess-existing-123".to_string(),
                    summary: "Context from previous run".to_string(),
                };

                let res = setup_session(&cx, &repo_dir, Some(&continuation), &mcp_cmd, &mcp_sock)
                    .await
                    .expect("setup session should succeed");

                assert!(res.is_resumed, "Session should be resumed");
                assert_eq!(res.session_id, "sess-existing-123");
                assert!(res.notices.is_empty(), "No notices on clean resume");

                // When resumed, summary is NOT prepended to prompts
                let prepend_summary = if !res.is_resumed {
                    Some(continuation.summary.as_str())
                } else {
                    None
                };
                let prompt_blocks = build_acp_prompt_blocks(
                    &[df_proto::agent::v1::Prompt {
                        prompt_id: "p1".to_string(),
                        author: None,
                        queued_at: None,
                        content: vec![df_proto::transcript::v1::ContentBlock {
                            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                                df_proto::transcript::v1::TextContent {
                                    text: "Follow up prompt".to_string(),
                                },
                            )),
                        }],
                    }],
                    prepend_summary,
                );
                assert_eq!(prompt_blocks.len(), 1);
                match &prompt_blocks[0] {
                    agent_client_protocol::schema::v1::ContentBlock::Text(t) => {
                        assert_eq!(t.text, "Follow up prompt");
                    }
                    _ => panic!("Expected text block"),
                }

                Ok(())
            })
            .await
    });

    client_task.await.unwrap().unwrap();
    agent_task.abort();

    // Verify mock agent received the MCP server
    let servers = mock_agent.received_mcp_servers.lock().unwrap();
    assert_eq!(servers.len(), 1);
    match &servers[0] {
        McpServer::Stdio(s) => {
            assert_eq!(s.name, "factory");
            assert_eq!(s.args, vec!["mcp", "--socket", "/run/factory/runtime.sock"]);
        }
        _ => panic!("Expected Stdio MCP server"),
    }
}

#[tokio::test]
async fn test_scenario_harness_resume_fails_falls_back_to_new() {
    use crate::harness::{build_acp_prompt_blocks, setup_session};
    use crate::test_harness::MockAcpAgent;
    use agent_client_protocol::{Channel, Client};
    use df_proto::agent::v1::Continuation;

    // Agent does NOT have this session
    let mock_agent = MockAcpAgent::new().with_load_session(true);

    let agent_builder = mock_agent.build();
    let (transport, peer) = Channel::duplex();

    let agent_task = tokio::spawn(async move {
        agent_builder.connect_to(peer).await.unwrap();
    });

    let client = Client.builder();
    let client_task = tokio::spawn(async move {
        client
            .connect_with(transport, async move |cx| {
                let temp = tempfile::tempdir().unwrap();
                let repo_dir = temp.path().join("repo");
                let mcp_cmd = std::path::PathBuf::from("/opt/factory/factory-agent");
                let mcp_sock = std::path::PathBuf::from("/run/factory/runtime.sock");

                let continuation = Continuation {
                    harness_session_id: "nonexistent-sess".to_string(),
                    summary: "Summary of earlier progress".to_string(),
                };

                let res = setup_session(&cx, &repo_dir, Some(&continuation), &mcp_cmd, &mcp_sock)
                    .await
                    .expect("fallback to new session should succeed");

                assert!(!res.is_resumed, "Session should NOT be resumed");
                assert_ne!(res.session_id, "nonexistent-sess");
                assert_eq!(res.notices.len(), 1);
                assert_eq!(res.notices[0].code, "harness.session_resume_failed");

                // When resume fails, summary IS prepended to first turn's prompts
                let prepend_summary = if !res.is_resumed {
                    Some(continuation.summary.as_str())
                } else {
                    None
                };
                let prompt_blocks = build_acp_prompt_blocks(
                    &[df_proto::agent::v1::Prompt {
                        prompt_id: "p1".to_string(),
                        author: None,
                        queued_at: None,
                        content: vec![df_proto::transcript::v1::ContentBlock {
                            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                                df_proto::transcript::v1::TextContent {
                                    text: "First prompt of run 2".to_string(),
                                },
                            )),
                        }],
                    }],
                    prepend_summary,
                );
                assert_eq!(prompt_blocks.len(), 2);
                match &prompt_blocks[0] {
                    agent_client_protocol::schema::v1::ContentBlock::Text(t) => {
                        assert_eq!(
                            t.text,
                            "Context from previous run:\nSummary of earlier progress"
                        );
                    }
                    _ => panic!("Expected text block for summary"),
                }
                match &prompt_blocks[1] {
                    agent_client_protocol::schema::v1::ContentBlock::Text(t) => {
                        assert_eq!(t.text, "First prompt of run 2");
                    }
                    _ => panic!("Expected text block for user prompt"),
                }

                Ok(())
            })
            .await
    });

    client_task.await.unwrap().unwrap();
    agent_task.abort();
}

#[test]
fn test_scenario_harness_permission_auto_approved() {
    use crate::harness::approve_permission;
    use agent_client_protocol::schema::v1::{
        PermissionOption, PermissionOptionId, PermissionOptionKind, RequestPermissionOutcome,
        RequestPermissionRequest, SessionId, ToolCallUpdate, ToolCallUpdateFields,
    };

    let opt_allow = PermissionOption::new(
        PermissionOptionId::new("opt-allow-1"),
        "Allow command execution",
        PermissionOptionKind::AllowOnce,
    );
    let opt_reject = PermissionOption::new(
        PermissionOptionId::new("opt-reject-1"),
        "Deny command execution",
        PermissionOptionKind::RejectOnce,
    );

    let req = RequestPermissionRequest::new(
        SessionId::new("sess-1"),
        ToolCallUpdate::new("call-1", ToolCallUpdateFields::default()),
        vec![opt_reject, opt_allow],
    );

    let resp = approve_permission(&req);
    match resp.outcome {
        RequestPermissionOutcome::Selected(selected) => {
            assert_eq!(selected.option_id.0.as_ref(), "opt-allow-1");
        }
        _ => panic!("Expected Selected permission outcome"),
    }
}

#[tokio::test]
async fn test_scenario_fake_acp_agent_subprocess_session_new_and_resume() {
    use crate::harness::{TokioAsyncReadCompat, TokioAsyncWriteCompat, setup_session};
    use agent_client_protocol::{ByteStreams, Client};
    use df_proto::agent::v1::Continuation;

    let fake_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/fake-acp-agent");

    if !fake_bin.exists() {
        eprintln!(
            "Skipping subprocess test because {} not found",
            fake_bin.display()
        );
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let home_dir = temp.path().join("home");
    let repo_dir = temp.path().join("repo");
    std::fs::create_dir_all(&home_dir).unwrap();
    std::fs::create_dir_all(&repo_dir).unwrap();

    let script_yaml = r#"
sessions:
  new:
    - - say: "Hello from turn 1"
      - end: end_turn
  loaded:
    - - say: "Hello from resumed turn"
      - end: end_turn
"#;
    let script_file = temp.path().join("script.yaml");
    std::fs::write(&script_file, script_yaml).unwrap();

    // 1. First run: session/new
    let mut cmd = tokio::process::Command::new(&fake_bin);
    cmd.arg("--script")
        .arg(&script_file)
        .env("HOME", &home_dir)
        .current_dir(&repo_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());

    let mut child = cmd.spawn().expect("spawn fake-acp-agent");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let transport = ByteStreams::new(TokioAsyncWriteCompat(stdin), TokioAsyncReadCompat(stdout));

    let mcp_cmd = std::path::PathBuf::from("/opt/factory/factory-agent");
    let mcp_sock = std::path::PathBuf::from("/run/factory/runtime.sock");

    let repo_dir_1 = repo_dir.clone();
    let mcp_cmd_1 = mcp_cmd.clone();
    let mcp_sock_1 = mcp_sock.clone();

    let client = Client.builder();
    let setup_result = client
        .connect_with(transport, async move |cx| {
            let res = setup_session(&cx, &repo_dir_1, None, &mcp_cmd_1, &mcp_sock_1)
                .await
                .expect("setup session new");
            Ok(res)
        })
        .await
        .expect("client connect_with");

    assert!(!setup_result.is_resumed);
    let session_id = setup_result.session_id;
    assert!(!session_id.is_empty());
    let _ = child.kill().await;

    // 2. Second run: session/load with session_id
    let mut cmd2 = tokio::process::Command::new(&fake_bin);
    cmd2.arg("--script")
        .arg(&script_file)
        .env("HOME", &home_dir)
        .current_dir(&repo_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());

    let mut child2 = cmd2.spawn().expect("spawn fake-acp-agent 2");
    let stdin2 = child2.stdin.take().unwrap();
    let stdout2 = child2.stdout.take().unwrap();

    let transport2 = ByteStreams::new(TokioAsyncWriteCompat(stdin2), TokioAsyncReadCompat(stdout2));

    let continuation = Continuation {
        harness_session_id: session_id.clone(),
        summary: "Previous summary".to_string(),
    };

    let repo_dir_2 = repo_dir.clone();
    let mcp_cmd_2 = mcp_cmd.clone();
    let mcp_sock_2 = mcp_sock.clone();

    let client2 = Client.builder();
    let setup_result2 = client2
        .connect_with(transport2, async move |cx| {
            let res = setup_session(
                &cx,
                &repo_dir_2,
                Some(&continuation),
                &mcp_cmd_2,
                &mcp_sock_2,
            )
            .await
            .expect("setup session load");
            Ok(res)
        })
        .await
        .expect("client2 connect_with");

    assert!(setup_result2.is_resumed);
    assert_eq!(setup_result2.session_id, session_id);
    let _ = child2.kill().await;
}

#[test]
fn test_scenario_two_queued_messages_arrive_during_turn() {
    use crate::harness::{PromptQueue, build_acp_prompt_blocks};
    use df_proto::agent::v1::Prompt;
    use df_proto::transcript::v1::{
        ContentBlock, Principal, TextContent, content_block::Kind as ContentKind,
        event::Kind as EventKind,
    };

    let mut queue = PromptQueue::new();

    let p1 = Prompt {
        prompt_id: "prompt-1".to_string(),
        author: Some(Principal {
            kind: Some(df_proto::transcript::v1::principal::Kind::Human(
                df_proto::transcript::v1::HumanPrincipal {
                    user_id: "user-123".to_string(),
                    display_name: "Alice".to_string(),
                },
            )),
        }),
        content: vec![ContentBlock {
            kind: Some(ContentKind::Text(TextContent {
                text: "Turn 1 prompt".to_string(),
            })),
        }],
        queued_at: None,
    };

    queue.push(p1);

    // Turn 1 starts with p1
    let (turn_1_prompts, turn_1_events) = queue.start_turn("turn-1");
    assert_eq!(turn_1_prompts.len(), 1);
    assert_eq!(turn_1_events.len(), 2); // TurnStarted + UserMessage

    let turn_started_1 = match &turn_1_events[0].kind {
        Some(EventKind::TurnStarted(ts)) => ts,
        _ => panic!("Expected TurnStarted event"),
    };
    assert_eq!(turn_started_1.prompt_ids, vec!["prompt-1"]);

    let user_msg_1 = match &turn_1_events[1].kind {
        Some(EventKind::UserMessage(um)) => um,
        _ => panic!("Expected UserMessage event"),
    };
    assert_eq!(user_msg_1.prompt_id, "prompt-1");

    // While Turn 1 is active, two new prompts arrive
    let p2 = Prompt {
        prompt_id: "prompt-2".to_string(),
        author: None,
        content: vec![ContentBlock {
            kind: Some(ContentKind::Text(TextContent {
                text: "Second prompt (queued during turn 1)".to_string(),
            })),
        }],
        queued_at: None,
    };

    let p3 = Prompt {
        prompt_id: "prompt-3".to_string(),
        author: None,
        content: vec![ContentBlock {
            kind: Some(ContentKind::Text(TextContent {
                text: "Third prompt (queued during turn 1)".to_string(),
            })),
        }],
        queued_at: None,
    };

    queue.push(p2);
    queue.push(p3);
    assert_eq!(queue.len(), 2);
    assert!(queue.has_queued());

    // When turn 1 finishes, turn 2 starts with BOTH queued prompts
    let (turn_2_prompts, turn_2_events) = queue.start_turn("turn-2");
    assert_eq!(turn_2_prompts.len(), 2);
    assert_eq!(turn_2_prompts[0].prompt_id, "prompt-2");
    assert_eq!(turn_2_prompts[1].prompt_id, "prompt-3");

    // TurnStarted must list both prompt ids
    assert_eq!(turn_2_events.len(), 3); // 1 TurnStarted + 2 UserMessage
    let turn_started_2 = match &turn_2_events[0].kind {
        Some(EventKind::TurnStarted(ts)) => ts,
        _ => panic!("Expected TurnStarted event"),
    };
    assert_eq!(turn_started_2.prompt_ids, vec!["prompt-2", "prompt-3"]);

    let user_msg_2 = match &turn_2_events[1].kind {
        Some(EventKind::UserMessage(um)) => um,
        _ => panic!("Expected UserMessage 2"),
    };
    assert_eq!(user_msg_2.prompt_id, "prompt-2");

    let user_msg_3 = match &turn_2_events[2].kind {
        Some(EventKind::UserMessage(um)) => um,
        _ => panic!("Expected UserMessage 3"),
    };
    assert_eq!(user_msg_3.prompt_id, "prompt-3");

    // ACP prompt blocks contain both prompt messages
    let acp_blocks = build_acp_prompt_blocks(&turn_2_prompts, None);
    assert_eq!(acp_blocks.len(), 2);

    // Queue is now empty
    assert!(!queue.has_queued());
    assert_eq!(queue.len(), 0);
}

#[test]
fn test_scenario_prompt_queue_deduplicates_duplicate_prompts() {
    use crate::connection::CommandFilter;
    use df_proto::agent::v1::{ControlMessage, Prompt, control_message};

    // Duplicates are dropped at the connection boundary by `CommandFilter`
    let msg = ControlMessage {
        ack: 0,
        body: Some(control_message::Body::Prompt(Prompt {
            prompt_id: "prompt-dup".to_string(),
            author: None,
            content: vec![],
            queued_at: None,
        })),
    };
    let mut filter = CommandFilter::new();
    assert!(filter.filter_control_message(&msg));
    assert!(!filter.filter_control_message(&msg));
}

#[test]
fn test_scenario_text_after_a_tool_call() {
    use crate::transcript::TranscriptTranslator;
    use agent_client_protocol::schema::v1::{
        ContentBlock as AcpContentBlock, ContentChunk, TextContent as AcpTextContent,
        ToolCall as AcpToolCall, ToolCallId,
    };
    use df_proto::transcript::v1::{
        content_block::Kind as ContentBlockKind, event::Kind as EventKind,
    };

    let mut translator = TranscriptTranslator::new("turn-1");

    // 1. Agent writes text chunk: "Hello"
    let c1 = ContentChunk::new(AcpContentBlock::Text(AcpTextContent::new("Hello")));
    let ev1 = translator.translate_agent_message_chunk(&c1);
    let (msg_id_1, text1) = match ev1.kind.unwrap() {
        EventKind::AgentMessageChunk(chunk) => {
            let text = match chunk.content.unwrap().kind.unwrap() {
                ContentBlockKind::Text(t) => t.text,
                _ => panic!("Expected text"),
            };
            (chunk.message_id, text)
        }
        _ => panic!("Expected AgentMessageChunk"),
    };
    assert_eq!(msg_id_1, "msg-1");
    assert_eq!(text1, "Hello");

    // 2. Agent writes more text immediately: " world"
    let c2 = ContentChunk::new(AcpContentBlock::Text(AcpTextContent::new(" world")));
    let ev2 = translator.translate_agent_message_chunk(&c2);
    let (msg_id_2, text2) = match ev2.kind.unwrap() {
        EventKind::AgentMessageChunk(chunk) => {
            let text = match chunk.content.unwrap().kind.unwrap() {
                ContentBlockKind::Text(t) => t.text,
                _ => panic!("Expected text"),
            };
            (chunk.message_id, text)
        }
        _ => panic!("Expected AgentMessageChunk"),
    };
    // Must be the same message id
    assert_eq!(msg_id_2, "msg-1");
    assert_eq!(text2, " world");

    // 3. Agent calls a tool
    let tool_call = AcpToolCall::new(ToolCallId::new("call-1"), "read_file");
    let ev_tool = translator.translate_tool_call(&tool_call);
    match ev_tool.kind.unwrap() {
        EventKind::ToolCall(tc) => {
            assert_eq!(tc.tool_call_id, "call-1");
            assert_eq!(tc.title, "read_file");
        }
        _ => panic!("Expected ToolCall"),
    }

    // 4. Agent writes more text: "The file contains..."
    let c3 = ContentChunk::new(AcpContentBlock::Text(AcpTextContent::new(
        "The file contains...",
    )));
    let ev3 = translator.translate_agent_message_chunk(&c3);
    let (msg_id_3, text3) = match ev3.kind.unwrap() {
        EventKind::AgentMessageChunk(chunk) => {
            let text = match chunk.content.unwrap().kind.unwrap() {
                ContentBlockKind::Text(t) => t.text,
                _ => panic!("Expected text"),
            };
            (chunk.message_id, text)
        }
        _ => panic!("Expected AgentMessageChunk"),
    };
    // Must carry a DIFFERENT message id!
    assert_ne!(msg_id_3, msg_id_1);
    assert_eq!(msg_id_3, "msg-2");
    assert_eq!(text3, "The file contains...");

    // 5. Agent writes another chunk: " done."
    let c4 = ContentChunk::new(AcpContentBlock::Text(AcpTextContent::new(" done.")));
    let ev4 = translator.translate_agent_message_chunk(&c4);
    let msg_id_4 = match ev4.kind.unwrap() {
        EventKind::AgentMessageChunk(chunk) => chunk.message_id,
        _ => panic!("Expected AgentMessageChunk"),
    };
    assert_eq!(msg_id_4, "msg-2");
}

#[test]
fn test_scenario_large_tool_output() {
    use crate::transcript::{MAX_RAW_IO_BYTES, TranscriptTranslator};
    use agent_client_protocol::schema::v1::{
        Content, ContentBlock as AcpContentBlock, TextContent as AcpTextContent,
        ToolCall as AcpToolCall, ToolCallContent as AcpToolCallContent, ToolCallId,
        ToolCallUpdate as AcpToolCallUpdate, ToolCallUpdateFields,
    };
    use df_proto::transcript::v1::{
        content_block::Kind as ContentBlockKind, event::Kind as EventKind,
        tool_call_content::Kind as ToolCallContentKind,
    };

    let mut translator = TranscriptTranslator::new("turn-1");

    // Output of 200 KiB
    let large_output = "a".repeat(200 * 1024);

    // Case 1: In ToolCall.raw_output
    let mut call = AcpToolCall::new(ToolCallId::new("call-large-1"), "bash");
    call.raw_output = Some(serde_json::Value::String(large_output.clone()));
    let ev1 = translator.translate_tool_call(&call);
    match ev1.kind.unwrap() {
        EventKind::ToolCall(tc) => {
            assert!(tc.truncated, "Event must be marked truncated");
            assert!(
                tc.raw_output_json.len() <= MAX_RAW_IO_BYTES,
                "raw_output_json len {} must be <= 64 KiB",
                tc.raw_output_json.len()
            );
            assert_eq!(tc.raw_output_json.len(), MAX_RAW_IO_BYTES);
        }
        _ => panic!("Expected ToolCall"),
    }

    // Case 2: In ToolCallUpdate.content
    let fields =
        ToolCallUpdateFields::new().content(vec![AcpToolCallContent::Content(Content::new(
            AcpContentBlock::Text(AcpTextContent::new(large_output.clone())),
        ))]);
    let update = AcpToolCallUpdate::new(ToolCallId::new("call-large-2"), fields);
    let ev2 = translator.translate_tool_call_update(&update);
    match ev2.kind.unwrap() {
        EventKind::ToolCallUpdate(tcu) => {
            assert_eq!(tcu.truncated, Some(true), "Update must be marked truncated");
            let content_list = tcu.content.expect("content list");
            assert_eq!(content_list.items.len(), 1);
            match content_list.items[0].kind.as_ref().unwrap() {
                ToolCallContentKind::Content(block) => match block.kind.as_ref().unwrap() {
                    ContentBlockKind::Text(t) => {
                        assert!(
                            t.text.len() <= MAX_RAW_IO_BYTES,
                            "Text len {} must be <= 64 KiB",
                            t.text.len()
                        );
                        assert_eq!(t.text.len(), MAX_RAW_IO_BYTES);
                    }
                    _ => panic!("Expected text content block"),
                },
                _ => panic!("Expected content kind"),
            }
        }
        _ => panic!("Expected ToolCallUpdate"),
    }
}

#[test]
fn test_transcript_plan_and_tool_origin() {
    use crate::transcript::TranscriptTranslator;
    use agent_client_protocol::schema::v1::{
        Plan as AcpPlan, PlanEntry as AcpPlanEntry, PlanEntryPriority as AcpPlanPriority,
        PlanEntryStatus as AcpPlanStatus, StopReason as AcpStopReason, ToolCall as AcpToolCall,
        ToolCallId,
    };
    use df_proto::transcript::v1::{
        PlanEntryPriority, PlanEntryStatus, StopReason, ToolOrigin, Usage, UsageSource,
        event::Kind as EventKind,
    };

    let mut translator = TranscriptTranslator::new("turn-1");

    // 1. ToolOrigin determination
    let factory_call = AcpToolCall::new(ToolCallId::new("c1"), "mcp__factory__complete_task");
    let ev_f = translator.translate_tool_call(&factory_call);
    match ev_f.kind.unwrap() {
        EventKind::ToolCall(tc) => {
            assert_eq!(tc.origin, ToolOrigin::Factory as i32);
        }
        _ => panic!("Expected ToolCall"),
    }

    let harness_call = AcpToolCall::new(ToolCallId::new("c2"), "git_diff");
    let ev_h = translator.translate_tool_call(&harness_call);
    match ev_h.kind.unwrap() {
        EventKind::ToolCall(tc) => {
            assert_eq!(tc.origin, ToolOrigin::Harness as i32);
        }
        _ => panic!("Expected ToolCall"),
    }

    // 2. Plan translation
    let plan = AcpPlan::new(vec![
        AcpPlanEntry::new("Step 1", AcpPlanPriority::High, AcpPlanStatus::Completed),
        AcpPlanEntry::new("Step 2", AcpPlanPriority::Medium, AcpPlanStatus::InProgress),
        AcpPlanEntry::new("Step 3", AcpPlanPriority::Low, AcpPlanStatus::Pending),
    ]);
    let ev_plan = translator.translate_plan(&plan);
    match ev_plan.kind.unwrap() {
        EventKind::Plan(p) => {
            assert_eq!(p.entries.len(), 3);
            assert_eq!(p.entries[0].content, "Step 1");
            assert_eq!(p.entries[0].priority, PlanEntryPriority::High as i32);
            assert_eq!(p.entries[0].status, PlanEntryStatus::Completed as i32);
            assert_eq!(p.entries[1].priority, PlanEntryPriority::Medium as i32);
            assert_eq!(p.entries[1].status, PlanEntryStatus::InProgress as i32);
            assert_eq!(p.entries[2].priority, PlanEntryPriority::Low as i32);
            assert_eq!(p.entries[2].status, PlanEntryStatus::Pending as i32);
        }
        _ => panic!("Expected Plan"),
    }

    // 3. TurnEnded translation
    let usage = vec![Usage {
        model_id: "claude-3-5-sonnet".to_string(),
        input_tokens: 1500,
        output_tokens: 300,
        cache_read_tokens: 50,
        cache_write_tokens: 10,
    }];
    let ev_ended = translator.translate_turn_ended(
        AcpStopReason::EndTurn,
        usage,
        UsageSource::Harness,
        "finished normally",
    );
    match ev_ended.kind.unwrap() {
        EventKind::TurnEnded(te) => {
            assert_eq!(te.stop_reason, StopReason::EndTurn as i32);
            assert_eq!(te.usage_source, UsageSource::Harness as i32);
            assert_eq!(te.detail, "finished normally");
            assert_eq!(te.usage.len(), 1);
            assert_eq!(te.usage[0].input_tokens, 1500);
        }
        _ => panic!("Expected TurnEnded"),
    }
}

#[test]
fn test_cwd_to_slug() {
    use crate::usage::cwd_to_slug;
    use std::path::Path;

    assert_eq!(cwd_to_slug(Path::new("/data/repo")), "-data-repo");
    assert_eq!(
        cwd_to_slug(Path::new("/Users/baez/sources/code.icb4dc0.de/repo")),
        "-Users-baez-sources-code-icb4dc0-de-repo"
    );
}

#[test]
fn test_parse_session_log_fixture() {
    use crate::usage::parse_session_log;
    use std::io::Cursor;

    let fixture = r#"
{"type":"user","message":{"role":"user","content":"Do the work"}}
{"type":"assistant","message":{"model":"claude-3-5-sonnet","role":"assistant","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}}}
{"type":"tool_use","message":{"role":"assistant","content":"running bash"}}
{"type":"assistant","message":{"model":"claude-3-5-sonnet","role":"assistant","usage":{"input_tokens":80,"output_tokens":40,"cache_read_input_tokens":20,"cache_creation_input_tokens":0}}}
{"type":"assistant","message":{"model":"claude-3-haiku","role":"assistant","usage":{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}
"#;

    let cursor = Cursor::new(fixture);
    let usage = parse_session_log(cursor).expect("parse_session_log");

    assert_eq!(usage.len(), 2);

    let sonnet = usage
        .iter()
        .find(|u| u.model_id == "claude-3-5-sonnet")
        .expect("sonnet");
    assert_eq!(sonnet.input_tokens, 180);
    assert_eq!(sonnet.output_tokens, 90);
    assert_eq!(sonnet.cache_read_tokens, 30);
    assert_eq!(sonnet.cache_write_tokens, 5);

    let haiku = usage
        .iter()
        .find(|u| u.model_id == "claude-3-haiku")
        .expect("haiku");
    assert_eq!(haiku.input_tokens, 200);
    assert_eq!(haiku.output_tokens, 30);
    assert_eq!(haiku.cache_read_tokens, 0);
    assert_eq!(haiku.cache_write_tokens, 0);
}

#[test]
fn test_scenario_adapter_without_usage_session_log_tracker() {
    use crate::usage::SessionLogTracker;
    use df_proto::transcript::v1::UsageSource;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::Path;

    let temp_home = tempfile::tempdir().expect("tempdir");
    let cwd = Path::new("/data/repo");
    let session_id = "sess-12345";

    let mut tracker = SessionLogTracker::new(temp_home.path(), cwd, session_id);
    let session_file = tracker.session_file_path();
    std::fs::create_dir_all(session_file.parent().unwrap()).expect("create_dir_all");

    // Before turn 1: mark turn start
    tracker.mark_turn_start();

    // Turn 1 produces log entry
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&session_file)
        .expect("open session file");
    writeln!(
        file,
        r#"{{"type":"assistant","message":{{"model":"claude-3-5-sonnet","role":"assistant","usage":{{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}}}}}}"#
    )
    .expect("writeln");
    file.flush().expect("flush");

    // Turn 1 ends: parse usage
    let (usage_turn_1, source_1) = tracker.parse_turn_usage();
    assert_eq!(source_1, UsageSource::SessionLog);
    assert_eq!(usage_turn_1.len(), 1);
    assert_eq!(usage_turn_1[0].input_tokens, 100);
    assert_eq!(usage_turn_1[0].output_tokens, 50);

    // Turn 2 begins: mark turn start
    tracker.mark_turn_start();

    // Turn 2 produces another log entry
    writeln!(
        file,
        r#"{{"type":"assistant","message":{{"model":"claude-3-5-sonnet","role":"assistant","usage":{{"input_tokens":150,"output_tokens":70,"cache_read_input_tokens":5,"cache_creation_input_tokens":0}}}}}}"#
    )
    .expect("writeln");
    file.flush().expect("flush");

    // Turn 2 ends: parse usage (must only contain turn 2 metrics!)
    let (usage_turn_2, source_2) = tracker.parse_turn_usage();
    assert_eq!(source_2, UsageSource::SessionLog);
    assert_eq!(usage_turn_2.len(), 1);
    assert_eq!(usage_turn_2[0].input_tokens, 150);
    assert_eq!(usage_turn_2[0].output_tokens, 70);

    // Turn 3 begins: mark turn start
    tracker.mark_turn_start();

    // No logs produced in turn 3
    let (usage_turn_3, source_3) = tracker.parse_turn_usage();
    assert_eq!(source_3, UsageSource::Unspecified);
    assert!(usage_turn_3.is_empty());
}

#[tokio::test]
async fn test_scenario_mcp_server_initialize_tools_list_and_call() {
    use crate::runtime_socket::{RuntimeHandler, RuntimeSocketServer};
    use df_proto::agent::v1::{ToolAnnotations, ToolCallResult, ToolDescriptor};
    use df_proto::transcript::v1::{
        ContentBlock, TextContent, content_block::Kind as ContentBlockKind,
    };
    use rmcp::ServiceExt;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockMcpRuntimeHandler {
        call_count: AtomicUsize,
        last_call_id: tokio::sync::Mutex<Option<String>>,
    }

    #[tonic::async_trait]
    impl RuntimeHandler for MockMcpRuntimeHandler {
        async fn handle_git_credential(
            &self,
            _protocol: &str,
            _host: &str,
            _path: &str,
        ) -> (Option<String>, Option<String>, Option<String>) {
            (None, None, None)
        }

        async fn handle_mcp_list_tools(&self) -> Vec<ToolDescriptor> {
            vec![
                ToolDescriptor {
                    name: "update_pull_request".to_string(),
                    title: "Update Pull Request".to_string(),
                    description: "Updates PR title and body".to_string(),
                    input_schema_json: r#"{"type":"object"}"#.to_string(),
                    output_schema_json: None,
                    annotations: Some(ToolAnnotations {
                        read_only: false,
                        destructive: false,
                        idempotent: true,
                        open_world: false,
                    }),
                },
                ToolDescriptor {
                    name: "comment_on_issue".to_string(),
                    title: "Comment on Issue".to_string(),
                    description: "Adds a comment to the task issue".to_string(),
                    input_schema_json: r#"{"type":"object"}"#.to_string(),
                    output_schema_json: None,
                    annotations: Some(ToolAnnotations {
                        read_only: false,
                        destructive: false,
                        idempotent: false,
                        open_world: false,
                    }),
                },
                ToolDescriptor {
                    name: "report_progress".to_string(),
                    title: "Report Progress".to_string(),
                    description: "Reports intermediate progress".to_string(),
                    input_schema_json: r#"{"type":"object"}"#.to_string(),
                    output_schema_json: None,
                    annotations: Some(ToolAnnotations {
                        read_only: false,
                        destructive: false,
                        idempotent: true,
                        open_world: false,
                    }),
                },
                ToolDescriptor {
                    name: "complete_task".to_string(),
                    title: "Complete Task".to_string(),
                    description: "Marks the task as completed".to_string(),
                    input_schema_json: r#"{"type":"object"}"#.to_string(),
                    output_schema_json: None,
                    annotations: Some(ToolAnnotations {
                        read_only: false,
                        destructive: false,
                        idempotent: true,
                        open_world: false,
                    }),
                },
            ]
        }

        async fn handle_mcp_call_tool(
            &self,
            name: &str,
            _arguments_json: &str,
        ) -> Result<ToolCallResult, String> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            // Runtime assigns a UUIDv7 call_id
            let call_id = format!("{}", uuid::Uuid::now_v7());
            *self.last_call_id.lock().await = Some(call_id.clone());

            match name {
                "comment_on_issue" => {
                    // Scenario: Error result (task has no linked issue)
                    Ok(ToolCallResult {
                        call_id,
                        is_error: true,
                        content: vec![ContentBlock {
                            kind: Some(ContentBlockKind::Text(TextContent {
                                text: "this task has no linked issue".to_string(),
                            })),
                        }],
                        structured_content_json: String::new(),
                        end_run_after_turn: false,
                    })
                }
                "complete_task" => {
                    // Scenario: complete_task sets end_run_after_turn
                    Ok(ToolCallResult {
                        call_id,
                        is_error: false,
                        content: vec![ContentBlock {
                            kind: Some(ContentBlockKind::Text(TextContent {
                                text: "task completed successfully".to_string(),
                            })),
                        }],
                        structured_content_json: r#"{"status":"completed"}"#.to_string(),
                        end_run_after_turn: true,
                    })
                }
                _ => Err(format!("unknown tool: {name}")),
            }
        }
    }

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let socket_path = temp_dir.path().join("runtime.sock");

    let handler = Arc::new(MockMcpRuntimeHandler {
        call_count: AtomicUsize::new(0),
        last_call_id: tokio::sync::Mutex::new(None),
    });

    let server = RuntimeSocketServer::bind(
        &socket_path,
        Arc::clone(&handler) as Arc<dyn RuntimeHandler>,
    )
    .expect("bind server");
    let _server_handle = server.spawn();

    // Spawn McpBridgeServer over a duplex stream
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server_io);
    let socket_path_clone = socket_path.clone();
    let _mcp_handle = tokio::spawn(async move {
        let _ = crate::mcp::run_mcp_stream(server_read, server_write, &socket_path_clone).await;
    });

    let (client_read, client_write) = tokio::io::split(client_io);
    let client = ().serve((client_read, client_write)).await.expect("client connect");

    // Scenario: Four tools from Assign.tools
    let tools = client.list_all_tools().await.expect("list_all_tools");
    assert_eq!(tools.len(), 4);
    let tool_names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        tool_names,
        vec![
            "update_pull_request",
            "comment_on_issue",
            "report_progress",
            "complete_task"
        ]
    );

    // Scenario: Error result (with is_error: true and text "this task has no linked issue")
    let call_params = rmcp::model::CallToolRequestParams::new("comment_on_issue");
    let res = client.call_tool(call_params).await.expect("call_tool");
    assert_eq!(res.is_error, Some(true));
    assert!(!res.content.is_empty());
    let first_block = &res.content[0];
    let json_str = serde_json::to_string(first_block).unwrap_or_default();
    assert!(
        json_str.contains("this task has no linked issue"),
        "expected error message in content, got: {json_str}"
    );

    // Verify call_id generated was valid UUIDv7
    let last_id = handler
        .last_call_id
        .lock()
        .await
        .clone()
        .expect("call_id was set");
    let parsed_uuid = uuid::Uuid::parse_str(&last_id).expect("valid uuid");
    assert_eq!(parsed_uuid.get_version_num(), 7, "call_id must be UUIDv7");

    // Scenario: complete_task success with structured content
    let call_params_complete = rmcp::model::CallToolRequestParams::new("complete_task");
    let res_complete = client
        .call_tool(call_params_complete)
        .await
        .expect("call_tool");
    assert_ne!(res_complete.is_error, Some(true));
    assert_eq!(
        res_complete.structured_content,
        Some(serde_json::json!({"status": "completed"}))
    );
}

// ---------------------------------------------------------------------------
// Task 5.6: Run outcome determination, end_run_after_turn, Cancel, budgets
// ---------------------------------------------------------------------------

#[test]
fn test_rate_limit_parsing_and_retry_after() {
    use crate::harness::{RateLimitInfo, extract_retry_after, parse_rate_limit};
    use agent_client_protocol::Error;

    // 1. Extract retry after from text
    assert_eq!(
        extract_retry_after("Rate limit exceeded. Retry after 120 seconds."),
        Some(120)
    );
    assert_eq!(extract_retry_after("retry after 45s"), Some(45));
    assert_eq!(extract_retry_after("Please wait a moment"), None);

    // 2. Parse rate limit from error data JSON
    let err_json = Error::internal_error().data(serde_json::json!({
        "error": "rate_limited",
        "retry_after_seconds": 3600,
        "message": "Quota exhausted"
    }));
    let info = parse_rate_limit(&err_json).expect("should parse rate limit");
    assert_eq!(
        info,
        RateLimitInfo {
            retry_after_seconds: Some(3600),
            detail: "Quota exhausted".to_string(),
        }
    );

    // 3. Parse rate limit from error message
    let err_msg = Error::new(-32603, "Rate limit reached. Retry after 300 seconds.");
    let info_msg = parse_rate_limit(&err_msg).expect("should parse from message");
    assert_eq!(info_msg.retry_after_seconds, Some(300));
    assert!(info_msg.detail.contains("Rate limit reached"));

    // 4. Non-rate-limit error returns None
    let normal_err = Error::invalid_params().data("syntax error");
    assert!(parse_rate_limit(&normal_err).is_none());
}

#[test]
fn test_budget_tracker_constraints() {
    use crate::harness::BudgetTracker;
    use df_proto::agent::v1::Budgets;
    use df_proto::pbjson_types::Duration;

    let budgets = Budgets {
        max_wall_clock: Some(Duration {
            seconds: 1,
            nanos: 0,
        }),
        max_output_tokens: Some(1000),
        max_turns: Some(3),
    };

    let mut tracker = BudgetTracker::new(Some(&budgets));

    assert_eq!(tracker.turns_completed(), 0);
    assert_eq!(tracker.total_output_tokens(), 0);
    assert!(!tracker.is_turn_limit_reached());
    assert!(!tracker.is_output_tokens_exceeded());

    // Record turn 1 with 400 tokens
    tracker.record_turn(400);
    assert_eq!(tracker.turns_completed(), 1);
    assert_eq!(tracker.total_output_tokens(), 400);
    assert!(!tracker.is_turn_limit_reached());
    assert!(!tracker.is_output_tokens_exceeded());

    // Record turn 2 with 700 tokens (cumulative 1100 >= 1000)
    tracker.record_turn(700);
    assert_eq!(tracker.turns_completed(), 2);
    assert_eq!(tracker.total_output_tokens(), 1100);
    assert!(!tracker.is_turn_limit_reached());
    assert!(tracker.is_output_tokens_exceeded());

    // Record turn 3 (cumulative turns 3 >= 3)
    tracker.record_turn(100);
    assert_eq!(tracker.turns_completed(), 3);
    assert!(tracker.is_turn_limit_reached());
}

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

    // 10. No queued prompts ends turn
    let input_end_turn = TurnOutcomeInput {
        has_queued_prompts: false,
        ..Default::default()
    };
    assert_eq!(
        determine_run_outcome(&input_end_turn),
        RunOutcomeDecision::Finalize {
            outcome: RunOutcome::EndTurn,
            detail: "Turn ended normally with no queued prompts".to_string(),
            retry_after: None,
        }
    );
}

fn setup_test_remote_repo(temp_dir: &std::path::Path) -> std::path::PathBuf {
    let upstream = temp_dir.join("upstream.git");
    // explicit branch: without `init.defaultBranch` HEAD would point to the never pushed `master`
    let status = std::process::Command::new("git")
        .args(["init", "--bare", "-b", "main", upstream.to_str().unwrap()])
        .status()
        .expect("git init bare");
    assert!(status.success());

    let init_repo = temp_dir.join("init_repo");
    let status = std::process::Command::new("git")
        .args([
            "clone",
            upstream.to_str().unwrap(),
            init_repo.to_str().unwrap(),
        ])
        .status()
        .expect("git clone");
    assert!(status.success());

    std::fs::write(init_repo.join("README.md"), "# Test Workspace\n").unwrap();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "Bot"])
        .current_dir(&init_repo)
        .status();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "bot@example.com"])
        .current_dir(&init_repo)
        .status();
    let _ = std::process::Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(&init_repo)
        .status();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", "main"])
        .current_dir(&init_repo)
        .status();
    let _ = std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&init_repo)
        .status();
    let status = std::process::Command::new("git")
        .args(["commit", "--no-gpg-sign", "-m", "init"])
        .current_dir(&init_repo)
        .status()
        .expect("git commit");
    assert!(status.success());
    let status = std::process::Command::new("git")
        .args(["push", "origin", "main"])
        .current_dir(&init_repo)
        .status()
        .expect("git push");
    assert!(status.success());

    upstream
}

#[tokio::test]
async fn test_scenario_outcome_end_turn_when_agent_asks_question() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{Assign, Harness, Prompt, RunOutcome, Workspace};
    use tokio::sync::mpsc;

    let fake_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/fake-acp-agent");
    let mcp_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/factory-agent");

    if !fake_bin.exists() || !mcp_bin.exists() {
        eprintln!("Skipping integration test: binaries not present");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());

    let script_yaml = r#"
sessions:
  new:
    - - say: "Which database should we use?"
      - end: end_turn
"#;
    let script_file = temp.path().join("script.yaml");
    std::fs::write(&script_file, script_yaml).unwrap();

    let repo_dir = temp.path().join("repo");
    let home_dir = temp.path().join("home");
    let runtime_dir = temp.path().join("runtime");
    let hooks_path = temp.path().join("hooks-empty");
    let runtime_sock = temp.path().join("runtime.sock");
    std::fs::create_dir_all(&hooks_path).unwrap();

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&runtime_dir, "run-end-turn", 10 * 1024 * 1024, 64 * 1024).unwrap(),
    ));

    let assign = Assign {
        run_id: "run-end-turn".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: upstream.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: None,
            git_hooks: Vec::new(),
            git_config: Vec::new(),
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        script_file.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: String::new(),
        }),
        tools: Vec::new(),
        budgets: None,
        continuation: None,
    };

    let initial_prompts = vec![Prompt {
        prompt_id: "prompt-1".to_string(),
        author: None,
        content: vec![df_proto::transcript::v1::ContentBlock {
            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                df_proto::transcript::v1::TextContent {
                    text: "Start working".to_string(),
                },
            )),
        }],
        queued_at: None,
    }];

    let (_inbound_tx, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, mut outbound_rx) = mpsc::channel(100);

    // Drain outbound messages in background to prevent backpressure
    let drain_handle =
        tokio::spawn(async move { while let Some(_msg) = outbound_rx.recv().await {} });

    let git = Git::new(&repo_dir).allow_protocols(["file", "https"]);

    let params = RunDriverParams {
        assign,
        credentials: None,
        initial_prompts,
        repo_dir,
        home_dir,
        runtime_dir,
        hooks_path,
        mcp_command: mcp_bin,
        runtime_sock_path: runtime_sock,
        git,
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
        cancel_requested: false,
    };

    let result = run_session(params)
        .await
        .expect("run_session should succeed");
    assert_eq!(result.outcome, RunOutcome::EndTurn);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::EndTurn as i32
    );
    drain_handle.abort();
}

#[tokio::test]
async fn test_run_sends_runtime_timings_in_sequence_with_other_messages() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{
        Assign, Harness, Prompt, RuntimeOperation, Workspace, agent_message,
    };
    use tokio::sync::mpsc;

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fake_bin = root.join("target/debug/fake-acp-agent");
    let mcp_bin = root.join("target/debug/factory-agent");
    if !fake_bin.exists() || !mcp_bin.exists() {
        eprintln!("Skipping integration test: binaries not present");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());
    let script_file = temp.path().join("script.yaml");
    std::fs::write(
        &script_file,
        r#"
sessions:
  new:
    - - shell: "echo x >> f.txt && git add f.txt && git config user.name 'Agent Bot' && git config user.email 'bot@example.com' && git config commit.gpgsign false && git commit --no-gpg-sign -m 'work'"
      - end: end_turn
    - - say: "second turn"
      - end: end_turn
"#,
    )
    .unwrap();

    let repo_dir = temp.path().join("repo");
    let runtime_dir = temp.path().join("runtime");
    let hooks_path = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&hooks_path).unwrap();
    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&runtime_dir, "run-timing", 10 * 1024 * 1024, 64 * 1024).unwrap(),
    ));
    let assign = Assign {
        run_id: "run-timing".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: upstream.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: None,
            git_hooks: Vec::new(),
            git_config: Vec::new(),
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        script_file.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: String::new(),
        }),
        tools: Vec::new(),
        budgets: None,
        continuation: None,
    };
    let initial_prompts = vec![Prompt {
        prompt_id: "prompt-1".to_string(),
        author: None,
        content: vec![df_proto::transcript::v1::ContentBlock {
            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                df_proto::transcript::v1::TextContent {
                    text: "Start working".to_string(),
                },
            )),
        }],
        queued_at: None,
    }];
    let (inbound_tx, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<df_proto::agent::v1::AgentMessage>(100);
    // Mock control plane: queues the second turn's prompt during the first turn and keeps
    // everything the runtime sends.
    let collector = tokio::spawn(async move {
        let mut bodies = Vec::new();
        let mut turns = 0;
        while let Some(msg) = outbound_rx.recv().await {
            if let Some(agent_message::Body::TranscriptEvent(ev)) = &msg.body
                && let Some(df_proto::transcript::v1::event::Kind::TurnStarted(_)) = &ev.kind
            {
                turns += 1;
                if turns == 1 {
                    let prompt = Prompt {
                        prompt_id: "prompt-2".to_string(),
                        author: None,
                        content: vec![],
                        queued_at: None,
                    };
                    inbound_tx
                        .send(df_proto::agent::v1::ControlMessage {
                            ack: 0,
                            body: Some(df_proto::agent::v1::control_message::Body::Prompt(prompt)),
                        })
                        .await
                        .expect("the run still reads its inbox");
                }
            }
            bodies.push(msg.body);
        }
        bodies
    });
    let params = RunDriverParams {
        assign,
        credentials: None,
        initial_prompts,
        repo_dir: repo_dir.clone(),
        home_dir: temp.path().join("home"),
        runtime_dir,
        hooks_path,
        mcp_command: mcp_bin,
        runtime_sock_path: temp.path().join("runtime.sock"),
        git: Git::new(&repo_dir).allow_protocols(["file", "https"]),
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
        cancel_requested: false,
    };
    run_session(params)
        .await
        .expect("run_session should succeed");

    let bodies = collector.await.unwrap();
    let timings: Vec<_> = bodies
        .iter()
        .flatten()
        .filter_map(|b| match b {
            agent_message::Body::RuntimeTiming(t) => Some((t.operation(), t.ok)),
            _ => None,
        })
        .collect();
    assert_eq!(
        timings,
        [
            (RuntimeOperation::WorkspacePrepare, true),
            (RuntimeOperation::HarnessStart, true),
            (RuntimeOperation::Turn, true),
            (RuntimeOperation::Push, true),
            (RuntimeOperation::Turn, true),
            (RuntimeOperation::Finalize, true),
        ]
    );
    // Finalize is measured before the report that ends the run.
    let last = bodies.iter().flatten().last();
    assert!(matches!(last, Some(agent_message::Body::RunReport(_))));
    let position = |wanted: RuntimeOperation| {
        bodies.iter().flatten().position(
            |b| matches!(b, agent_message::Body::RuntimeTiming(t) if t.operation() == wanted),
        )
    };
    let turn_ended = bodies.iter().flatten().position(|b| {
        matches!(b, agent_message::Body::TranscriptEvent(e)
            if matches!(e.kind, Some(df_proto::transcript::v1::event::Kind::TurnEnded(_))))
    });
    assert!(position(RuntimeOperation::Turn) < turn_ended);
    // The push is measured after the turn it follows and reported next to its `PushReport`.
    let push_report = bodies
        .iter()
        .flatten()
        .position(|b| matches!(b, agent_message::Body::PushReport(_)));
    assert!(push_report.is_some());
}

#[tokio::test]
async fn test_scenario_outcome_complete_task_called() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{
        Assign, ControlMessage, Harness, Prompt, RunOutcome, ToolCallResult, ToolDescriptor,
        Workspace, agent_message, control_message,
    };
    use tokio::sync::mpsc;

    let (logs, _log_guard) = df_telemetry::testing::capture("info");

    let fake_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/fake-acp-agent");
    let mcp_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/factory-agent");

    if !fake_bin.exists() || !mcp_bin.exists() {
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());

    let script_yaml = r#"
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
    let script_file = temp.path().join("script.yaml");
    std::fs::write(&script_file, script_yaml).unwrap();

    let repo_dir = temp.path().join("repo");
    let home_dir = temp.path().join("home");
    let runtime_dir = temp.path().join("runtime");
    let hooks_path = temp.path().join("hooks-empty");
    let runtime_sock = temp.path().join("runtime.sock");
    std::fs::create_dir_all(&hooks_path).unwrap();

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(
            &runtime_dir,
            "run-complete-task",
            10 * 1024 * 1024,
            64 * 1024,
        )
        .unwrap(),
    ));

    let assign = Assign {
        run_id: "run-complete-task".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: upstream.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: None,
            git_hooks: Vec::new(),
            git_config: Vec::new(),
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        script_file.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: String::new(),
        }),
        tools: vec![ToolDescriptor {
            name: "complete_task".to_string(),
            title: "Complete Task".to_string(),
            description: "Mark task complete".to_string(),
            input_schema_json: "{}".to_string(),
            output_schema_json: None,
            annotations: None,
        }],
        budgets: None,
        continuation: None,
    };

    let initial_prompts = vec![Prompt {
        prompt_id: "prompt-1".to_string(),
        author: None,
        content: vec![df_proto::transcript::v1::ContentBlock {
            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                df_proto::transcript::v1::TextContent {
                    text: "Complete the task".to_string(),
                },
            )),
        }],
        queued_at: None,
    }];

    let (inbound_tx, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<df_proto::agent::v1::AgentMessage>(100);

    // Mock control plane: answer complete_task with end_run_after_turn = true
    // AND queue an extra prompt to verify complete_task overrides queued prompts!
    let inbound_tx_clone = inbound_tx.clone();
    let cp_task = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            if let Some(agent_message::Body::ToolCallRequest(req)) = msg.body
                && req.tool_name == "complete_task"
            {
                // Send tool result with end_run_after_turn = true
                let res = ControlMessage {
                    ack: 0,
                    body: Some(control_message::Body::ToolCallResult(ToolCallResult {
                        call_id: req.call_id,
                        is_error: false,
                        content: vec![df_proto::transcript::v1::ContentBlock {
                            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                                df_proto::transcript::v1::TextContent {
                                    text: "Task marked completed".to_string(),
                                },
                            )),
                        }],
                        structured_content_json: String::new(),
                        end_run_after_turn: true,
                    })),
                };
                let _ = inbound_tx_clone.send(res).await;

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
                let _ = inbound_tx_clone.send(prompt_msg).await;
            }
        }
    });

    let git = Git::new(&repo_dir).allow_protocols(["file", "https"]);

    let params = RunDriverParams {
        assign,
        credentials: None,
        initial_prompts,
        repo_dir,
        home_dir,
        runtime_dir,
        hooks_path,
        mcp_command: mcp_bin,
        runtime_sock_path: runtime_sock,
        git,
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
        cancel_requested: false,
    };

    let result = run_session(params)
        .await
        .expect("run_session should succeed");
    assert_eq!(result.outcome, RunOutcome::Completed);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::Completed as i32
    );
    cp_task.abort();

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

/// Spike `native-harness-spike` (task 2.3): the runtime starts `df-harness` as its ACP command;
/// the model (a fake OpenAI-compatible endpoint) calls `complete_task` through the factory MCP
/// bridge and the run finishes as `COMPLETED`.
#[tokio::test]
async fn test_df_harness_complete_task_ends_run_completed() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{
        Assign, ControlMessage, Harness, Prompt, RunOutcome, ToolCallResult, ToolDescriptor,
        Workspace, agent_message, control_message,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::mpsc;

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let harness_bin = root.join("target/debug/df-harness");
    let mcp_bin = root.join("target/debug/factory-agent");
    if !harness_bin.exists() || !mcp_bin.exists() {
        eprintln!("Skipping integration test: binaries not present");
        return;
    }

    // Fake OpenAI-compatible endpoint: first request calls complete_task, later ones end the turn.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let endpoint = tokio::spawn(async move {
        let mut n = 0;
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = vec![0u8; 1 << 20];
            let _ = sock.read(&mut buf).await;
            let chunks = if n == 0 {
                vec![
                    serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"mcp__factory__complete_task","arguments":"{\"summary\":\"done\"}"}}]},"finish_reason":null}]}),
                    serde_json::json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
                ]
            } else {
                vec![
                    serde_json::json!({"choices":[{"delta":{"content":"finished"},"finish_reason":null}]}),
                    serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
                ]
            };
            n += 1;
            let mut body: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
            body.push_str("data: [DONE]\n\n");
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());
    let repo_dir = temp.path().join("repo");
    let home_dir = temp.path().join("home");
    let runtime_dir = temp.path().join("runtime");
    let hooks_path = temp.path().join("hooks-empty");
    let runtime_sock = temp.path().join("runtime.sock");
    std::fs::create_dir_all(&hooks_path).unwrap();
    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&runtime_dir, "run-df-harness", 10 * 1024 * 1024, 64 * 1024).unwrap(),
    ));

    let assign = Assign {
        run_id: "run-df-harness".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: upstream.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: None,
            git_hooks: Vec::new(),
            git_config: Vec::new(),
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        "env".to_string(),
                        format!("DF_HARNESS_BASE_URL={base_url}"),
                        "DF_HARNESS_API_KEY=unused".to_string(),
                        "DF_HARNESS_MODEL=fake".to_string(),
                        harness_bin.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: String::new(),
        }),
        tools: vec![ToolDescriptor {
            name: "complete_task".to_string(),
            title: "Complete Task".to_string(),
            description: "Mark task complete".to_string(),
            input_schema_json: r#"{"type":"object","properties":{"summary":{"type":"string"}}}"#
                .to_string(),
            output_schema_json: None,
            annotations: None,
        }],
        budgets: None,
        continuation: None,
    };
    let initial_prompts = vec![Prompt {
        prompt_id: "prompt-1".to_string(),
        author: None,
        content: vec![df_proto::transcript::v1::ContentBlock {
            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                df_proto::transcript::v1::TextContent {
                    text: "Complete the task".to_string(),
                },
            )),
        }],
        queued_at: None,
    }];

    let (inbound_tx, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<df_proto::agent::v1::AgentMessage>(100);
    let cp_task = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            if let Some(agent_message::Body::ToolCallRequest(req)) = msg.body
                && req.tool_name == "complete_task"
            {
                let res = ControlMessage {
                    ack: 0,
                    body: Some(control_message::Body::ToolCallResult(ToolCallResult {
                        call_id: req.call_id,
                        is_error: false,
                        content: Vec::new(),
                        structured_content_json: String::new(),
                        end_run_after_turn: true,
                    })),
                };
                let _ = inbound_tx.send(res).await;
            }
        }
    });

    let params = RunDriverParams {
        assign,
        credentials: None,
        initial_prompts,
        repo_dir: repo_dir.clone(),
        home_dir,
        runtime_dir,
        hooks_path,
        mcp_command: mcp_bin,
        runtime_sock_path: runtime_sock,
        git: Git::new(&repo_dir).allow_protocols(["file", "https"]),
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
        cancel_requested: false,
    };
    let result = run_session(params)
        .await
        .expect("run_session should succeed");
    assert_eq!(result.outcome, RunOutcome::Completed);
    cp_task.abort();
    endpoint.abort();
}

#[tokio::test]
async fn test_scenario_outcome_provider_rate_limited() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{Assign, Harness, Prompt, RunOutcome, Workspace};
    use tokio::sync::mpsc;

    let fake_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/fake-acp-agent");
    let mcp_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/factory-agent");

    if !fake_bin.exists() || !mcp_bin.exists() {
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());

    let script_yaml = r#"
sessions:
  new:
    - - end:
          rate_limited:
            retry_after_seconds: 3600
"#;
    let script_file = temp.path().join("script.yaml");
    std::fs::write(&script_file, script_yaml).unwrap();

    let repo_dir = temp.path().join("repo");
    let home_dir = temp.path().join("home");
    let runtime_dir = temp.path().join("runtime");
    let hooks_path = temp.path().join("hooks-empty");
    let runtime_sock = temp.path().join("runtime.sock");
    std::fs::create_dir_all(&hooks_path).unwrap();

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(
            &runtime_dir,
            "run-rate-limited",
            10 * 1024 * 1024,
            64 * 1024,
        )
        .unwrap(),
    ));

    let assign = Assign {
        run_id: "run-rate-limited".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: upstream.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: None,
            git_hooks: Vec::new(),
            git_config: Vec::new(),
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        script_file.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: String::new(),
        }),
        tools: Vec::new(),
        budgets: None,
        continuation: None,
    };

    let initial_prompts = vec![Prompt {
        prompt_id: "prompt-1".to_string(),
        author: None,
        content: vec![],
        queued_at: None,
    }];

    let (_inbound_tx, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, mut outbound_rx) = mpsc::channel(100);

    let drain_handle =
        tokio::spawn(async move { while let Some(_msg) = outbound_rx.recv().await {} });

    let git = Git::new(&repo_dir).allow_protocols(["file", "https"]);

    let params = RunDriverParams {
        assign,
        credentials: None,
        initial_prompts,
        repo_dir,
        home_dir,
        runtime_dir,
        hooks_path,
        mcp_command: mcp_bin,
        runtime_sock_path: runtime_sock,
        git,
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
        cancel_requested: false,
    };

    let result = run_session(params)
        .await
        .expect("run_session should succeed");
    assert_eq!(result.outcome, RunOutcome::RateLimited);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::RateLimited as i32
    );
    assert!(result.run_report.retry_after.is_some());
    drain_handle.abort();
}

#[tokio::test]
async fn test_scenario_outcome_turn_limit() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{
        Assign, Budgets, ControlMessage, Harness, Prompt, RunOutcome, Workspace, agent_message,
        control_message,
    };
    use tokio::sync::mpsc;

    let fake_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/fake-acp-agent");
    let mcp_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/factory-agent");

    if !fake_bin.exists() || !mcp_bin.exists() {
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());

    // 2 turns in script, but 3 prompts will be queued
    let script_yaml = r#"
sessions:
  new:
    - - say: "Turn 1 answer"
      - end: end_turn
    - - say: "Turn 2 answer"
      - end: end_turn
"#;
    let script_file = temp.path().join("script.yaml");
    std::fs::write(&script_file, script_yaml).unwrap();

    let repo_dir = temp.path().join("repo");
    let home_dir = temp.path().join("home");
    let runtime_dir = temp.path().join("runtime");
    let hooks_path = temp.path().join("hooks-empty");
    let runtime_sock = temp.path().join("runtime.sock");
    std::fs::create_dir_all(&hooks_path).unwrap();

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&runtime_dir, "run-turn-limit", 10 * 1024 * 1024, 64 * 1024).unwrap(),
    ));

    // Limit to max 2 turns
    let budgets = Budgets {
        max_wall_clock: None,
        max_output_tokens: None,
        max_turns: Some(2),
    };

    let assign = Assign {
        run_id: "run-turn-limit".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: upstream.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: None,
            git_hooks: Vec::new(),
            git_config: Vec::new(),
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        script_file.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: String::new(),
        }),
        tools: Vec::new(),
        budgets: Some(budgets),
        continuation: None,
    };

    let initial_prompts = vec![Prompt {
        prompt_id: "prompt-1".to_string(),
        author: None,
        content: vec![],
        queued_at: None,
    }];

    let (inbound_tx, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<df_proto::agent::v1::AgentMessage>(100);

    // Queue prompts for turn 2 and a potential turn 3 during execution
    let inbound_tx_clone = inbound_tx.clone();
    let cp_task = tokio::spawn(async move {
        let mut turn_count = 0;
        while let Some(msg) = outbound_rx.recv().await {
            if let Some(agent_message::Body::TranscriptEvent(ev)) = &msg.body
                && let Some(df_proto::transcript::v1::event::Kind::TurnStarted(_)) = &ev.kind
            {
                turn_count += 1;
                if turn_count == 1 {
                    // During turn 1, send prompt for turn 2
                    let _ = inbound_tx_clone
                        .send(ControlMessage {
                            ack: 0,
                            body: Some(control_message::Body::Prompt(Prompt {
                                prompt_id: "prompt-2".to_string(),
                                author: None,
                                content: vec![],
                                queued_at: None,
                            })),
                        })
                        .await;
                } else if turn_count == 2 {
                    // During turn 2, send prompt for turn 3
                    let _ = inbound_tx_clone
                        .send(ControlMessage {
                            ack: 0,
                            body: Some(control_message::Body::Prompt(Prompt {
                                prompt_id: "prompt-3".to_string(),
                                author: None,
                                content: vec![],
                                queued_at: None,
                            })),
                        })
                        .await;
                }
            }
        }
    });

    let git = Git::new(&repo_dir).allow_protocols(["file", "https"]);

    let params = RunDriverParams {
        assign,
        credentials: None,
        initial_prompts,
        repo_dir,
        home_dir,
        runtime_dir,
        hooks_path,
        mcp_command: mcp_bin,
        runtime_sock_path: runtime_sock,
        git,
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
        cancel_requested: false,
    };

    let result = run_session(params)
        .await
        .expect("run_session should succeed");
    assert_eq!(result.outcome, RunOutcome::LimitReached);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::LimitReached as i32
    );
    assert!(result.run_report.detail.contains("Turn budget"));
    cp_task.abort();
}

#[tokio::test]
async fn test_scenario_outcome_wall_clock_budget_exceeded() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{Assign, Budgets, Harness, Prompt, RunOutcome, Workspace};
    use df_proto::pbjson_types::Duration;
    use tokio::sync::mpsc;

    let fake_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/fake-acp-agent");
    let mcp_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/factory-agent");

    if !fake_bin.exists() || !mcp_bin.exists() {
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());

    // Agent hangs until cancelled
    let script_yaml = r#"
sessions:
  new:
    - - end: hang
"#;
    let script_file = temp.path().join("script.yaml");
    std::fs::write(&script_file, script_yaml).unwrap();

    let repo_dir = temp.path().join("repo");
    let home_dir = temp.path().join("home");
    let runtime_dir = temp.path().join("runtime");
    let hooks_path = temp.path().join("hooks-empty");
    let runtime_sock = temp.path().join("runtime.sock");
    std::fs::create_dir_all(&hooks_path).unwrap();

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(
            &runtime_dir,
            "run-wall-clock-limit",
            10 * 1024 * 1024,
            64 * 1024,
        )
        .unwrap(),
    ));

    // Limit wall clock to 200 ms
    let budgets = Budgets {
        max_wall_clock: Some(Duration {
            seconds: 0,
            nanos: 200_000_000,
        }),
        max_output_tokens: None,
        max_turns: None,
    };

    let assign = Assign {
        run_id: "run-wall-clock-limit".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: upstream.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: None,
            git_hooks: Vec::new(),
            git_config: Vec::new(),
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        script_file.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: String::new(),
        }),
        tools: Vec::new(),
        budgets: Some(budgets),
        continuation: None,
    };

    let initial_prompts = vec![Prompt {
        prompt_id: "prompt-1".to_string(),
        author: None,
        content: vec![],
        queued_at: None,
    }];

    let (_inbound_tx, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, mut outbound_rx) = mpsc::channel(100);

    let drain_handle =
        tokio::spawn(async move { while let Some(_msg) = outbound_rx.recv().await {} });

    let git = Git::new(&repo_dir).allow_protocols(["file", "https"]);

    let params = RunDriverParams {
        assign,
        credentials: None,
        initial_prompts,
        repo_dir,
        home_dir,
        runtime_dir,
        hooks_path,
        mcp_command: mcp_bin,
        runtime_sock_path: runtime_sock,
        git,
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
        cancel_requested: false,
    };

    let result = run_session(params)
        .await
        .expect("run_session should succeed");
    assert_eq!(result.outcome, RunOutcome::LimitReached);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::LimitReached as i32
    );
    assert!(result.run_report.detail.contains("Wall clock budget"));
    drain_handle.abort();
}

#[tokio::test]
async fn test_scenario_outcome_cancelled_by_control_plane() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{
        Assign, Cancel, ControlMessage, Harness, Prompt, RunOutcome, Workspace, control_message,
    };
    use tokio::sync::mpsc;

    let fake_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/fake-acp-agent");
    let mcp_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/factory-agent");

    if !fake_bin.exists() || !mcp_bin.exists() {
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());

    // Agent hangs until cancelled
    let script_yaml = r#"
sessions:
  new:
    - - end: hang
"#;
    let script_file = temp.path().join("script.yaml");
    std::fs::write(&script_file, script_yaml).unwrap();

    let repo_dir = temp.path().join("repo");
    let home_dir = temp.path().join("home");
    let runtime_dir = temp.path().join("runtime");
    let hooks_path = temp.path().join("hooks-empty");
    let runtime_sock = temp.path().join("runtime.sock");
    std::fs::create_dir_all(&hooks_path).unwrap();

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&runtime_dir, "run-cancel", 10 * 1024 * 1024, 64 * 1024).unwrap(),
    ));

    let assign = Assign {
        run_id: "run-cancel".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: upstream.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: None,
            git_hooks: Vec::new(),
            git_config: Vec::new(),
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        script_file.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: String::new(),
        }),
        tools: Vec::new(),
        budgets: None,
        continuation: None,
    };

    let initial_prompts = vec![Prompt {
        prompt_id: "prompt-1".to_string(),
        author: None,
        content: vec![],
        queued_at: None,
    }];

    let (inbound_tx, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, mut outbound_rx) = mpsc::channel(100);

    // After 100ms, send Cancel control message
    let inbound_tx_clone = inbound_tx.clone();
    let cp_task = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let _ = inbound_tx_clone
            .send(ControlMessage {
                ack: 0,
                body: Some(control_message::Body::Cancel(Cancel {
                    reason: "User requested abort".to_string(),
                })),
            })
            .await;

        while let Some(_msg) = outbound_rx.recv().await {}
    });

    let git = Git::new(&repo_dir).allow_protocols(["file", "https"]);

    let params = RunDriverParams {
        assign,
        credentials: None,
        initial_prompts,
        repo_dir,
        home_dir,
        runtime_dir,
        hooks_path,
        mcp_command: mcp_bin,
        runtime_sock_path: runtime_sock,
        git,
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
        cancel_requested: false,
    };

    let result = run_session(params)
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
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{Assign, Harness, Prompt, RunOutcome, Workspace};
    use tokio::sync::mpsc;

    let fake_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/fake-acp-agent");
    let mcp_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/factory-agent");

    if !fake_bin.exists() || !mcp_bin.exists() {
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());

    // Agent executes exit(1) crash
    let script_yaml = r#"
sessions:
  new:
    - - end: crash
"#;
    let script_file = temp.path().join("script.yaml");
    std::fs::write(&script_file, script_yaml).unwrap();

    let repo_dir = temp.path().join("repo");
    let home_dir = temp.path().join("home");
    let runtime_dir = temp.path().join("runtime");
    let hooks_path = temp.path().join("hooks-empty");
    let runtime_sock = temp.path().join("runtime.sock");
    std::fs::create_dir_all(&hooks_path).unwrap();

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&runtime_dir, "run-crash", 10 * 1024 * 1024, 64 * 1024).unwrap(),
    ));

    let assign = Assign {
        run_id: "run-crash".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: upstream.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: None,
            git_hooks: Vec::new(),
            git_config: Vec::new(),
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        script_file.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: String::new(),
        }),
        tools: Vec::new(),
        budgets: None,
        continuation: None,
    };

    let initial_prompts = vec![Prompt {
        prompt_id: "prompt-1".to_string(),
        author: None,
        content: vec![],
        queued_at: None,
    }];

    let (_inbound_tx, inbound_rx) = mpsc::channel(100);
    let (outbound_tx, mut outbound_rx) = mpsc::channel(100);

    let drain_handle =
        tokio::spawn(async move { while let Some(_msg) = outbound_rx.recv().await {} });

    let git = Git::new(&repo_dir).allow_protocols(["file", "https"]);

    let params = RunDriverParams {
        assign,
        credentials: None,
        initial_prompts,
        repo_dir,
        home_dir,
        runtime_dir,
        hooks_path,
        mcp_command: mcp_bin,
        runtime_sock_path: runtime_sock,
        git,
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
        cancel_requested: false,
    };

    let result = run_session(params)
        .await
        .expect("run_session should succeed");
    assert_eq!(result.outcome, RunOutcome::Failed);
    assert_eq!(
        result.run_report.outcome,
        df_proto::agent::v1::RunOutcome::Failed as i32
    );
    drain_handle.abort();
}

#[tokio::test]
async fn test_scenario_full_runtime_two_runs_on_single_data_dir() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use crate::test_harness::{LocalHttpsGitServer, TestPki};
    use df_proto::agent::v1::{
        Assign, CommitIdentity, Continuation, ControlMessage, GitConfigEntry, Harness, Prompt,
        PushStatus, PushTarget, RunOutcome, ToolCallResult, ToolDescriptor, Workspace,
        agent_message, control_message,
    };
    use tokio::sync::mpsc;

    let fake_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/fake-acp-agent");
    let mcp_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/factory-agent");

    if !fake_bin.exists() || !mcp_bin.exists() {
        eprintln!("Skipping test: binaries not present");
        return;
    }

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let git_root = temp_dir.path().join("git_root");
    std::fs::create_dir_all(&git_root).expect("create git_root");

    // 1. Initialize bare git repository in git_root/repo.git with main branch
    let upstream_repo = git_root.join("repo.git");
    let out = tokio::process::Command::new("git")
        .args(["init", "--bare", upstream_repo.to_str().unwrap()])
        .output()
        .await
        .expect("git init bare");
    assert!(out.status.success());

    let _ = tokio::process::Command::new("git")
        .current_dir(&upstream_repo)
        .args(["config", "http.receivepack", "true"])
        .output()
        .await;

    // Seed initial commit into bare repo
    let seed_dir = temp_dir.path().join("seed");
    let out = tokio::process::Command::new("git")
        .args([
            "clone",
            upstream_repo.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .expect("clone seed");
    assert!(out.status.success());

    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed User"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await;
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "main"])
        .output()
        .await;

    std::fs::write(seed_dir.join("README.md"), "# Project Root\n").expect("write README");
    let _ = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "README.md"])
        .output()
        .await;
    let out = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "--no-gpg-sign", "-m", "Initial commit"])
        .output()
        .await
        .expect("seed commit");
    assert!(out.status.success());
    let out = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "main"])
        .output()
        .await
        .expect("seed push");
    assert!(out.status.success());

    // 2. Start LocalHttpsGitServer with required Basic auth
    let pki = TestPki::generate("localhost").expect("pki");
    let git_user = "agent-bot".to_string();
    let git_token = "agent-secret-pass-42".to_string();
    let git_server = LocalHttpsGitServer::start(
        &pki,
        git_root.clone(),
        Some((git_user.clone(), git_token.clone())),
    )
    .expect("start https git server");

    let ca_path = temp_dir.path().join("ca.crt");
    std::fs::write(&ca_path, &pki.ca_cert_pem).expect("write ca.crt");

    // 3. Shared sandbox directory structure
    let data_dir = temp_dir.path().join("data");
    let repo_dir = data_dir.join("repo");
    let home_dir = data_dir.join("home");
    let runtime_dir = data_dir.join("runtime");
    let hooks_path = temp_dir.path().join("hooks-empty");
    let runtime_sock = temp_dir.path().join("runtime.sock");
    std::fs::create_dir_all(&hooks_path).expect("create hooks");

    let repo_url = format!("https://localhost:{}/repo.git", git_server.addr.port());

    // --- RUN 1: First Run ---
    let script1_yaml = r#"
sessions:
  new:
    - - say: "Run 1 started, modifying repository"
      - shell: "echo 'Run 1 content' >> feature1.txt && git add feature1.txt && git config user.name 'Agent Bot' && git config user.email 'bot@example.com' && git config commit.gpgsign false && git commit --no-gpg-sign -m 'Commit from run 1'"
      - say: "Finished turn 1"
      - end: end_turn
"#;
    let script1_file = temp_dir.path().join("script1.yaml");
    std::fs::write(&script1_file, script1_yaml).expect("write script1");

    let outbox1 = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&runtime_dir, "run-e2e-1", 10 * 1024 * 1024, 64 * 1024)
            .expect("open outbox 1"),
    ));

    let assign1 = Assign {
        run_id: "run-e2e-1".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: repo_url.clone(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: Some(CommitIdentity {
                name: "Agent Bot".to_string(),
                email: "bot@example.com".to_string(),
            }),
            git_hooks: Vec::new(),
            git_config: vec![GitConfigEntry {
                key: "http.sslCAInfo".to_string(),
                value: ca_path.to_str().unwrap().to_string(),
            }],
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        script1_file.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: "You are the Dark Factory agent".to_string(),
        }),
        tools: Vec::new(),
        budgets: None,
        continuation: None,
    };

    let initial_prompts1 = vec![Prompt {
        prompt_id: "prompt-1".to_string(),
        author: None,
        content: vec![df_proto::transcript::v1::ContentBlock {
            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                df_proto::transcript::v1::TextContent {
                    text: "Start run 1 work".to_string(),
                },
            )),
        }],
        queued_at: None,
    }];

    let (_inbound_tx1, inbound_rx1) = mpsc::channel(100);
    let (outbound_tx1, mut outbound_rx1) = mpsc::channel::<df_proto::agent::v1::AgentMessage>(100);

    let run1_messages = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let run1_messages_clone = std::sync::Arc::clone(&run1_messages);
    let drain_handle1 = tokio::spawn(async move {
        while let Some(msg) = outbound_rx1.recv().await {
            run1_messages_clone.lock().await.push(msg);
        }
    });

    let git1 = Git::new(&repo_dir)
        .allow_protocols(["file", "https"])
        .with_hooks_empty_dir(&hooks_path)
        .with_ssl_ca_info(&ca_path);

    let params1 = RunDriverParams {
        assign: assign1,
        credentials: None,
        initial_prompts: initial_prompts1,
        repo_dir: repo_dir.clone(),
        home_dir: home_dir.clone(),
        runtime_dir: runtime_dir.clone(),
        hooks_path: hooks_path.clone(),
        mcp_command: mcp_bin.clone(),
        runtime_sock_path: runtime_sock.clone(),
        git: git1,
        outbox: outbox1,
        inbound_rx: inbound_rx1,
        outbound_tx: outbound_tx1,
        git_credentials: Some((git_user.clone(), git_token.clone())),
        cancel_requested: false,
    };

    let result1 = run_session(params1).await.expect("run 1 should succeed");
    drain_handle1.abort();

    // Verify Run 1 outcome and reports
    assert_eq!(result1.outcome, RunOutcome::EndTurn);
    assert_eq!(
        result1.run_report.outcome,
        df_proto::agent::v1::RunOutcome::EndTurn as i32
    );
    assert!(!result1.run_report.harness_session_id.is_empty());
    let session_id_1 = result1.run_report.harness_session_id.clone();

    // Verify Run 1 pushed task branch to remote
    let pushed_task_report = result1.push_reports.iter().find(|pr| {
        pr.target == PushTarget::TaskBranch as i32 && pr.status == PushStatus::Ok as i32
    });
    assert!(
        pushed_task_report.is_some(),
        "Run 1 must have pushed task branch"
    );

    // Verify commit in local repo
    assert!(repo_dir.join("feature1.txt").exists());

    // Verify commit in remote upstream repo
    let verify_out = tokio::process::Command::new("git")
        .current_dir(&upstream_repo)
        .args(["log", "--oneline", "factory/task/DF-1"])
        .output()
        .await
        .expect("git log remote");
    let log_str = String::from_utf8_lossy(&verify_out.stdout);
    assert!(
        log_str.contains("Commit from run 1"),
        "Remote repo must contain commit from run 1: {log_str}"
    );

    // --- RUN 2: Second Run on SAME /data Directory ---
    let script2_yaml = r#"
sessions:
  loaded:
    - - say: "Run 2 resumed previous session, finishing task"
      - shell: "echo 'Run 2 content' >> feature2.txt && git add feature2.txt && git config user.name 'Agent Bot' && git config user.email 'bot@example.com' && git config commit.gpgsign false && git commit --no-gpg-sign -m 'Commit from run 2'"
      - mcp:
          server: factory
          tool: complete_task
          args:
            summary: "Completed feature 1 and feature 2"
      - say: "Finished work"
      - end: end_turn
"#;
    let script2_file = temp_dir.path().join("script2.yaml");
    std::fs::write(&script2_file, script2_yaml).expect("write script2");

    let outbox2 = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&runtime_dir, "run-e2e-2", 10 * 1024 * 1024, 64 * 1024)
            .expect("open outbox 2"),
    ));

    let assign2 = Assign {
        run_id: "run-e2e-2".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: repo_url.clone(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: Some(CommitIdentity {
                name: "Agent Bot".to_string(),
                email: "bot@example.com".to_string(),
            }),
            git_hooks: Vec::new(),
            git_config: vec![GitConfigEntry {
                key: "http.sslCAInfo".to_string(),
                value: ca_path.to_str().unwrap().to_string(),
            }],
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        script2_file.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: "You are the Dark Factory agent".to_string(),
        }),
        tools: vec![ToolDescriptor {
            name: "complete_task".to_string(),
            title: "Complete Task".to_string(),
            description: "Mark task complete".to_string(),
            input_schema_json: "{}".to_string(),
            output_schema_json: None,
            annotations: None,
        }],
        budgets: None,
        continuation: Some(Continuation {
            harness_session_id: session_id_1.clone(),
            summary: "Run 1 added feature1.txt".to_string(),
        }),
    };

    let initial_prompts2 = vec![Prompt {
        prompt_id: "prompt-2".to_string(),
        author: None,
        content: vec![df_proto::transcript::v1::ContentBlock {
            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                df_proto::transcript::v1::TextContent {
                    text: "Continue and complete work".to_string(),
                },
            )),
        }],
        queued_at: None,
    }];

    let (inbound_tx2, inbound_rx2) = mpsc::channel(100);
    let (outbound_tx2, mut outbound_rx2) = mpsc::channel::<df_proto::agent::v1::AgentMessage>(100);

    let run2_messages = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let run2_messages_clone = std::sync::Arc::clone(&run2_messages);
    let inbound_tx2_clone = inbound_tx2.clone();

    let cp_task2 = tokio::spawn(async move {
        while let Some(msg) = outbound_rx2.recv().await {
            run2_messages_clone.lock().await.push(msg.clone());
            if let Some(agent_message::Body::ToolCallRequest(req)) = msg.body
                && req.tool_name == "complete_task"
            {
                let res = ControlMessage {
                    ack: 0,
                    body: Some(control_message::Body::ToolCallResult(ToolCallResult {
                        call_id: req.call_id,
                        is_error: false,
                        content: vec![df_proto::transcript::v1::ContentBlock {
                            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                                df_proto::transcript::v1::TextContent {
                                    text: "Task completed successfully".to_string(),
                                },
                            )),
                        }],
                        structured_content_json: String::new(),
                        end_run_after_turn: true,
                    })),
                };
                let _ = inbound_tx2_clone.send(res).await;
            }
        }
    });

    let git2 = Git::new(&repo_dir)
        .allow_protocols(["file", "https"])
        .with_hooks_empty_dir(&hooks_path)
        .with_ssl_ca_info(&ca_path);

    let params2 = RunDriverParams {
        assign: assign2,
        credentials: None,
        initial_prompts: initial_prompts2,
        repo_dir: repo_dir.clone(),
        home_dir: home_dir.clone(),
        runtime_dir: runtime_dir.clone(),
        hooks_path: hooks_path.clone(),
        mcp_command: mcp_bin.clone(),
        runtime_sock_path: runtime_sock.clone(),
        git: git2,
        outbox: outbox2,
        inbound_rx: inbound_rx2,
        outbound_tx: outbound_tx2,
        git_credentials: Some((git_user, git_token)),
        cancel_requested: false,
    };

    let result2 = run_session(params2).await.expect("run 2 should succeed");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    cp_task2.abort();

    // Verify Run 2 outcome and reports
    assert_eq!(result2.outcome, RunOutcome::Completed);
    assert_eq!(
        result2.run_report.outcome,
        df_proto::agent::v1::RunOutcome::Completed as i32
    );
    assert_eq!(
        result2.run_report.harness_session_id, session_id_1,
        "Run 2 must load session from run 1"
    );

    // Verify Run 2 pushed new commits to remote
    let pushed_task_report2 = result2.push_reports.iter().find(|pr| {
        pr.target == PushTarget::TaskBranch as i32 && pr.status == PushStatus::Ok as i32
    });
    assert!(
        pushed_task_report2.is_some(),
        "Run 2 must have pushed task branch"
    );

    // Verify both files exist in local repo
    assert!(repo_dir.join("feature1.txt").exists());
    assert!(repo_dir.join("feature2.txt").exists());

    // Verify remote repository has BOTH commits from run 1 and run 2
    let verify_out2 = tokio::process::Command::new("git")
        .current_dir(&upstream_repo)
        .args(["log", "--oneline", "factory/task/DF-1"])
        .output()
        .await
        .expect("git log remote");
    let log_str2 = String::from_utf8_lossy(&verify_out2.stdout);
    assert!(
        log_str2.contains("Commit from run 1"),
        "Remote repo must contain commit 1: {log_str2}"
    );
    assert!(
        log_str2.contains("Commit from run 2"),
        "Remote repo must contain commit 2: {log_str2}"
    );

    // Verify transcript events in Run 2 messages
    let msgs2 = run2_messages.lock().await;
    let has_tool_call = msgs2.iter().any(|m| {
        matches!(&m.body, Some(agent_message::Body::TranscriptEvent(ev))
            if matches!(&ev.kind, Some(df_proto::transcript::v1::event::Kind::ToolCall(_))))
    });
    assert!(
        has_tool_call,
        "Run 2 transcript events must contain tool call"
    );

    let has_turn_ended = msgs2.iter().any(|m| {
        matches!(&m.body, Some(agent_message::Body::TranscriptEvent(ev))
            if matches!(&ev.kind, Some(df_proto::transcript::v1::event::Kind::TurnEnded(_))))
    });
    assert!(
        has_turn_ended,
        "Run 2 transcript events must contain turn ended"
    );

    let has_run_report = msgs2
        .iter()
        .any(|m| matches!(&m.body, Some(agent_message::Body::RunReport(_))));
    assert!(has_run_report, "Run 2 must emit RunReport");
}

#[tokio::test]
#[ignore = "Manual test requiring CLAUDE_CODE_OAUTH_TOKEN and network access"]
async fn test_manual_claude_acp_run() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{
        Assign, Harness, HarnessCredentials, Prompt, RunOutcome, ToolCallResult, ToolDescriptor,
        Workspace, control_message,
    };
    use tokio::sync::mpsc;

    let token = match std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => {
            eprintln!("Skipping manual Claude ACP test: CLAUDE_CODE_OAUTH_TOKEN not set");
            return;
        }
    };

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());

    let repo_dir = temp.path().join("repo");
    let home_dir = temp.path().join("home");
    let runtime_dir = temp.path().join("runtime");
    let hooks_path = temp.path().join("hooks-empty");
    let runtime_sock = temp.path().join("runtime.sock");
    std::fs::create_dir_all(&hooks_path).unwrap();

    let mcp_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/factory-agent");

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(
            &runtime_dir,
            "run-claude-manual",
            10 * 1024 * 1024,
            64 * 1024,
        )
        .unwrap(),
    ));

    let assign = Assign {
        run_id: "run-claude-manual".to_string(),
        task: None,
        workspace: Some(Workspace {
            repo_url: upstream.to_str().unwrap().to_string(),
            base_branch: "main".to_string(),
            task_branch: "factory/task/DF-1".to_string(),
            wip_branch: "factory/wip/DF-1".to_string(),
            commit_identity: None,
            git_hooks: Vec::new(),
            git_config: Vec::new(),
        }),
        harness: Some(Harness {
            kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                df_proto::agent::v1::AcpHarness {
                    command: vec![
                        "npx".to_string(),
                        "-y".to_string(),
                        "@agentclientprotocol/claude-agent-acp".to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                },
            )),
            system_prompt: "You are an automated pair programmer in Dark Factory.".to_string(),
        }),
        tools: vec![ToolDescriptor {
            name: "complete_task".to_string(),
            title: "Complete Task".to_string(),
            description: "Mark task complete".to_string(),
            input_schema_json: "{}".to_string(),
            output_schema_json: None,
            annotations: None,
        }],
        budgets: None,
        continuation: None,
    };

    let mut creds_env = std::collections::HashMap::new();
    creds_env.insert("CLAUDE_CODE_OAUTH_TOKEN".to_string(), token);
    let credentials = HarnessCredentials { env: creds_env };

    let initial_prompts = vec![Prompt {
        prompt_id: "prompt-1".to_string(),
        author: None,
        content: vec![df_proto::transcript::v1::ContentBlock {
            kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                df_proto::transcript::v1::TextContent {
                    text: "Read README.md, append '# Claude Edit' and call complete_task."
                        .to_string(),
                },
            )),
        }],
        queued_at: None,
    }];

    let (inbound_tx, inbound_rx) = mpsc::channel::<df_proto::agent::v1::ControlMessage>(100);
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<df_proto::agent::v1::AgentMessage>(100);

    let inbound_tx_clone = inbound_tx.clone();
    let cp_task = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            if let Some(df_proto::agent::v1::agent_message::Body::ToolCallRequest(req)) = msg.body
                && req.tool_name == "complete_task"
            {
                let res = df_proto::agent::v1::ControlMessage {
                    ack: 0,
                    body: Some(control_message::Body::ToolCallResult(ToolCallResult {
                        call_id: req.call_id,
                        is_error: false,
                        content: vec![],
                        structured_content_json: String::new(),
                        end_run_after_turn: true,
                    })),
                };
                let _ = inbound_tx_clone.send(res).await;
            }
        }
    });

    let git = Git::new(&repo_dir).allow_protocols(["file", "https"]);

    let params = RunDriverParams {
        assign,
        credentials: Some(credentials),
        initial_prompts,
        repo_dir,
        home_dir,
        runtime_dir,
        hooks_path,
        mcp_command: mcp_bin,
        runtime_sock_path: runtime_sock,
        git,
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
        cancel_requested: false,
    };

    let result = run_session(params)
        .await
        .expect("manual run should succeed");
    assert_eq!(result.outcome, RunOutcome::Completed);
    cp_task.abort();
}

#[test]
fn test_finding_4_git_credential_scoping() {
    use crate::runner::matches_repo_url;

    let repo = "https://github.com/prskr/dark-factory.git";
    assert!(matches_repo_url(
        repo,
        "https",
        "github.com",
        "prskr/dark-factory.git"
    ));
    assert!(matches_repo_url(
        repo,
        "https",
        "github.com",
        "prskr/dark-factory"
    ));
    assert!(
        !matches_repo_url(repo, "https", "github.com", ""),
        "path is required"
    );

    // Attacker host must be denied
    assert!(!matches_repo_url(
        repo,
        "https",
        "attacker.example",
        "prskr/dark-factory.git"
    ));
    assert!(!matches_repo_url(
        repo,
        "http",
        "github.com",
        "prskr/dark-factory.git"
    ));
    assert!(!matches_repo_url(
        repo,
        "https",
        "github.com",
        "other-org/other-repo"
    ));

    // Local file URL
    let file_repo = "file:///data/local-repo.git";
    assert!(matches_repo_url(
        file_repo,
        "file",
        "",
        "/data/local-repo.git"
    ));
    assert!(matches_repo_url(file_repo, "file", "", "data/local-repo"));
    assert!(!matches_repo_url(
        file_repo,
        "file",
        "",
        "data/different-repo"
    ));
}

#[test]
fn test_finding_6_is_secret_file_ssh_keys_and_ordinary_files() {
    use crate::workspace::is_secret_file;

    // Ordinary source files must NOT be detected as secrets
    assert!(!is_secret_file("id_utils.rs"));
    assert!(!is_secret_file("src/id_generator.rs"));
    assert!(!is_secret_file("tests/id_test.rs"));

    // SSH private keys must be detected
    assert!(is_secret_file("id_rsa"));
    assert!(is_secret_file("id_ed25519"));
    assert!(is_secret_file("id_ecdsa"));
    assert!(is_secret_file("id_dsa"));
    assert!(is_secret_file("sub/id_rsa.bak"));
}

#[tokio::test]
async fn test_finding_13_compute_dir_size_skips_symlinks() {
    use crate::workspace::compute_dir_size;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    std::fs::write(root.join("file1.txt"), vec![b'a'; 100]).unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub").join("file2.txt"), vec![b'b'; 200]).unwrap();

    // Create a circular symlink pointing back to root
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root, root.join("sub").join("loop_link")).unwrap();
        std::os::unix::fs::symlink(root.join("file1.txt"), root.join("link_file.txt")).unwrap();
    }

    let size = compute_dir_size(root);
    // Size should only include actual files (100 + 200 = 300), and NOT follow circular symlink
    assert_eq!(size, 300);
}

#[test]
fn test_finding_16_tool_origin_detection() {
    use crate::transcript::TranscriptTranslator;
    use agent_client_protocol::schema::v1::{ToolCall as AcpToolCall, ToolCallId};
    use df_proto::transcript::v1::ToolOrigin;

    let mut translator = TranscriptTranslator::new("turn_1");

    // Tool call with "factory" in title but ordinary tool name must be Harness origin
    let mut call1 = AcpToolCall::new(
        ToolCallId::new("call1"),
        "Read crates/factory-agent/src/lib.rs",
    );
    call1.name = Some("read_file".to_string());
    let ev1 = translator.translate_tool_call(&call1);
    match ev1.kind {
        Some(df_proto::transcript::v1::event::Kind::ToolCall(tc)) => {
            assert_eq!(tc.origin, ToolOrigin::Harness as i32);
        }
        _ => panic!("Expected ToolCall event"),
    }

    // Factory tool must start with mcp__factory__
    let mut call2 = AcpToolCall::new(ToolCallId::new("call2"), "mcp__factory__complete_task");
    call2.name = Some("mcp__factory__complete_task".to_string());
    let ev2 = translator.translate_tool_call(&call2);
    match ev2.kind {
        Some(df_proto::transcript::v1::event::Kind::ToolCall(tc)) => {
            assert_eq!(tc.origin, ToolOrigin::Factory as i32);
        }
        _ => panic!("Expected ToolCall event"),
    }
}

#[test]
fn test_finding_18_render_git_config_rejects_injection() {
    use crate::git::{GitConfigParams, render_git_config};
    use df_proto::agent::v1::GitConfigEntry;
    use std::path::Path;

    // Newlines in value must be skipped/filtered
    let entries = vec![GitConfigEntry {
        key: "user.name".to_string(),
        value: "evil\n[url \"https://evil/\"]\ninsteadOf = https://github.com/".to_string(),
    }];
    let params = GitConfigParams {
        hooks_path: Path::new("/etc/factory/hooks"),
        extra_config: &entries,
        repo_url: "https://github.com/prskr/dark-factory.git",
        credential_helper: "factory-agent git-credential",
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };
    let rendered = render_git_config(&params);
    assert!(!rendered.contains("https://evil/"));

    // Newlines in key must be skipped/filtered
    let entries2 = vec![GitConfigEntry {
        key: "user.name\nfake".to_string(),
        value: "test".to_string(),
    }];
    let params2 = GitConfigParams {
        hooks_path: Path::new("/etc/factory/hooks"),
        extra_config: &entries2,
        repo_url: "https://github.com/prskr/dark-factory.git",
        credential_helper: "factory-agent git-credential",
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };
    let rendered2 = render_git_config(&params2);
    assert!(!rendered2.contains("fake"));

    // Brackets in section name must be skipped/filtered
    let entries3 = vec![GitConfigEntry {
        key: "bad]section.key".to_string(),
        value: "test".to_string(),
    }];
    let params3 = GitConfigParams {
        hooks_path: Path::new("/etc/factory/hooks"),
        extra_config: &entries3,
        repo_url: "https://github.com/prskr/dark-factory.git",
        credential_helper: "factory-agent git-credential",
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };
    let rendered3 = render_git_config(&params3);
    assert!(!rendered3.contains("bad]section"));
}

#[test]
fn test_finding_20_parse_rate_limit_precise_429() {
    use crate::harness::parse_rate_limit;
    use agent_client_protocol::schema::v1::Error as AcpError;

    // Line 1429 error must NOT be parsed as rate limit
    let err_line = AcpError::internal_error();
    let mut err1 = err_line;
    err1.message = "Syntax error at line 1429: unexpected token".to_string();
    assert!(parse_rate_limit(&err1).is_none());

    // Request ID containing 429 must NOT be parsed as rate limit
    let mut err2 = AcpError::internal_error();
    err2.message = "Failed to fetch resource for request-42981-xyz".to_string();
    assert!(parse_rate_limit(&err2).is_none());

    // Real HTTP 429 must be parsed as rate limit
    let mut err3 = AcpError::internal_error();
    err3.message = "API returned HTTP 429 Too Many Requests. retry-after: 60".to_string();
    let parsed = parse_rate_limit(&err3);
    assert!(parsed.is_some());
    assert_eq!(parsed.unwrap().retry_after_seconds, Some(60));
}

#[test]
fn test_finding_21_parse_session_log_deduplication() {
    use crate::usage::parse_session_log;

    // Multiple JSONL lines sharing the same message id should only count usage once
    let jsonl = r#"
{"type":"assistant","message":{"id":"msg_1","model":"claude-3-7-sonnet","role":"assistant","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":0}}}
{"type":"assistant","message":{"id":"msg_1","model":"claude-3-7-sonnet","role":"assistant","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":0}}}
{"type":"assistant","message":{"id":"msg_2","model":"claude-3-7-sonnet","role":"assistant","usage":{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":5}}}
"#;

    let usages = parse_session_log(jsonl.as_bytes()).unwrap();
    assert_eq!(usages.len(), 1);
    let u = &usages[0];
    assert_eq!(u.model_id, "claude-3-7-sonnet");
    // input_tokens should be 100 + 200 = 300 (msg_1 counted only once, not twice!)
    assert_eq!(u.input_tokens, 300);
    // output_tokens should be 50 + 30 = 80
    assert_eq!(u.output_tokens, 80);
}

#[test]
fn test_finding_25_save_run_state_atomic() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    let mut outbox = Outbox::open(temp.path(), "run-atomic-1", 1024 * 1024, 64 * 1024).unwrap();
    outbox
        .set_last_pushed_sha(Some("abcdef123456".to_string()))
        .unwrap();

    let run_json_path = temp.path().join("run.json");
    assert!(run_json_path.exists());
    let tmp_path = temp.path().join("run.tmp");
    assert!(
        !tmp_path.exists(),
        "temp file should be renamed over target"
    );

    let content = std::fs::read_to_string(&run_json_path).unwrap();
    assert!(content.contains("abcdef123456"));
}

// --- Regression tests for review_result.md findings (R1-R36) ---

#[test]
fn test_review_r8_r9_config_key_normalization() {
    use crate::git::is_config_entry_denied;

    // Whitespace or `=` in a key must not hide a denied key (R8)
    assert!(is_config_entry_denied("core .hooksPath", "/x"));
    assert!(is_config_entry_denied("core.hooksPath=/x", ""));
    assert!(is_config_entry_denied("remote .origin.url", "https://evil"));
    assert!(is_config_entry_denied(" core.hooksPath ", "/x"));
    assert!(is_config_entry_denied("core.hooks Path", "/x"));

    // Keys that run commands (R9)
    assert!(is_config_entry_denied(
        "interactive.diffFilter",
        "sh -c evil"
    ));
    assert!(is_config_entry_denied("mergetool.x.cmd", "evil"));
    assert!(is_config_entry_denied("difftool.x.cmd", "evil"));
    assert!(is_config_entry_denied("core.alternateRefsCommand", "evil"));
    assert!(is_config_entry_denied("browser.x.cmd", "evil"));

    assert!(!is_config_entry_denied("rebase.autoStash", "true"));
}

#[test]
fn test_review_r8_r24_render_skips_unsafe_keys_and_quotes_values() {
    use crate::git::{GitConfigParams, render_git_config};
    use df_proto::agent::v1::GitConfigEntry;

    let entries = vec![
        GitConfigEntry {
            key: "core .hooksPath".to_string(),
            value: "/data/repo/.evil".to_string(),
        },
        GitConfigEntry {
            key: "pull.rebase".to_string(),
            value: "a#b\"c\\d".to_string(),
        },
    ];
    let params = GitConfigParams {
        hooks_path: std::path::Path::new("/etc/factory/hooks"),
        extra_config: &entries,
        repo_url: "https://github.com/prskr/dark-factory.git",
        credential_helper: "factory-agent git-credential",
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };
    let rendered = render_git_config(&params);
    assert!(!rendered.contains(".evil"));
    assert!(rendered.contains(r#"rebase = "a#b\"c\\d""#));
    assert!(rendered.contains("useHttpPath = true"));
}

#[test]
fn test_review_r23_rewrite_git_config_requires_real_git_dir() {
    use crate::git::{GitConfigParams, rewrite_git_config};

    let temp = tempfile::tempdir().unwrap();
    let params = GitConfigParams {
        hooks_path: std::path::Path::new("/etc/factory/hooks"),
        extra_config: &[],
        repo_url: "https://github.com/prskr/dark-factory.git",
        credential_helper: "",
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };

    // Missing .git
    let missing = temp.path().join("missing");
    std::fs::create_dir_all(&missing).unwrap();
    assert!(rewrite_git_config(&missing, &params).is_err());
    assert!(!missing.join("config").exists());

    // .git is a `gitdir:` file pointing somewhere else
    let redirected = temp.path().join("redirected");
    let elsewhere = temp.path().join("elsewhere");
    std::fs::create_dir_all(&redirected).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(
        redirected.join(".git"),
        format!("gitdir: {}", elsewhere.display()),
    )
    .unwrap();
    assert!(rewrite_git_config(&redirected, &params).is_err());
    assert!(!elsewhere.join("config").exists());

    // .git is a symlink
    let linked = temp.path().join("linked");
    std::fs::create_dir_all(&linked).unwrap();
    std::os::unix::fs::symlink(&elsewhere, linked.join(".git")).unwrap();
    assert!(rewrite_git_config(&linked, &params).is_err());
    assert!(!elsewhere.join("config").exists());
}

#[test]
fn test_review_r10_credential_scope_requires_matching_path() {
    use crate::runner::matches_repo_url;

    let repo = "https://github.com/org/a.git";
    assert!(matches_repo_url(repo, "https", "github.com", "org/a"));
    assert!(!matches_repo_url(repo, "https", "github.com", "attacker/b"));
    assert!(!matches_repo_url(repo, "https", "github.com", ""));

    // A local-path repo only matches the file protocol
    assert!(!matches_repo_url(
        "/srv/repo.git",
        "https",
        "evil.example",
        "srv/repo"
    ));
    assert!(matches_repo_url(
        "/srv/repo.git",
        "file",
        "",
        "/srv/repo.git"
    ));
}

#[test]
fn test_review_r1_open_without_run_id_keeps_state() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    {
        let mut ob = Outbox::open(temp.path(), "run-a", 1024 * 1024, 64 * 1024).unwrap();
        ob.set_last_pushed_sha(Some("abc".to_string())).unwrap();
        ob.append(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                    detail: "kept".to_string(),
                },
            )),
        })
        .unwrap();
    }

    let ob = Outbox::open(temp.path(), "", 1024 * 1024, 64 * 1024).unwrap();
    assert_eq!(ob.run_state().run_id, "run-a");
    assert_eq!(ob.run_state().last_pushed_sha.as_deref(), Some("abc"));
    assert_eq!(ob.run_state().next_seq, 2);
    assert_eq!(ob.read_unacknowledged(0).unwrap().len(), 1);
}

#[test]
fn test_review_r29_next_seq_survives_restart_after_ack() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    {
        let mut ob = Outbox::open(temp.path(), "run-a", 1024 * 1024, 64 * 1024).unwrap();
        for _ in 0..3 {
            ob.append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                        detail: String::new(),
                    },
                )),
            })
            .unwrap();
        }
        ob.acknowledge(3).unwrap();
        assert!(ob.is_empty());
    }

    // All segments are gone; the next seq must still not restart at 1
    let ob = Outbox::open(temp.path(), "", 1024 * 1024, 64 * 1024).unwrap();
    assert_eq!(ob.run_state().next_seq, 4);
}

#[test]
fn test_review_r26_corrupt_run_json_is_repaired() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("run.json"), "{not json").unwrap();
    let ob = Outbox::open(temp.path(), "run-x", 1024 * 1024, 64 * 1024).unwrap();
    assert_eq!(ob.run_state().run_id, "run-x");
    let stored = std::fs::read_to_string(temp.path().join("run.json")).unwrap();
    assert!(serde_json::from_str::<crate::outbox::RunState>(&stored).is_ok());
}

#[tokio::test]
async fn test_review_r15_oversized_message_does_not_block_empty_outbox() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    let ob = Outbox::open(temp.path(), "run-a", 10, 64 * 1024).unwrap();
    let ob = std::sync::Arc::new(tokio::sync::Mutex::new(ob));
    let msg = AgentMessage {
        seq: 0,
        body: Some(agent_message::Body::PhaseChanged(
            df_proto::agent::v1::PhaseChanged {
                phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                detail: "larger than the whole outbox".to_string(),
            },
        )),
    };
    let seq = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        Outbox::append_with_backpressure(&ob, msg),
    )
    .await
    .expect("must not wait forever")
    .unwrap();
    assert_eq!(seq, 1);
}

#[test]
fn test_review_r21_partial_session_log_line_is_read_later() {
    use crate::usage::{SessionLogTracker, cwd_to_slug};
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let cwd = std::path::Path::new("/data/repo");
    let dir = home.join(".claude").join("projects").join(cwd_to_slug(cwd));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("sess.jsonl");
    std::fs::write(&file, "").unwrap();

    let mut tracker = SessionLogTracker::new(&home, cwd, "sess");
    tracker.mark_turn_start();

    let line1 = r#"{"type":"assistant","message":{"id":"m1","model":"m","role":"assistant","usage":{"output_tokens":50}}}"#;
    let line2 = r#"{"type":"assistant","message":{"id":"m2","model":"m","role":"assistant","usage":{"output_tokens":30}}}"#;
    let (first, rest) = line2.split_at(40);

    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap();
    write!(f, "{line1}\n{first}").unwrap();
    let (usage, _) = tracker.parse_turn_usage();
    assert_eq!(usage.iter().map(|u| u.output_tokens).sum::<u64>(), 50);

    writeln!(f, "{rest}").unwrap();
    let (usage, _) = tracker.parse_turn_usage();
    assert_eq!(usage.iter().map(|u| u.output_tokens).sum::<u64>(), 30);
}

#[test]
fn test_review_r22_empty_prompt_ids_are_not_duplicates() {
    use crate::connection::CommandFilter;
    use crate::harness::PromptQueue;
    use df_proto::agent::v1::{ControlMessage, Prompt};

    let mut queue = PromptQueue::new();
    queue.push(Prompt::default());
    queue.push(Prompt::default());
    assert_eq!(queue.len(), 2);

    let msg = ControlMessage {
        ack: 0,
        body: Some(control_message::Body::Prompt(Prompt::default())),
    };
    let mut filter = CommandFilter::new();
    assert!(filter.filter_control_message(&msg));
    assert!(filter.filter_control_message(&msg));
}

#[test]
fn test_review_r16_expired_credential_requests_are_not_resent() {
    use crate::connection::{CREDENTIAL_REQUEST_TIMEOUT, CommandFilter};
    use df_proto::agent::v1::CredentialRequest;

    let mut filter = CommandFilter::new();
    let old = std::time::Instant::now()
        .checked_sub(CREDENTIAL_REQUEST_TIMEOUT + std::time::Duration::from_secs(1))
        .expect("instant arithmetic");
    for (id, at) in [("old", old), ("new", std::time::Instant::now())] {
        filter.pending_credentials.insert(
            id.to_string(),
            (
                at,
                CredentialRequest {
                    request_id: id.to_string(),
                    kind: None,
                },
            ),
        );
    }
    let resend = filter.pending_requests_to_resend();
    assert_eq!(resend.len(), 1);
    assert!(!filter.pending_credentials.contains_key("old"));
}

#[test]
fn test_review_r25_rate_limit_text_is_anchored() {
    use crate::harness::parse_rate_limit;
    use agent_client_protocol::schema::v1::Error as AcpError;

    let mut err = AcpError::internal_error();
    err.message = "tool failed: API rate limit doc not found".to_string();
    assert!(parse_rate_limit(&err).is_none());

    err.message = "Rate limit exceeded. Retry after 30 seconds.".to_string();
    assert_eq!(
        parse_rate_limit(&err).unwrap().retry_after_seconds,
        Some(30)
    );
}

/// Clones `upstream` into `dir` with a committer identity, for workspace tests.
fn clone_for_workspace_test(upstream: &std::path::Path, dir: &std::path::Path) {
    let run = |args: &[&str], cwd: Option<&std::path::Path>| {
        let mut cmd = std::process::Command::new("git");
        cmd.args(args);
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }
        assert!(cmd.output().expect("git").status.success(), "git {args:?}");
    };
    run(
        &["clone", upstream.to_str().unwrap(), dir.to_str().unwrap()],
        None,
    );
    run(&["config", "user.name", "Bot"], Some(dir));
    run(&["config", "user.email", "bot@example.com"], Some(dir));
    run(&["config", "commit.gpgsign", "false"], Some(dir));
}

#[tokio::test]
async fn test_review_r3_wip_snapshot_keeps_tracked_secret_file_edits() {
    use crate::git::Git;
    use crate::workspace::{FinalizeParams, finalize_workspace};
    use df_proto::agent::v1::{RunOutcome, Workspace};

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());
    let local = temp.path().join("local");
    let runtime_dir = temp.path().join("runtime");
    let hooks = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::create_dir_all(&hooks).unwrap();
    clone_for_workspace_test(&upstream, &local);

    let git = Git::new(&local)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&hooks);
    std::fs::write(local.join(".env.example"), "KEY=\n").unwrap();
    git.git_output(&["add", ".env.example"]).await.unwrap();
    git.git_output(&["commit", "-m", "add env example"])
        .await
        .unwrap();

    // The agent edits the tracked secret-named file without committing
    std::fs::write(local.join(".env.example"), "KEY=edited\n").unwrap();
    std::fs::write(local.join("notes.txt"), "uncommitted\n").unwrap();
    let status_before = git.git_status().await.unwrap();

    let workspace = Workspace {
        repo_url: format!("file://{}", upstream.display()),
        base_branch: "main".to_string(),
        task_branch: "main".to_string(),
        wip_branch: "factory/wip/DF-3".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };
    let mut last_pushed_sha = Some(git.current_head().await.unwrap());
    let params = FinalizeParams {
        repo_dir: &local,
        runtime_dir: &runtime_dir,
        workspace: &workspace,
        run_id: "run-r3",
        outcome: RunOutcome::Completed,
        detail: "",
        harness_session_id: "",
        hooks_path: &hooks,
        credential_helper: "",
        wip_size_cap_bytes: 10 * 1024 * 1024,
        diverged: false,
    };
    let res = finalize_workspace(&git, &params, &mut last_pushed_sha)
        .await
        .expect("finalize");
    assert!(!res.run_report.workspace.unwrap().wip_sha.is_empty());

    assert_eq!(
        std::fs::read_to_string(local.join(".env.example")).unwrap(),
        "KEY=edited\n",
        "finalize must not overwrite the agent's edits"
    );
    assert_eq!(git.git_status().await.unwrap(), status_before);
}

#[tokio::test]
async fn test_review_r13_prepare_recreates_missing_task_branch() {
    use crate::git::Git;
    use crate::workspace::prepare_workspace;
    use df_proto::agent::v1::Workspace;

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());
    let local = temp.path().join("local");
    let hooks = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&hooks).unwrap();
    // Existing workspace on `main` without a local task branch
    clone_for_workspace_test(&upstream, &local);

    let git = Git::new(&local)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&hooks);
    let workspace = Workspace {
        repo_url: format!("file://{}", upstream.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-13".to_string(),
        wip_branch: "factory/wip/DF-13".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    let res = prepare_workspace(&git, &local, &workspace, &hooks, "")
        .await
        .expect("prepare");
    assert!(!res.push_failed);
    assert_eq!(
        git.git_output(&["rev-parse", "--abbrev-ref", "HEAD"])
            .await
            .unwrap(),
        "factory/task/DF-13"
    );
}

// --- Regression tests for review iteration 3 (R37-R61) ---

type PendingCreds = std::sync::Arc<
    tokio::sync::Mutex<
        std::collections::HashMap<
            String,
            tokio::sync::oneshot::Sender<df_proto::agent::v1::CredentialGrant>,
        >,
    >,
>;

fn cred_handler_for_test(
    repo_url: &str,
    cached: Option<(String, String)>,
) -> (
    crate::runner::DriverRuntimeHandler,
    tokio::sync::mpsc::Receiver<AgentMessage>,
    PendingCreds,
) {
    let (outbound_tx, outbound_rx) = tokio::sync::mpsc::channel(10);
    let pending_creds =
        std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let handler = crate::runner::DriverRuntimeHandler::new(
        repo_url.to_string(),
        Vec::new(),
        outbound_tx,
        std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        std::sync::Arc::clone(&pending_creds),
        std::sync::Arc::new(tokio::sync::Mutex::new(cached)),
    );
    (handler, outbound_rx, pending_creds)
}

/// Answers the next `CredentialRequest` on `outbound_rx` with `grant`.
async fn answer_credential_request(
    outbound_rx: &mut tokio::sync::mpsc::Receiver<AgentMessage>,
    pending: &PendingCreds,
    username: &str,
    expires_at: Option<i64>,
) {
    use df_proto::agent::v1::{CredentialGrant, GitCredential, credential_grant};
    let msg = tokio::time::timeout(std::time::Duration::from_secs(2), outbound_rx.recv())
        .await
        .expect("a CredentialRequest must be sent")
        .expect("channel open");
    let Some(agent_message::Body::CredentialRequest(req)) = msg.body else {
        panic!("expected CredentialRequest, got {msg:?}");
    };
    let tx = pending
        .lock()
        .await
        .remove(&req.request_id)
        .expect("pending request");
    let _ = tx.send(CredentialGrant {
        request_id: req.request_id,
        result: Some(credential_grant::Result::Git(GitCredential {
            username: username.to_string(),
            password: "pw".to_string(),
            expires_at: expires_at
                .map(|seconds| df_proto::pbjson_types::Timestamp { seconds, nanos: 0 }),
        })),
    });
}

#[tokio::test]
async fn test_review_r38_erase_and_expiry_drop_cached_credentials() {
    use crate::runtime_socket::RuntimeHandler;

    let repo = "https://git.example.org/org/repo.git";
    let (handler, mut outbound_rx, pending) =
        cred_handler_for_test(repo, Some(("old".to_string(), "pw".to_string())));
    let handler = std::sync::Arc::new(handler);

    // Cached credentials are served without asking the control plane
    let (user, _, _) = handler
        .handle_git_credential("https", "git.example.org", "org/repo.git")
        .await;
    assert_eq!(user.as_deref(), Some("old"));
    assert!(outbound_rx.try_recv().is_err());

    // `erase` drops them, so the next `get` asks again
    handler
        .handle_git_credential_erase("https", "git.example.org", "org/repo.git")
        .await;
    let h = std::sync::Arc::clone(&handler);
    let get = tokio::spawn(async move {
        h.handle_git_credential("https", "git.example.org", "org/repo.git")
            .await
    });
    answer_credential_request(&mut outbound_rx, &pending, "fresh", None).await;
    assert_eq!(get.await.unwrap().0.as_deref(), Some("fresh"));

    // A grant that is about to expire is not served from the cache
    handler
        .handle_git_credential_erase("https", "git.example.org", "org/repo.git")
        .await;
    let h = std::sync::Arc::clone(&handler);
    let get = tokio::spawn(async move {
        h.handle_git_credential("https", "git.example.org", "org/repo.git")
            .await
    });
    answer_credential_request(
        &mut outbound_rx,
        &pending,
        "short-lived",
        Some(chrono::Utc::now().timestamp() + 5),
    )
    .await;
    assert_eq!(get.await.unwrap().0.as_deref(), Some("short-lived"));
    let h = std::sync::Arc::clone(&handler);
    let get = tokio::spawn(async move {
        h.handle_git_credential("https", "git.example.org", "org/repo.git")
            .await
    });
    answer_credential_request(&mut outbound_rx, &pending, "renewed", None).await;
    assert_eq!(get.await.unwrap().0.as_deref(), Some("renewed"));

    // Erase for another repository leaves the cache alone
    handler
        .handle_git_credential_erase("https", "git.example.org", "other/repo")
        .await;
    let (user, _, _) = handler
        .handle_git_credential("https", "git.example.org", "org/repo.git")
        .await;
    assert_eq!(user.as_deref(), Some("renewed"));
}

#[tokio::test]
async fn test_review_r38_git_credential_erase_reaches_handler_over_socket() {
    use crate::runtime_socket::{RuntimeHandler, RuntimeSocketServer, run_git_credential_stream};

    struct Recorder(std::sync::Arc<std::sync::atomic::AtomicBool>);
    #[tonic::async_trait]
    impl RuntimeHandler for Recorder {
        async fn handle_git_credential(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> (Option<String>, Option<String>, Option<String>) {
            (None, None, None)
        }
        async fn handle_git_credential_erase(&self, _: &str, host: &str, _: &str) {
            assert_eq!(host, "git.example.org");
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let sock = temp.path().join("runtime.sock");
    let erased = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server = RuntimeSocketServer::bind(
        &sock,
        std::sync::Arc::new(Recorder(std::sync::Arc::clone(&erased))),
    )
    .unwrap();
    let _handle = server.spawn();

    let input = b"protocol=https\nhost=git.example.org\npath=org/repo.git\n\n";
    let mut out = Vec::new();
    run_git_credential_stream(&input[..], &mut out, &sock, Some("erase"))
        .await
        .unwrap();
    assert!(erased.load(std::sync::atomic::Ordering::SeqCst));
    assert!(out.is_empty());
}

#[test]
fn test_review_r39_late_usage_record_is_counted_after_mark_turn_start() {
    use crate::usage::{SessionLogTracker, cwd_to_slug};
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let cwd = std::path::Path::new("/data/repo");
    let dir = home.join(".claude").join("projects").join(cwd_to_slug(cwd));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("sess.jsonl");
    std::fs::write(&file, "").unwrap();

    let mut tracker = SessionLogTracker::new(&home, cwd, "sess");
    tracker.mark_turn_start();

    let line = r#"{"type":"assistant","message":{"id":"m1","model":"m","role":"assistant","usage":{"output_tokens":40}}}"#;
    let (first, rest) = line.split_at(30);
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap();
    write!(f, "{first}").unwrap();
    assert!(tracker.parse_turn_usage().0.is_empty());

    // The harness finishes the line after the prompt response; the next turn starts
    writeln!(f, "{rest}").unwrap();
    tracker.mark_turn_start();
    let (usage, _) = tracker.parse_turn_usage();
    assert_eq!(usage.iter().map(|u| u.output_tokens).sum::<u64>(), 40);
}

#[tokio::test]
async fn test_review_r41_wip_snapshot_excludes_quoted_and_glob_like_secret_names() {
    use crate::git::Git;
    use crate::workspace::{FinalizeParams, finalize_workspace};
    use df_proto::agent::v1::{RunOutcome, Workspace};

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());
    let local = temp.path().join("local");
    let runtime_dir = temp.path().join("runtime");
    let hooks = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::create_dir_all(&hooks).unwrap();
    clone_for_workspace_test(&upstream, &local);
    let git = Git::new(&local)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&hooks);

    std::fs::write(local.join("clé.pem"), "secret").unwrap();
    std::fs::write(local.join("prod[1].key"), "secret").unwrap();
    std::fs::write(local.join("prod1.key"), "secret").unwrap();
    std::fs::write(local.join("notes.txt"), "keep me").unwrap();

    let workspace = Workspace {
        repo_url: format!("file://{}", upstream.display()),
        base_branch: "main".to_string(),
        task_branch: "main".to_string(),
        wip_branch: "factory/wip/DF-41".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };
    let mut last_pushed_sha = Some(git.current_head().await.unwrap());
    let params = FinalizeParams {
        repo_dir: &local,
        runtime_dir: &runtime_dir,
        workspace: &workspace,
        run_id: "run-r41",
        outcome: RunOutcome::Completed,
        detail: "",
        harness_session_id: "",
        hooks_path: &hooks,
        credential_helper: "",
        wip_size_cap_bytes: 10 * 1024 * 1024,
        diverged: false,
    };
    let res = finalize_workspace(&git, &params, &mut last_pushed_sha)
        .await
        .expect("finalize");
    let wip_sha = res.run_report.workspace.unwrap().wip_sha;
    assert!(!wip_sha.is_empty());

    let files = git
        .git_output(&["ls-tree", "-r", "-z", "--name-only", &wip_sha])
        .await
        .unwrap();
    let files: Vec<&str> = files.split('\0').filter(|f| !f.is_empty()).collect();
    assert!(files.contains(&"notes.txt"));
    for secret in ["clé.pem", "prod[1].key", "prod1.key"] {
        assert!(
            !files.contains(&secret),
            "{secret} must not be in the WIP snapshot: {files:?}"
        );
    }
}

#[test]
fn test_review_r42_runtime_writes_do_not_follow_planted_symlinks() {
    use crate::fs_util::{create_real_dir, write_fresh};
    use crate::git::{GitConfigParams, rewrite_git_config};

    let temp = tempfile::tempdir().unwrap();
    let victim = temp.path().join("run.json");
    std::fs::write(&victim, "original").unwrap();

    // write_fresh replaces the link instead of writing through it
    let link = temp.path().join("settings.json");
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    write_fresh(&link, "new").unwrap();
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "original");
    assert_eq!(std::fs::read_to_string(&link).unwrap(), "new");

    // a symlinked directory is replaced by a real one
    let dir_target = temp.path().join("elsewhere");
    std::fs::create_dir_all(&dir_target).unwrap();
    let dir_link = temp.path().join(".claude");
    std::os::unix::fs::symlink(&dir_target, &dir_link).unwrap();
    create_real_dir(&dir_link).unwrap();
    assert!(
        !std::fs::symlink_metadata(&dir_link)
            .unwrap()
            .file_type()
            .is_symlink()
    );

    // .git/config planted as a symlink
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::os::unix::fs::symlink(&victim, repo.join(".git").join("config")).unwrap();
    let params = GitConfigParams {
        hooks_path: std::path::Path::new("/opt/factory/hooks-empty"),
        extra_config: &[],
        repo_url: "https://github.com/org/repo.git",
        credential_helper: "",
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };
    rewrite_git_config(&repo, &params).unwrap();
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "original");
    assert!(
        std::fs::read_to_string(repo.join(".git/config"))
            .unwrap()
            .contains("[remote \"origin\"]")
    );
}

#[test]
fn test_review_r52_repo_url_with_userinfo_matches() {
    use crate::runner::matches_repo_url;

    let repo = "https://x-access-token:abc@git.example.com/org/repo.git";
    assert!(matches_repo_url(
        repo,
        "https",
        "git.example.com",
        "org/repo"
    ));
    assert!(!matches_repo_url(
        repo,
        "https",
        "evil.example.com",
        "org/repo"
    ));
}

#[test]
fn test_review_r53_runtime_owned_values_are_quoted_and_validated() {
    use crate::git::{GitConfigParams, render_git_config, rewrite_git_config};

    let params = GitConfigParams {
        hooks_path: std::path::Path::new("/opt/factory/hooks-empty"),
        extra_config: &[],
        repo_url: "https://git.example.org/a.git",
        credential_helper: "helper",
        commit_author_name: "Team #1 Bot",
        commit_author_email: "bot@example.com",
    };
    assert!(render_git_config(&params).contains("name = \"Team #1 Bot\""));

    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".git")).unwrap();
    let evil = GitConfigParams {
        commit_author_name: "Bot\n[core]\nhooksPath = /x",
        ..params
    };
    assert!(rewrite_git_config(temp.path(), &evil).is_err());
}

#[tokio::test]
async fn test_review_r49_append_or_stop_appends_when_stopped_while_full() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    let ob = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(temp.path(), "run-a", 10, 64 * 1024).unwrap(),
    ));
    let msg = |detail: &str| AgentMessage {
        seq: 0,
        body: Some(agent_message::Body::PhaseChanged(
            df_proto::agent::v1::PhaseChanged {
                phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                detail: detail.to_string(),
            },
        )),
    };
    // Fills the outbox beyond its (soft) limit
    Outbox::append_with_backpressure(&ob, msg("first message"))
        .await
        .unwrap();

    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let ob2 = std::sync::Arc::clone(&ob);
    let waiting = tokio::spawn(async move {
        Outbox::append_or_stop(&ob2, msg("second message"), &mut stop_rx).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !waiting.is_finished(),
        "must wait for space while not stopped"
    );

    stop_tx.send(true).unwrap();
    let seq = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
        .await
        .expect("stop must release the wait")
        .unwrap()
        .unwrap();
    assert_eq!(seq, 2);
    assert_eq!(ob.lock().await.read_unacknowledged(0).unwrap().len(), 2);
}

#[tokio::test]
async fn test_review_r51_cancel_before_assign_skips_prepare() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{Assign, RunOutcome, Workspace};

    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let repo_dir = data.join("repo");
    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&data.join("runtime"), "run-r51", 1024 * 1024, 64 * 1024).unwrap(),
    ));
    let (_inbound_tx, inbound_rx) = tokio::sync::mpsc::channel(10);
    let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel(100);

    let params = RunDriverParams {
        assign: Assign {
            run_id: "run-r51".to_string(),
            task: None,
            workspace: Some(Workspace {
                repo_url: "https://localhost:1/never-contacted.git".to_string(),
                base_branch: "main".to_string(),
                task_branch: "factory/task/DF-51".to_string(),
                wip_branch: "factory/wip/DF-51".to_string(),
                commit_identity: None,
                git_hooks: vec![],
                git_config: vec![],
            }),
            harness: None,
            tools: vec![],
            budgets: None,
            continuation: None,
        },
        credentials: None,
        initial_prompts: vec![],
        cancel_requested: true,
        repo_dir: repo_dir.clone(),
        home_dir: data.join("home"),
        runtime_dir: temp.path().join("run"),
        hooks_path: temp.path().join("hooks"),
        mcp_command: std::path::PathBuf::from("factory-agent"),
        runtime_sock_path: temp.path().join("run").join("runtime.sock"),
        git: Git::new(&repo_dir),
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
    };
    let result = run_session(params).await.unwrap();
    assert_eq!(result.outcome, RunOutcome::Cancelled);
    assert!(
        !repo_dir.join(".git").exists(),
        "the workspace must not be prepared"
    );
    let report = outbound_rx.recv().await.expect("RunReport");
    assert!(matches!(
        report.body,
        Some(agent_message::Body::RunReport(_))
    ));
}

#[tokio::test]
async fn test_review_r2_credential_grant_delivered_during_prepare() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use crate::test_harness::{LocalHttpsGitServer, TestPki};
    use df_proto::agent::v1::{
        Assign, CredentialGrant, GitCredential, Harness, Workspace, credential_grant,
    };

    let mcp_bin = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/factory-agent");
    if !mcp_bin.exists() {
        eprintln!("Skipping test: factory-agent binary not present");
        return;
    }

    let temp = tempfile::tempdir().unwrap();
    let _upstream = setup_test_remote_repo(temp.path()); // temp/upstream.git
    let pki = TestPki::generate("localhost").unwrap();
    let server = LocalHttpsGitServer::start(
        &pki,
        temp.path().to_path_buf(),
        Some(("bot".to_string(), "tok".to_string())),
    )
    .unwrap();
    let ca_path = temp.path().join("ca.crt");
    std::fs::write(&ca_path, &pki.ca_cert_pem).unwrap();

    let data = temp.path().join("data");
    let repo_dir = data.join("repo");
    let hooks = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&hooks).unwrap();
    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(&data.join("runtime"), "run-r2", 1024 * 1024, 64 * 1024).unwrap(),
    ));

    let (inbound_tx, inbound_rx) = tokio::sync::mpsc::channel(10);
    let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel::<AgentMessage>(100);
    // Control plane stand-in: answers every credential request, drops everything else
    let responder = tokio::spawn(async move {
        let mut asked = 0;
        while let Some(msg) = outbound_rx.recv().await {
            if let Some(agent_message::Body::CredentialRequest(req)) = msg.body {
                asked += 1;
                let _ = inbound_tx
                    .send(ControlMessage {
                        ack: 0,
                        body: Some(control_message::Body::CredentialGrant(CredentialGrant {
                            request_id: req.request_id,
                            result: Some(credential_grant::Result::Git(GitCredential {
                                username: "bot".to_string(),
                                password: "tok".to_string(),
                                expires_at: None,
                            })),
                        })),
                    })
                    .await;
            }
        }
        asked
    });

    let params = RunDriverParams {
        assign: Assign {
            run_id: "run-r2".to_string(),
            task: None,
            workspace: Some(Workspace {
                repo_url: format!("https://localhost:{}/upstream.git", server.addr.port()),
                base_branch: "main".to_string(),
                task_branch: "factory/task/DF-2".to_string(),
                wip_branch: "factory/wip/DF-2".to_string(),
                commit_identity: None,
                git_hooks: vec![],
                git_config: vec![],
            }),
            // The harness can't start; reaching that point means prepare succeeded
            harness: Some(Harness {
                kind: Some(df_proto::agent::v1::harness::Kind::Acp(
                    df_proto::agent::v1::AcpHarness {
                        command: vec!["/nonexistent/harness".to_string()],
                        env: std::collections::HashMap::new(),
                        model: None,
                        settings_json: String::new(),
                    },
                )),
                system_prompt: String::new(),
            }),
            tools: vec![],
            budgets: None,
            continuation: None,
        },
        credentials: None,
        initial_prompts: vec![],
        cancel_requested: false,
        repo_dir: repo_dir.clone(),
        home_dir: data.join("home"),
        runtime_dir: temp.path().join("run"),
        hooks_path: hooks.clone(),
        mcp_command: mcp_bin,
        runtime_sock_path: temp.path().join("run").join("runtime.sock"),
        git: Git::new(&repo_dir)
            .allow_protocols(["https"])
            .with_hooks_empty_dir(&hooks)
            .with_ssl_ca_info(&ca_path),
        outbox,
        inbound_rx,
        outbound_tx,
        git_credentials: None,
    };
    let result = run_session(params).await.unwrap();
    assert!(
        result
            .run_report
            .detail
            .starts_with("Failed to spawn harness"),
        "prepare must have succeeded, got: {}",
        result.run_report.detail
    );
    assert!(repo_dir.join(".git").exists());
    drop(result);
    server.shutdown();
    responder.abort();
}

fn flush_test_report() -> AgentMessage {
    AgentMessage {
        seq: 0,
        body: Some(agent_message::Body::RunReport(
            df_proto::agent::v1::RunReport {
                outcome: df_proto::agent::v1::RunOutcome::EndTurn as i32,
                detail: "done".to_string(),
                retry_after: None,
                harness_session_id: String::new(),
                workspace: None,
            },
        )),
    }
}

/// Starts a connection manager against a mock control plane and completes the first handshake.
async fn start_flush_test_connection(
    run_id: &str,
) -> (
    crate::connection::ConnectionHandle,
    MockControlPlane,
    tempfile::TempDir,
) {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");
    let temp = tempfile::tempdir().unwrap();
    let tls_dir = temp.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).unwrap();
    let runtime_dir = temp.path().join("runtime");
    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::outbox::Outbox::open(&runtime_dir, run_id, 1024 * 1024, 64 * 1024).unwrap(),
    ));
    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir,
        data_dir: temp.path().join("data"),
        run_dir: runtime_dir,
    };
    let policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };
    let handle = crate::connection::ConnectionManager::new(config, outbox, policy).spawn();
    let _ = server.recv_agent_message().await.expect("hello");
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: run_id.to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .unwrap();
    (handle, server, temp)
}

/// Receives messages until the RunReport arrives, acknowledges it, and returns its seq.
async fn receive_and_ack_report(server: &mut MockControlPlane) -> u64 {
    let report = loop {
        let msg = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            server.recv_agent_message(),
        )
        .await
        .expect("RunReport must be delivered")
        .expect("stream open");
        if matches!(msg.body, Some(agent_message::Body::RunReport(_))) {
            break msg;
        }
    };
    server
        .send_control_message(ControlMessage {
            ack: report.seq,
            body: None,
        })
        .await
        .unwrap();
    report.seq
}

#[tokio::test]
async fn test_review_r61_final_report_is_flushed_at_shutdown() {
    let (mut handle, mut server, _temp) = start_flush_test_connection("run-flush").await;
    handle.send(flush_test_report()).await.unwrap();
    handle.shutdown();

    assert_eq!(receive_and_ack_report(&mut server).await, 1);
    let task = handle.task.take().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .expect("connection manager must finish once the report is acked")
        .unwrap();
    server.shutdown();
}

#[tokio::test]
async fn test_review_r40_final_report_is_delivered_after_shutdown_while_disconnected() {
    let (mut handle, mut server, _temp) = start_flush_test_connection("run-flush-offline").await;
    server.drop_active_stream().await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    handle.send(flush_test_report()).await.unwrap();
    handle.shutdown();

    // The manager must reconnect instead of exiting
    loop {
        let msg = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            server.recv_agent_message(),
        )
        .await
        .expect("manager must reconnect after shutdown")
        .expect("stream open");
        if matches!(msg.body, Some(agent_message::Body::Hello(_))) {
            break;
        }
    }
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-flush-offline".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .unwrap();
    receive_and_ack_report(&mut server).await;
    let task = handle.task.take().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .expect("connection manager must finish once the report is acked")
        .unwrap();
    server.shutdown();
}
