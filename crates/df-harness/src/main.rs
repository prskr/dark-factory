//! Spike: ACP server over stdio that runs the `rpi-agent` loop against an
//! OpenAI-compatible endpoint (`openspec/changes/archive/2026-10-27-native-harness-spike`).

use clap::Parser;

mod agent;
mod mcp;

#[derive(Debug, Parser)]
#[command(name = "df-harness", version, about = "Native ACP harness (spike)")]
pub struct Cli {
    /// Base URL of the OpenAI-compatible endpoint.
    #[arg(long, env = "DF_HARNESS_BASE_URL")]
    pub base_url: String,
    /// API key for the endpoint.
    #[arg(long, env = "DF_HARNESS_API_KEY", hide_env_values = true)]
    pub api_key: String,
    /// Model id.
    #[arg(long, env = "DF_HARNESS_MODEL")]
    pub model: String,
    /// Whether the model emits reasoning tokens.
    #[arg(long, env = "DF_HARNESS_REASONING", default_value_t = true, action = clap::ArgAction::Set)]
    pub reasoning: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    agent::run_stdio(Cli::parse()).await
}
