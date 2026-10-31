//! `just seed`: writes the well-known test data into the database behind `DATABASE_URL`.

use anyhow::{Context, Result};
#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("E2E_DATABASE_URL")
        .or_else(|_unset| std::env::var("DATABASE_URL"))
        .context("set E2E_DATABASE_URL or DATABASE_URL")?;
    let pool = df_e2e::db::connect(&url).await?;
    df_e2e::seed::run(&pool).await
}
