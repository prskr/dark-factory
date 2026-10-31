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
        env = "DF_LOG_FORMAT",
        value_name = "DF_LOG_FORMAT",
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
            env = "DF_RUNTIME_SOCK",
            default_value = "/run/factory/runtime.sock"
        )]
        socket: std::path::PathBuf,
    },
}
