//! Dark Factory control plane, database migration, and egress gateway binary.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use df_core::InstanceConfig;
use df_core::config::DEFAULT_CONFIG_PATH;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "dark-factory",
    version,
    about = "Dark Factory control plane and gateway"
)]
pub struct Cli {
    /// Path to the instance configuration YAML file.
    #[arg(
        long,
        short = 'c',
        global = true,
        env = "DARK_FACTORY_CONFIG",
        default_value = DEFAULT_CONFIG_PATH
    )]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the Dark Factory control plane server.
    Serve,
    /// Apply embedded PostgreSQL migrations, optional application role management, and grants.
    Migrate,
    /// Run the Dark Factory sandbox egress gateway.
    Gateway,
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();
    let cli = Cli::parse();

    match cli.command {
        Command::Serve => {
            error!(config = %cli.config.display(), "not implemented");
            ExitCode::from(1)
        }
        Command::Migrate => match run_migrate_command(&cli.config).await {
            Ok(()) => {
                info!("database migration completed successfully");
                ExitCode::SUCCESS
            }
            Err(err) => {
                error!(error = %err, "database migration failed");
                ExitCode::from(1)
            }
        },
        Command::Gateway => {
            error!(config = %cli.config.display(), "not implemented");
            ExitCode::from(1)
        }
    }
}

async fn run_migrate_command(config_path: &std::path::Path) -> anyhow::Result<()> {
    let cfg = InstanceConfig::from_file(config_path)?;
    df_db::run_migrate(&cfg.database).await?;
    Ok(())
}
