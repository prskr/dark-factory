//! Unit and integration tests for `factory-agent`, one file per exercised module. Shared fixtures
//! live here.

use std::path::PathBuf;
use std::time::Duration;

use df_proto::agent::v1::{
    AcpHarness, AgentMessage, Assign, Budgets, Continuation, ControlMessage, Harness, Prompt,
    Release, ToolDescriptor, Workspace, control_message,
};
use tokio::sync::mpsc;

use crate::git::Git;
use crate::outbox::Outbox;
use crate::runner::{RunDriverParams, RunExecutionResult, run_session};

mod cli;
mod connection;
mod git;
mod git_retry;
mod harness;
mod mcp;
mod outbox;
mod runner;
mod runner_idle;
mod runner_runtime;
mod transcript;
mod usage;
mod workspace;
mod workspace_finalize;

/// A `Release`, as the control plane sends it once the idle deadline passed. The runtime keeps
/// an early one, so tests queue it up front to end the idle wait after the last turn.
fn release_message() -> ControlMessage {
    ControlMessage {
        ack: 0,
        body: Some(control_message::Body::Release(Release {})),
    }
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

pub(crate) fn setup_test_remote_repo(temp_dir: &std::path::Path) -> PathBuf {
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
    std::process::Command::new("git")
        .args(["config", "user.name", "Bot"])
        .current_dir(&init_repo)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "bot@example.com"])
        .current_dir(&init_repo)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "commit.gpgsign", "false"])
        .current_dir(&init_repo)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["checkout", "-b", "main"])
        .current_dir(&init_repo)
        .status()
        .unwrap();
    std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&init_repo)
        .status()
        .unwrap();
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

/// Setup shared by the `run_session` scenarios: a temp directory with an upstream repository,
/// the script of the fake ACP agent, an `Assign` for it, the outbox and the channels.
///
/// Scenarios whose setup differs (two runs on one data directory, a different harness binary,
/// a cancel before `Assign`) assemble their own.
pub(crate) struct RunScenario {
    temp: tempfile::TempDir,
    run_id: String,
    upstream: PathBuf,
    script_file: PathBuf,
    fake_bin: PathBuf,
    mcp_bin: PathBuf,
    prompt: Option<String>,
    tools: Vec<ToolDescriptor>,
    budgets: Option<Budgets>,
    continuation: Option<Continuation>,
    release_up_front: bool,
}

impl RunScenario {
    /// A run `run_id` of the fake ACP agent playing `script`, with one initial prompt "Start
    /// working". `None` when the helper binaries are not built (the scenario is skipped then).
    pub(crate) fn new(run_id: &str, script: &str) -> Option<Self> {
        let bin = |name: &str| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/debug")
                .join(name)
        };
        let (fake_bin, mcp_bin) = (bin("fake-acp-agent"), bin("factory-agent"));
        if !fake_bin.exists() || !mcp_bin.exists() {
            eprintln!("Skipping integration test: binaries not present");
            return None;
        }
        let temp = tempfile::tempdir().unwrap();
        let upstream = setup_test_remote_repo(temp.path());
        let script_file = temp.path().join("script.yaml");
        std::fs::write(&script_file, script).unwrap();
        Some(Self {
            temp,
            run_id: run_id.to_string(),
            upstream,
            script_file,
            fake_bin,
            mcp_bin,
            prompt: Some("Start working".to_string()),
            tools: Vec::new(),
            budgets: None,
            continuation: None,
            release_up_front: false,
        })
    }

    /// The text of the initial prompt.
    pub(crate) fn prompt(mut self, text: &str) -> Self {
        self.prompt = Some(text.to_string());
        self
    }

    /// An initial prompt without content.
    pub(crate) fn empty_prompt(mut self) -> Self {
        self.prompt = None;
        self
    }

    pub(crate) fn tools(mut self, tools: Vec<ToolDescriptor>) -> Self {
        self.tools = tools;
        self
    }

    pub(crate) fn budgets(mut self, budgets: Budgets) -> Self {
        self.budgets = Some(budgets);
        self
    }

    /// Resume the harness session `session_id` of a previous run.
    pub(crate) fn continuation(mut self, session_id: &str, summary: &str) -> Self {
        self.continuation = Some(Continuation {
            harness_session_id: session_id.to_string(),
            summary: summary.to_string(),
        });
        self
    }

    /// Queue a `Release` before the run starts so the idle wait ends after the last turn.
    pub(crate) fn release_up_front(mut self) -> Self {
        self.release_up_front = true;
        self
    }

    /// The `HOME` of the harness, for tests that prepare its state.
    pub(crate) fn home_dir(&self) -> PathBuf {
        self.temp.path().join("home")
    }

    /// The parameters of the run, wired to the given channels.
    pub(crate) fn params(
        &self,
        inbound_rx: mpsc::Receiver<ControlMessage>,
        outbound_tx: mpsc::Sender<AgentMessage>,
    ) -> RunDriverParams {
        let temp = self.temp.path();
        let repo_dir = temp.join("repo");
        let runtime_dir = temp.join("runtime");
        let hooks_path = temp.join("hooks-empty");
        std::fs::create_dir_all(&hooks_path).unwrap();
        let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
            Outbox::open(&runtime_dir, &self.run_id, 10 * 1024 * 1024, 64 * 1024).unwrap(),
        ));
        let assign = Assign {
            run_id: self.run_id.clone(),
            task: None,
            workspace: Some(Workspace {
                repo_url: self.upstream.to_str().unwrap().to_string(),
                base_branch: "main".to_string(),
                task_branch: "factory/task/DF-1".to_string(),
                wip_branch: "factory/wip/DF-1".to_string(),
                commit_identity: None,
                git_hooks: Vec::new(),
                git_config: Vec::new(),
            }),
            harness: Some(Harness {
                kind: Some(df_proto::agent::v1::harness::Kind::Acp(AcpHarness {
                    command: vec![
                        self.fake_bin.to_string_lossy().to_string(),
                        "--script".to_string(),
                        self.script_file.to_string_lossy().to_string(),
                    ],
                    env: std::collections::HashMap::new(),
                    model: None,
                    settings_json: String::new(),
                })),
                system_prompt: String::new(),
            }),
            tools: self.tools.clone(),
            budgets: self.budgets,
            continuation: self.continuation.clone(),
        };
        let content = self
            .prompt
            .iter()
            .map(|text| df_proto::transcript::v1::ContentBlock {
                kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                    df_proto::transcript::v1::TextContent { text: text.clone() },
                )),
                hidden: false,
            });
        RunDriverParams {
            assign,
            credentials: None,
            initial_prompts: vec![Prompt {
                prompt_id: "prompt-1".to_string(),
                author: None,
                content: content.collect(),
                queued_at: None,
            }],
            home_dir: self.home_dir(),
            runtime_dir,
            hooks_path,
            mcp_command: self.mcp_bin.clone(),
            runtime_sock_path: temp.join("runtime.sock"),
            git: Git::new(&repo_dir).allow_protocols(["file", "https"]),
            repo_dir,
            outbox,
            inbound_rx,
            outbound_tx,
            cancel_requested: false,
        }
    }

    /// Runs the session to its end. `react` plays the control plane: it sees every message the
    /// runtime sends and answers through the sender. Returns the result and all messages the
    /// runtime sent.
    pub(crate) async fn run(
        self,
        mut react: impl FnMut(&AgentMessage, &mpsc::Sender<ControlMessage>) + Send + 'static,
    ) -> (RunExecutionResult, Vec<AgentMessage>) {
        let (inbound_tx, inbound_rx) = mpsc::channel(100);
        if self.release_up_front {
            inbound_tx.try_send(release_message()).unwrap();
        }
        let (outbound_tx, mut outbound_rx) = mpsc::channel(100);
        let params = self.params(inbound_rx, outbound_tx);
        // The sender stays alive until the runtime closes its channel, as the run reads its inbox.
        let control_plane = tokio::spawn(async move {
            let mut sent = Vec::new();
            while let Some(msg) = outbound_rx.recv().await {
                react(&msg, &inbound_tx);
                sent.push(msg);
            }
            sent
        });
        let result = run_session(params)
            .await
            .expect("run_session should succeed");
        let sent = tokio::time::timeout(Duration::from_secs(60), control_plane)
            .await
            .expect("the runtime closes its channel when the run ends")
            .unwrap();
        (result, sent)
    }
}
