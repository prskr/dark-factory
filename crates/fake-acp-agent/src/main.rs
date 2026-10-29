//! Scripted fake ACP agent binary for deterministic tests (`fake-acp-agent`).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

pub mod agent;
pub mod mock_mcp;
pub mod script;

#[cfg(test)]
mod tests;

#[derive(Debug, Parser)]
#[command(name = "fake-acp-agent", version, about = "Scripted fake ACP agent")]
pub struct Cli {
    /// Optional path to a YAML/JSON script describing the session turns.
    #[arg(long, env = "FAKE_ACP_SCRIPT")]
    pub script: Option<PathBuf>,

    /// Run as a mock stdio MCP server for testing tool forwarding.
    #[arg(long)]
    pub mock_mcp_server: bool,
}

#[tokio::main]
#[expect(clippy::print_stderr, reason = "CLI output")]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.mock_mcp_server {
        if let Err(err) = mock_mcp::run_stdio().await {
            eprintln!("mock-mcp-server error: {err}");
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }

    let script = match cli.script {
        Some(path) => match script::Script::load_from_path(&path) {
            Ok(s) => Some(s),
            Err(err) => {
                eprintln!(
                    "fake-acp-agent: failed to load script from {}: {err}",
                    path.display()
                );
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };

    if let Err(err) = agent::run_stdio(script).await {
        eprintln!("fake-acp-agent error: {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn parses_cli_flags() {
        let cli = Cli::try_parse_from(["fake-acp-agent", "--script", "/tmp/script.yaml"]).unwrap();
        assert_eq!(cli.script, Some(PathBuf::from("/tmp/script.yaml")));
    }
}
