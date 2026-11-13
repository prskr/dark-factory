//! Dark Factory sandbox agent runtime (`factory-agent`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use df_telemetry::logging::LogFormat;
use tokio::sync::Mutex;

pub mod artifact_path;
pub mod cli;
pub mod config;
pub mod connection;
pub mod fs_util;
pub mod git;
pub mod harness;
pub mod hidden_paths;
pub mod mcp;
pub mod outbox;
pub mod runner;
pub mod runtime_handler;
pub mod runtime_socket;
pub mod sandbox;
pub mod transcript;
pub mod workspace;

#[cfg(test)]
pub mod test_harness;

#[cfg(test)]
mod tests;

use cli::{Cli, Command};

/// Time on top of the final report flush window for a connection attempt that is under way when
/// the window ends (three handshake steps of 15 s each).
const FINAL_FLUSH_MARGIN: Duration = Duration::from_secs(60);
/// How long `run` waits for the control plane to send `Assign`.
const ASSIGN_TIMEOUT: Duration = Duration::from_mins(10);
/// `core.hooksPath` for the agent's git: the empty, read-only hooks directory from the image.
const HARNESS_HOOKS_PATH: &str = "/opt/factory/hooks-empty";

#[expect(
    clippy::print_stderr,
    reason = "the mcp and sandbox subcommands report errors on stderr"
)]
fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Command::Sandbox { root, command } = cli.command {
        // Without an async runtime: the wrapper closes every inherited descriptor, which would
        // include the runtime's own, and a runtime that lost them hangs when it shuts down.
        // `run` only returns when the command did not start.
        eprintln!("sandbox: {:#}", sandbox::run(&root, &command));
        return ExitCode::from(sandbox::SETUP_FAILED);
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("cannot start the async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run_command(cli))
}

#[expect(
    clippy::print_stderr,
    reason = "the mcp subcommand reports errors on stderr"
)]
async fn run_command(cli: Cli) -> ExitCode {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    match cli.command {
        Command::Run => run_agent(cli.log_format).await,
        Command::Mcp { socket } => {
            if let Err(e) = mcp::run_mcp_stdio_server(&socket).await {
                eprintln!("mcp server error: {e}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Command::Sandbox { .. } => ExitCode::FAILURE,
    }
}

/// Directories of a run on the runtime volumes.
struct RunDirs {
    runtime: PathBuf,
    outbox_data: PathBuf,
    repo: PathBuf,
    home: PathBuf,
}

/// What the control plane sent before and with `Assign`.
struct Startup {
    assign: df_proto::agent::v1::Assign,
    initial_prompts: Vec<df_proto::agent::v1::Prompt>,
    /// True if a `Cancel` arrived before `Assign`.
    cancel_requested: bool,
}

/// The `run` command: connects to the control plane, waits for `Assign` and drives the run.
async fn run_agent(log_format: LogFormat) -> ExitCode {
    df_telemetry::logging::init_tracing(log_format, None);
    let config = match config::RuntimeConfig::from_env() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = format!("{e:#}"), "invalid configuration");
            return ExitCode::FAILURE;
        }
    };
    let layout = config.layout.clone();
    // Before anything is created or opened below the data directory: aliases that an unrestricted
    // run left on the task volume must not redirect or reach the outbox.
    if !clean_outbox_aliases(&layout.outbox_dir()) {
        return ExitCode::FAILURE;
    }
    let Some(dirs) = create_run_dirs(&layout) else {
        return ExitCode::FAILURE;
    };
    let Some(outbox) = open_outbox(&dirs.outbox_data) else {
        return ExitCode::FAILURE;
    };
    // The gateway's Git proxy presents a certificate of the instance CA. The runtime keeps a copy
    // that the harness may read (the TLS directory holds the private key).
    if let Err(e) = sandbox::aliases::publish_ca(&layout.tls_dir, &layout.ca_dir()) {
        tracing::error!(error = %e, "failed to copy the instance CA");
        return ExitCode::FAILURE;
    }
    let instance_ca = layout.ca_file();
    let confinement = runner::Confinement::new(config.landlock);

    let mut conn_handle = connection::ConnectionManager::new(
        config,
        Arc::clone(&outbox),
        connection::ReconnectPolicy::default(),
    )
    .spawn();
    let Some(startup) = receive_assignment(&mut conn_handle).await else {
        conn_handle.shutdown();
        return ExitCode::FAILURE;
    };
    log_assigned(&startup.assign);
    discard_outbox_of_other_run(&outbox, &startup.assign.run_id).await;

    let shutdown_tx = conn_handle.shutdown_tx();
    let conn_task = conn_handle.task.take();
    let driver_params = runner::RunDriverParams {
        confinement,
        harness_base_env: std::env::vars_os().collect(),
        assign: startup.assign,
        initial_prompts: startup.initial_prompts,
        cancel_requested: startup.cancel_requested,
        timeouts: runner::Timeouts::default(),
        git: git::Git::new(&dirs.repo)
            .with_ssl_ca_info(&instance_ca)
            .with_runtime_dir(&dirs.runtime),
        runtime_sock_path: dirs.runtime.join("runtime.sock"),
        repo_dir: dirs.repo,
        home_dir: dirs.home,
        runtime_dir: dirs.runtime,
        // Read-only empty hooks directory from the image, never a path the project or agent can write
        hooks_path: PathBuf::from(HARNESS_HOOKS_PATH),
        runtime_exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("factory-agent")),
        outbox,
        inbound_rx: conn_handle.inbound_rx,
        outbound_tx: conn_handle.outbound_tx.clone(),
    };

    let res = runner::run_session(driver_params).await;
    // Let the connection manager flush the final reports before the process exits
    shutdown_tx.send(true).ok();
    if let Some(task) = conn_task {
        match tokio::time::timeout(connection::FINAL_REPORT_FLUSH + FINAL_FLUSH_MARGIN, task).await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(error = %error, "connection manager failed during the final flush");
            }
            Err(_elapsed) => {
                tracing::warn!("connection manager did not finish the final flush in time");
            }
        }
    }

    match res {
        Ok(exec_res) if exec_res.outcome == df_proto::agent::v1::RunOutcome::Failed => {
            ExitCode::FAILURE
        }
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "run execution failed");
            ExitCode::FAILURE
        }
    }
}

/// Makes the outbox directory safe to open: refuses a symlinked directory, removes symlinks below
/// it and replaces hard-linked files by copies. `false` after a logged failure.
fn clean_outbox_aliases(dir: &Path) -> bool {
    let links = match sandbox::aliases::secure_outbox_dir(dir) {
        Ok(links) => links,
        Err(e) => {
            tracing::error!(error = format!("{e:#}"), "refusing the runtime directory");
            return false;
        }
    };
    let copies = match sandbox::aliases::break_hard_links(dir) {
        Ok(copies) => copies,
        Err(e) => {
            tracing::error!(error = %e, dir = %dir.display(), "failed to break hard links");
            return false;
        }
    };
    if links + copies > 0 {
        tracing::warn!(links, copies, "removed aliases below the runtime directory");
    }
    true
}

/// Creates the directories of the run. `None` after a logged failure.
fn create_run_dirs(layout: &config::Layout) -> Option<RunDirs> {
    let dirs = RunDirs {
        runtime: layout.run_dir.clone(),
        outbox_data: layout.outbox_dir(),
        repo: layout.repo_dir(),
        home: layout.home_dir(),
    };
    for dir in [&dirs.runtime, &dirs.outbox_data, &dirs.repo, &dirs.home] {
        if let Err(e) = std::fs::create_dir_all(dir) {
            tracing::error!(error = %e, dir = %dir.display(), "failed to create directory");
            return None;
        }
    }
    Some(dirs)
}

/// Opens the outbox. `None` after a logged failure.
fn open_outbox(outbox_data_dir: &Path) -> Option<Arc<Mutex<outbox::Outbox>>> {
    // An empty run id keeps the stored state; the control plane's HelloAck/Assign decide
    // whether it belongs to the current run.
    match outbox::Outbox::open(
        outbox_data_dir,
        "",
        outbox::DEFAULT_MAX_OUTBOX_BYTES,
        outbox::DEFAULT_MAX_SEGMENT_BYTES,
    ) {
        Ok(o) => Some(Arc::new(Mutex::new(o))),
        Err(e) => {
            tracing::error!(error = %e, "failed to open outbox");
            None
        }
    }
}

/// Collects control messages until `Assign`. `None` after a logged failure: no `Assign` within
/// [`ASSIGN_TIMEOUT`] (otherwise the pod would idle forever while holding the volume), or the
/// stream closed.
async fn receive_assignment(conn_handle: &mut connection::ConnectionHandle) -> Option<Startup> {
    use df_proto::agent::v1::control_message::Body;

    let mut assign = None;
    let mut initial_prompts = Vec::new();
    let mut cancel_requested = false;

    let wait_for_assign = async {
        while let Some(msg) = conn_handle.inbound_rx.recv().await {
            match msg.body {
                Some(Body::Assign(a)) => {
                    assign = Some(a);
                    break;
                }
                Some(Body::Cancel(_)) => cancel_requested = true,
                Some(Body::Prompts(batch)) => initial_prompts.extend(batch.prompts),
                _ => {}
            }
        }
    };
    if tokio::time::timeout(ASSIGN_TIMEOUT, wait_for_assign)
        .await
        .is_err()
    {
        tracing::error!(timeout = ?ASSIGN_TIMEOUT, "no Assign received from the control plane");
        return None;
    }
    let Some(assign) = assign else {
        tracing::error!("inbound stream closed without receiving Assign");
        return None;
    };
    Some(Startup {
        assign,
        initial_prompts,
        cancel_requested,
    })
}

/// Discards the stored outbox if it belongs to another run.
async fn discard_outbox_of_other_run(outbox: &Mutex<outbox::Outbox>, run_id: &str) {
    let mut ob = outbox.lock().await;
    if ob.run_state().run_id != run_id
        && let Err(e) = ob.discard(run_id)
    {
        tracing::error!(error = %e, "failed to discard outbox");
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
