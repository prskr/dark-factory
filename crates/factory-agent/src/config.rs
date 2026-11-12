//! Environment configuration for `factory-agent`.

use std::path::PathBuf;

use anyhow::Context as _;
use df_core::LandlockMode;

/// Environment variables expected by the sandbox runtime.
pub const ENV_CONTROL_PLANE_ADDR: &str = "DF_CONTROL_PLANE_ADDR";
pub const ENV_CONTROL_PLANE_SERVER_NAME: &str = "DF_CONTROL_PLANE_SERVER_NAME";
pub const ENV_TLS_DIR: &str = "DF_TLS_DIR";
pub const ENV_DATA_DIR: &str = "DF_DATA_DIR";
pub const ENV_RUN_DIR: &str = "DF_RUN_DIR";
pub const ENV_LANDLOCK: &str = "DF_LANDLOCK";
pub const ENV_RUNTIME_SOCK: &str = "DF_RUNTIME_SOCK";
pub const ENV_LOG_FORMAT: &str = "DF_LOG_FORMAT";

pub const DEFAULT_TLS_DIR: &str = df_core::harness::SANDBOX_TLS_DIR;
pub const DEFAULT_DATA_DIR: &str = df_core::harness::SANDBOX_DATA_DIR;
pub const DEFAULT_RUN_DIR: &str = df_core::harness::SANDBOX_RUN_DIR;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub control_plane_addr: String,
    pub server_name: String,
    pub layout: Layout,
    pub landlock: LandlockMode,
}

/// The directories of the runtime. The `sandbox` subcommand reads them from the environment it
/// inherits, so both sides agree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub tls_dir: PathBuf,
    pub data_dir: PathBuf,
    pub run_dir: PathBuf,
}

impl Layout {
    pub fn from_env() -> Self {
        let dir = |name, default| {
            std::env::var(name).map_or_else(|_| PathBuf::from(default), PathBuf::from)
        };
        Self {
            tls_dir: dir(ENV_TLS_DIR, DEFAULT_TLS_DIR),
            data_dir: dir(ENV_DATA_DIR, DEFAULT_DATA_DIR),
            run_dir: dir(ENV_RUN_DIR, DEFAULT_RUN_DIR),
        }
    }

    /// Workspace checkout; writable for the harness.
    pub fn repo_dir(&self) -> PathBuf {
        self.data_dir.join("repo")
    }

    /// `$HOME` of the harness; writable for the harness.
    pub fn home_dir(&self) -> PathBuf {
        self.data_dir.join("home")
    }

    /// Outbox segments and `run.json`; the harness has no access.
    pub fn outbox_dir(&self) -> PathBuf {
        self.data_dir.join("runtime")
    }

    /// Runtime-maintained copy of `ca.crt`; read-only for the harness.
    pub fn ca_dir(&self) -> PathBuf {
        self.run_dir.join("ca")
    }

    /// The instance CA as the harness reads it.
    pub fn ca_file(&self) -> PathBuf {
        self.ca_dir().join("ca.crt")
    }
}

/// Parses the value of `DF_LANDLOCK`; unset means `enforce`.
pub fn parse_landlock(value: Option<&str>) -> anyhow::Result<LandlockMode> {
    value
        .map_or(Ok(LandlockMode::Enforce), str::parse)
        .with_context(|| format!("environment variable {ENV_LANDLOCK}"))
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

        let layout = Layout::from_env();
        let landlock = parse_landlock(std::env::var(ENV_LANDLOCK).ok().as_deref())?;

        Ok(Self {
            control_plane_addr: Self::normalize_addr(&control_plane_addr),
            server_name,
            layout,
            landlock,
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
            (ENV_LANDLOCK, false),
        ] {
            assert_eq!(documented(name), Some(required), "{name}");
        }
    }

    #[test]
    fn the_default_ca_copy_is_the_path_the_control_plane_hands_to_the_harness() {
        let layout = Layout {
            tls_dir: DEFAULT_TLS_DIR.into(),
            data_dir: DEFAULT_DATA_DIR.into(),
            run_dir: DEFAULT_RUN_DIR.into(),
        };
        assert_eq!(
            layout.ca_file(),
            std::path::Path::new(df_core::harness::INSTANCE_CA_PATH)
        );
    }

    #[test]
    fn the_default_writable_directories_are_the_ones_the_prompt_names() {
        let layout = Layout {
            tls_dir: DEFAULT_TLS_DIR.into(),
            data_dir: DEFAULT_DATA_DIR.into(),
            run_dir: DEFAULT_RUN_DIR.into(),
        };
        assert_eq!(
            layout.repo_dir(),
            std::path::Path::new(df_core::harness::SANDBOX_REPO_DIR)
        );
        assert_eq!(
            layout.home_dir(),
            std::path::Path::new(df_core::harness::SANDBOX_HOME_DIR)
        );
    }

    #[test]
    fn landlock_defaults_to_enforce_and_accepts_disabled() {
        assert_eq!(parse_landlock(None).unwrap(), LandlockMode::Enforce);
        assert_eq!(
            parse_landlock(Some("enforce")).unwrap(),
            LandlockMode::Enforce
        );
        assert_eq!(
            parse_landlock(Some("disabled")).unwrap(),
            LandlockMode::Disabled
        );
    }

    #[test]
    fn landlock_rejects_other_values_naming_the_variable() {
        let error = format!("{:#}", parse_landlock(Some("off")).unwrap_err());
        assert!(
            error.contains("DF_LANDLOCK") && error.contains("\"off\""),
            "{error}"
        );
    }
}
