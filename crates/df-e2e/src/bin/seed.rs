//! `just seed` and `just seed-demo`: write well-known data into the database behind
//! `E2E_DATABASE_URL` (or `DATABASE_URL`).

use anyhow::{Context, Result};
use clap::Parser;

/// Writes the end-to-end fixtures, or with `--demo` realistic demo data. Both are idempotent and
/// can share a database.
#[derive(Parser)]
struct Args {
    /// Write the demo profile (projects, personas and tasks in every state with transcripts and
    /// artifacts, for screenshots) instead of the end-to-end fixtures.
    #[arg(long)]
    demo: bool,
}

#[tokio::main]
#[expect(clippy::print_stderr, reason = "CLI output")]
async fn main() -> Result<()> {
    let args = Args::parse();
    let url = std::env::var("E2E_DATABASE_URL")
        .or_else(|_unset| std::env::var("DATABASE_URL"))
        .context("set E2E_DATABASE_URL or DATABASE_URL")?;
    let pool = df_e2e::db::connect(&url).await?;
    if args.demo {
        for hint in df_e2e::seed::demo::run(&pool).await?.hints {
            eprintln!("hint: {hint}");
        }
        Ok(())
    } else {
        df_e2e::seed::run(&pool).await
    }
}
