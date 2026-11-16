//! `setup`: what `just e2e-local` prepares before the tests, with the names of the suite's
//! fixtures. Prints JSON on stdout for the script (`jq`); logs and errors go to stderr.

use std::path::Path;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Creates the users and their tokens, the repositories, the issue, the hidden file, the
    /// collaborator and the webhooks; prints `{"forgejo-token": …, "reviewer-token": …}`. Safe to
    /// run again: users are kept, their `e2e` token and the repositories are replaced.
    Forgejo {
        /// Forgejo as the test host reaches it.
        #[arg(long, env = "E2E_FORGEJO_URL")]
        url: String,
        /// Admin user the Forgejo chart created.
        #[arg(long, env = "E2E_FORGEJO_ADMIN_USER")]
        admin_user: String,
        #[arg(long, env = "E2E_FORGEJO_ADMIN_PASSWORD", hide_env_values = true)]
        admin_password: String,
        /// Where the repository webhooks post to (the control plane, as Forgejo reaches it).
        #[arg(long)]
        webhook_url: String,
    },
    /// Checks that the Playwright versions of the browser server image, its base image and the
    /// test driver agree (run from the repository root); prints
    /// `{"playwright": …, "node": …}` for the driver install.
    PlaywrightVersions,
}

#[tokio::main]
async fn main() -> Result<()> {
    match Args::parse().command {
        Command::Forgejo {
            url,
            admin_user,
            admin_password,
            webhook_url,
        } => print_json(
            &df_e2e::setup::forgejo::run(&url, &admin_user, &admin_password, &webhook_url).await?,
        ),
        Command::PlaywrightVersions => print_json(&df_e2e::setup::versions::read(Path::new("."))?),
    }
}

#[expect(clippy::print_stdout, reason = "CLI output")]
fn print_json(value: &impl Serialize) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(value).context("encode the output")?
    );
    Ok(())
}
