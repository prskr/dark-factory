//! Scripted fake ACP agent binary for deterministic tests (`fake-acp-agent`).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "fake-acp-agent", version, about = "Scripted fake ACP agent")]
pub struct Cli {
    /// Optional path to a YAML/JSON script describing the session turns.
    #[arg(long, env = "FAKE_ACP_SCRIPT")]
    pub script: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let _cli = Cli::parse();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_flags() {
        let cli = Cli::try_parse_from(["fake-acp-agent", "--script", "/tmp/script.yaml"]).unwrap();
        assert_eq!(cli.script, Some(PathBuf::from("/tmp/script.yaml")));
    }
}
