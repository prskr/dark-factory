//! Runtime scenarios around `run_session`: timings, `complete_task`, full runtime, resumed sessions.

#![allow(
    clippy::too_many_lines,
    reason = "scenario tests set up and check a whole run"
)]

use df_proto::agent::v1::{AgentMessage, ControlMessage, agent_message, control_message};

use super::{RunScenario, release_message, setup_test_remote_repo};

#[tokio::test]
async fn test_run_sends_runtime_timings_in_sequence_with_other_messages() {
    use df_proto::agent::v1::{Prompt, RuntimeOperation, agent_message};

    let script = r#"
sessions:
  new:
    - - shell: "echo x >> f.txt && git add f.txt && git config user.name 'Agent Bot' && git config user.email 'bot@example.com' && git config commit.gpgsign false && git commit --no-gpg-sign -m 'work'"
      - end: end_turn
    - - say: "second turn"
      - end: end_turn
"#;
    let Some(scenario) = RunScenario::new("run-timing", script) else {
        return;
    };
    // Mock control plane: queues the second turn's prompt during the first turn.
    let mut turns = 0;
    let queue_second_turn =
        move |msg: &AgentMessage, control: &tokio::sync::mpsc::Sender<ControlMessage>| {
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
                    control
                        .try_send(ControlMessage {
                            ack: 0,
                            body: Some(control_message::Body::Prompt(prompt)),
                        })
                        .expect("the run still reads its inbox");
                }
            }
        };
    let (_, messages) = scenario.release_up_front().run(queue_second_turn).await;
    let bodies: Vec<_> = messages.into_iter().map(|m| m.body).collect();

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
            hidden: false,
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
                        await_input_after_turn: false,
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
            hidden: false,
        }],
        queued_at: None,
    }];

    let (inbound_tx1, inbound_rx1) = mpsc::channel(100);
    inbound_tx1.try_send(release_message()).unwrap();
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
            hidden: false,
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
                            hidden: false,
                        }],
                        structured_content_json: String::new(),
                        end_run_after_turn: true,
                        await_input_after_turn: false,
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
            hidden: false,
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
                        await_input_after_turn: false,
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

#[tokio::test]
async fn test_cancel_before_assign_skips_prepare() {
    use crate::git::Git;
    use crate::outbox::Outbox;
    use crate::runner::{RunDriverParams, run_session};
    use df_proto::agent::v1::{Assign, RunOutcome, Workspace};

    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let repo_dir = data.join("repo");
    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(
            &data.join("runtime"),
            "run-cancel-before-assign",
            1024 * 1024,
            64 * 1024,
        )
        .unwrap(),
    ));
    let (_inbound_tx, inbound_rx) = tokio::sync::mpsc::channel(10);
    let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel(100);

    let params = RunDriverParams {
        assign: Assign {
            run_id: "run-cancel-before-assign".to_string(),
            task: None,
            workspace: Some(Workspace {
                repo_url: "https://localhost:1/never-contacted.git".to_string(),
                base_branch: "main".to_string(),
                task_branch: "factory/task/DF-1".to_string(),
                wip_branch: "factory/wip/DF-1".to_string(),
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
async fn test_credential_grant_delivered_during_prepare() {
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
        Outbox::open(
            &data.join("runtime"),
            "run-credential-grant",
            1024 * 1024,
            64 * 1024,
        )
        .unwrap(),
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
            run_id: "run-credential-grant".to_string(),
            task: None,
            workspace: Some(Workspace {
                repo_url: format!("https://localhost:{}/upstream.git", server.addr.port()),
                base_branch: "main".to_string(),
                task_branch: "factory/task/DF-1".to_string(),
                wip_branch: "factory/wip/DF-1".to_string(),
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

/// A resumed harness replays the conversation before it answers `session/load`. The replay is
/// already in the transcript and must not be recorded again.
#[tokio::test]
async fn test_resumed_session_does_not_record_the_replayed_history() {
    use df_proto::agent::v1::agent_message;
    use df_proto::transcript::v1::event::Kind as EventKind;

    let script = r#"
sessions:
  new:
    - - say: "old answer"
      - end: end_turn
  loaded:
    - - say: "new answer"
      - end: end_turn
"#;
    let Some(scenario) = RunScenario::new("run-resume", script) else {
        return;
    };
    // The session of the previous run, as the harness keeps it under HOME.
    let sessions = scenario.home_dir().join(".fake-acp").join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(sessions.join("sess-1"), "").unwrap();

    let (_, messages) = scenario
        .empty_prompt()
        .continuation("sess-1", "old")
        .release_up_front()
        .run(|_, _| {})
        .await;
    let texts: Vec<String> = messages
        .into_iter()
        .filter_map(|msg| match msg.body {
            Some(agent_message::Body::TranscriptEvent(ev)) => match ev.kind {
                Some(EventKind::AgentMessageChunk(chunk)) => chunk.content,
                _ => None,
            },
            _ => None,
        })
        .filter_map(|block| match block.kind {
            Some(df_proto::transcript::v1::content_block::Kind::Text(t)) => Some(t.text),
            _ => None,
        })
        .collect();

    assert_eq!(texts, ["new answer"]);
}
