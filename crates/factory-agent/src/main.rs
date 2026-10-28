//! Dark Factory sandbox agent runtime (`factory-agent`).

use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;

pub mod cli;
pub mod config;
pub mod connection;
pub mod fs_util;
pub mod git;
pub mod harness;
pub mod mcp;
pub mod outbox;
pub mod runner;
pub mod runtime_socket;
pub mod transcript;
pub mod usage;
pub mod workspace;

#[cfg(test)]
pub mod test_harness;

#[cfg(test)]
mod tests;

use cli::{Cli, Command};

/// How long `run` waits for the control plane to send `Assign`.
const ASSIGN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// `core.hooksPath` for the agent's git: the empty, read-only hooks directory from the image.
const HARNESS_HOOKS_PATH: &str = "/opt/factory/hooks-empty";

#[tokio::main]
#[expect(
    clippy::too_many_lines,
    reason = "split main into argument parsing, setup and run steps"
)]
async fn main() -> ExitCode {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();
    match cli.command {
        Command::Run => {
            df_telemetry::logging::init_tracing(cli.log_format, None);
            let config = match config::RuntimeConfig::from_env() {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(error = %e, "invalid configuration");
                    return ExitCode::FAILURE;
                }
            };

            let runtime_dir = config.run_dir.clone();
            let data_dir = config.data_dir.clone();
            let outbox_data_dir = data_dir.join("runtime");
            let repo_dir = data_dir.join("repo");
            let home_dir = data_dir.join("home");

            for dir in [&runtime_dir, &outbox_data_dir, &repo_dir, &home_dir] {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    tracing::error!(error = %e, dir = %dir.display(), "failed to create directory");
                    return ExitCode::FAILURE;
                }
            }

            // An empty run id keeps the stored state; the control plane's HelloAck/Assign decide
            // whether it belongs to the current run.
            let outbox = match outbox::Outbox::open(
                &outbox_data_dir,
                "",
                outbox::DEFAULT_MAX_OUTBOX_BYTES,
                outbox::DEFAULT_MAX_SEGMENT_BYTES,
            ) {
                Ok(o) => std::sync::Arc::new(tokio::sync::Mutex::new(o)),
                Err(e) => {
                    tracing::error!(error = %e, "failed to open outbox");
                    return ExitCode::FAILURE;
                }
            };

            let conn_mgr = connection::ConnectionManager::new(
                config.clone(),
                std::sync::Arc::clone(&outbox),
                connection::ReconnectPolicy::default(),
            );
            let mut conn_handle = conn_mgr.spawn();

            let mut assign = None;
            let mut credentials = None;
            let mut initial_prompts = Vec::new();
            let mut cancel_requested = false;

            // Without an Assign the pod would idle forever while holding the volume
            let wait_for_assign = async {
                while let Some(msg) = conn_handle.inbound_rx.recv().await {
                    match msg.body {
                        Some(df_proto::agent::v1::control_message::Body::Assign(a)) => {
                            assign = Some(a);
                            break;
                        }
                        Some(df_proto::agent::v1::control_message::Body::Cancel(_)) => {
                            cancel_requested = true;
                        }
                        Some(df_proto::agent::v1::control_message::Body::HarnessCredentials(c)) => {
                            credentials = Some(c);
                        }
                        Some(df_proto::agent::v1::control_message::Body::Prompt(p)) => {
                            initial_prompts.push(p);
                        }
                        _ => {}
                    }
                }
            };
            if tokio::time::timeout(ASSIGN_TIMEOUT, wait_for_assign)
                .await
                .is_err()
            {
                tracing::error!(timeout = ?ASSIGN_TIMEOUT, "no Assign received from the control plane");
                conn_handle.shutdown();
                return ExitCode::FAILURE;
            }

            let assign = match assign {
                Some(a) => a,
                None => {
                    tracing::error!("inbound stream closed without receiving Assign");
                    conn_handle.shutdown();
                    return ExitCode::FAILURE;
                }
            };

            log_assigned(&assign);

            {
                let mut ob = outbox.lock().await;
                if ob.run_state().run_id != assign.run_id
                    && let Err(e) = ob.discard(&assign.run_id)
                {
                    tracing::error!(error = %e, "failed to discard outbox");
                }
            }

            // Read-only empty hooks directory from the image, never a path the project or agent can write
            let hooks_path = std::path::PathBuf::from(HARNESS_HOOKS_PATH);
            let mcp_command = std::env::current_exe()
                .unwrap_or_else(|_| std::path::PathBuf::from("factory-agent"));
            let runtime_sock_path = runtime_dir.join("runtime.sock");
            let git = git::Git::new(&repo_dir);

            let shutdown_tx = conn_handle.shutdown_tx();
            let conn_task = conn_handle.task.take();

            let driver_params = runner::RunDriverParams {
                assign,
                credentials,
                initial_prompts,
                cancel_requested,
                repo_dir,
                home_dir,
                runtime_dir,
                hooks_path,
                mcp_command,
                runtime_sock_path,
                git,
                outbox,
                inbound_rx: conn_handle.inbound_rx,
                outbound_tx: conn_handle.outbound_tx.clone(),
                git_credentials: None,
            };

            let res = runner::run_session(driver_params).await;
            // Let the connection manager flush the final reports before the process exits
            let _ = shutdown_tx.send(true);
            if let Some(task) = conn_task {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(30), task).await;
            }

            match res {
                Ok(exec_res) => {
                    if exec_res.outcome == df_proto::agent::v1::RunOutcome::Failed {
                        ExitCode::FAILURE
                    } else {
                        ExitCode::SUCCESS
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "run execution failed");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Mcp { socket } => {
            if let Err(e) = mcp::run_mcp_stdio_server(&socket).await {
                eprintln!("mcp server error: {e}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Command::GitCredential { operation, socket } => {
            if let Err(e) =
                runtime_socket::run_git_credential_client(&socket, operation.as_deref()).await
            {
                eprintln!("git-credential error: {e}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
    }
}

/// Logs the run assignment (ids only).
fn log_assigned(assign: &df_proto::agent::v1::Assign) {
    tracing::info!(
        run_id = %assign.run_id,
        task_key = %assign.task.as_ref().map(|t| t.task_key.as_str()).unwrap_or_default(),
        "run assigned"
    );
}
