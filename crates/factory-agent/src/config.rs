//! Environment configuration for `factory-agent`.

use std::path::PathBuf;

/// Environment variables expected by the sandbox runtime.
pub const ENV_CONTROL_PLANE_ADDR: &str = "DF_CONTROL_PLANE_ADDR";
pub const ENV_CONTROL_PLANE_SERVER_NAME: &str = "DF_CONTROL_PLANE_SERVER_NAME";
pub const ENV_TLS_DIR: &str = "DF_TLS_DIR";
pub const ENV_DATA_DIR: &str = "DF_DATA_DIR";
pub const ENV_RUN_DIR: &str = "DF_RUN_DIR";

pub const DEFAULT_TLS_DIR: &str = "/run/factory/tls";
pub const DEFAULT_DATA_DIR: &str = "/data";
pub const DEFAULT_RUN_DIR: &str = "/run/factory";

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub control_plane_addr: String,
    pub server_name: String,
    pub tls_dir: PathBuf,
    pub data_dir: PathBuf,
    pub run_dir: PathBuf,
}

impl RuntimeConfig {
    /// Load runtime configuration from environment variables.
    pub fn from_env() -> anyhow::Result<Self> {
        let control_plane_addr = std::env::var(ENV_CONTROL_PLANE_ADDR).map_err(|_not_set| {
            anyhow::anyhow!("missing required environment variable {ENV_CONTROL_PLANE_ADDR}")
        })?;
        let server_name = std::env::var(ENV_CONTROL_PLANE_SERVER_NAME).map_err(|_not_set| {
            anyhow::anyhow!("missing required environment variable {ENV_CONTROL_PLANE_SERVER_NAME}")
        })?;

        let tls_dir = std::env::var(ENV_TLS_DIR)
            .map_or_else(|_| PathBuf::from(DEFAULT_TLS_DIR), PathBuf::from);
        let data_dir = std::env::var(ENV_DATA_DIR)
            .map_or_else(|_| PathBuf::from(DEFAULT_DATA_DIR), PathBuf::from);
        let run_dir = std::env::var(ENV_RUN_DIR)
            .map_or_else(|_| PathBuf::from(DEFAULT_RUN_DIR), PathBuf::from);

        Ok(Self {
            control_plane_addr: Self::normalize_addr(&control_plane_addr),
            server_name,
            tls_dir,
            data_dir,
            run_dir,
        })
    }

    /// Normalizes the control plane address to have a URL scheme (`https://`).
    pub fn normalize_addr(addr: &str) -> String {
        if addr.starts_with("http://") || addr.starts_with("https://") {
            addr.to_string()
        } else {
            format!("https://{addr}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// df-k8s checks the sandbox pod against this table, so it must match what `from_env` reads.
    const REFERENCE: &str = include_str!("../docs/configuration.md");

    fn documented(name: &str) -> Option<bool> {
        REFERENCE.lines().find_map(|line| {
            let mut cells = line.split('|').map(str::trim).skip(1);
            (cells.next()? == format!("`{name}`")).then(|| cells.next() == Some("yes"))
        })
    }

    #[test]
    fn configuration_reference_lists_every_variable_with_its_required_flag() {
        for (name, required) in [
            (ENV_CONTROL_PLANE_ADDR, true),
            (ENV_CONTROL_PLANE_SERVER_NAME, true),
            (ENV_TLS_DIR, false),
            (ENV_DATA_DIR, false),
            (ENV_RUN_DIR, false),
        ] {
            assert_eq!(documented(name), Some(required), "{name}");
        }
    }
}
