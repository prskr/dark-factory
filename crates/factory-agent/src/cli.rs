//! CLI argument parsing for `factory-agent`.

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "factory-agent",
    version,
    about = "Dark Factory sandbox runtime"
)]
pub struct Cli {
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
    /// Git credential helper bridging to the runtime Unix socket.
    GitCredential {
        /// Credential helper operation (`get`, `store`, `erase`).
        operation: Option<String>,
        /// Path to runtime Unix domain socket.
        #[arg(
            long,
            env = "DF_RUNTIME_SOCK",
            default_value = "/run/factory/runtime.sock"
        )]
        socket: std::path::PathBuf,
    },
}
