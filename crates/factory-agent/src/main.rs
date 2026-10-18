//! Dark Factory sandbox agent runtime (`factory-agent`).

use std::process::ExitCode;

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

#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum Command {
    /// Connect to the control plane and drive the sandbox run.
    Run,
    /// Stdio MCP server bridging factory tools to the runtime Unix socket.
    Mcp,
    /// Git credential helper bridging to the runtime Unix socket.
    GitCredential {
        /// Credential helper operation (`get`, `store`, `erase`).
        operation: Option<String>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();
    match cli.command {
        Command::Run | Command::Mcp | Command::GitCredential { .. } => ExitCode::SUCCESS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_subcommands() {
        let cli = Cli::try_parse_from(["factory-agent", "run"]).unwrap();
        assert_eq!(cli.command, Command::Run);

        let cli = Cli::try_parse_from(["factory-agent", "mcp"]).unwrap();
        assert_eq!(cli.command, Command::Mcp);

        let cli = Cli::try_parse_from(["factory-agent", "git-credential", "get"]).unwrap();
        assert_eq!(
            cli.command,
            Command::GitCredential {
                operation: Some("get".to_string())
            }
        );
    }
}
