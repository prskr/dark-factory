//! CLI argument parsing for `factory-agent`.

use clap::{Parser, Subcommand};

use df_telemetry::logging::LogFormat;

#[derive(Debug, Parser)]
#[command(
    name = "factory-agent",
    version,
    about = "Dark Factory sandbox runtime"
)]
pub struct Cli {
    /// Log output format.
    #[arg(
        long,
        global = true,
        env = crate::config::ENV_LOG_FORMAT,
        value_name = crate::config::ENV_LOG_FORMAT,
        value_enum,
        default_value = "text"
    )]
    pub log_format: LogFormat,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand, PartialEq, Eq, Clone)]
pub enum Command {
    /// Connect to the control plane and drive the sandbox run.
    Run,
    /// Stdio MCP server bridging factory tools to the runtime Unix socket.
    Mcp {
        /// Path to runtime Unix domain socket.
        #[arg(
            long,
            env = crate::config::ENV_RUNTIME_SOCK,
            default_value = df_core::harness::SANDBOX_RUNTIME_SOCK
        )]
        socket: std::path::PathBuf,
    },
    /// Applies the harness's Landlock domain to itself, then replaces itself with the command.
    /// Started by `run`; exits with 125 when the domain can't be applied.
    Sandbox {
        /// Directory whose entries get read and execute access (tests use a fake root).
        #[arg(long, hide = true, default_value = "/")]
        root: std::path::PathBuf,
        /// The command to run inside the domain, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<std::ffi::OsString>,
    },
}
