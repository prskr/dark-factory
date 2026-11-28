//! Scripted fake ACP agent binary for deterministic tests (`fake-acp-agent`).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

pub mod agent;
pub mod mcp_upstream;
pub mod mock_mcp;
pub mod model;
pub mod prompt_vars;
pub mod script;
pub mod upstream;

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

    /// Serve a fake Anthropic upstream on this address (end-to-end cluster); requests need the
    /// key in `FAKE_UPSTREAM_KEY`.
    #[arg(long, value_name = "ADDRESS", env = "FAKE_UPSTREAM_ADDRESS")]
    pub serve_fake_anthropic: Option<String>,

    /// The key the fake upstream expects in `x-api-key`.
    #[arg(
        long,
        env = "FAKE_UPSTREAM_KEY",
        default_value = "fake-key",
        hide_env_values = true
    )]
    pub fake_upstream_key: String,

    /// Serve a fake third-party MCP server on this address (end-to-end cluster); requests need
    /// `Authorization: Bearer <FAKE_MCP_TOKEN>`.
    #[arg(long, value_name = "ADDRESS", env = "FAKE_MCP_ADDRESS")]
    pub serve_fake_mcp: Option<String>,

    /// The bearer token the fake MCP server expects.
    #[arg(
        long,
        env = "FAKE_MCP_TOKEN",
        default_value = "fake-mcp-token",
        hide_env_values = true
    )]
    pub fake_mcp_token: String,
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

    if let Some(address) = &cli.serve_fake_anthropic {
        if let Err(err) = upstream::serve(address, cli.fake_upstream_key.clone()).await {
            eprintln!("fake-acp-agent: fake upstream: {err:#}");
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }

    if let Some(address) = &cli.serve_fake_mcp {
        if let Err(err) = mcp_upstream::serve(address, cli.fake_mcp_token.clone()).await {
            eprintln!("fake-acp-agent: fake MCP server: {err:#}");
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
