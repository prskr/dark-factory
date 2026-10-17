//! Instance configuration (`InstanceConfig`) loaded from YAML (`--config`).
//!
//! Secrets are always referenced via file paths (`*_file`), never inline values.

use std::fs;
use std::path::{Path, PathBuf};

use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default path to the instance configuration YAML file.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/dark-factory/config.yaml";

/// Errors returned when loading or validating [`InstanceConfig`].
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid YAML configuration: {0}")]
    Yaml(#[from] serde_saphyr::Error),
    #[error("failed to read secret file `{path}`: {source}")]
    SecretFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Top-level instance configuration deserialized from `/etc/dark-factory/config.yaml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceConfig {
    #[serde(default = "default_instance_name")]
    pub instance_name: String,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub sandbox: SandboxSectionConfig,
}

impl InstanceConfig {
    /// Parses an [`InstanceConfig`] from a YAML string.
    pub fn from_yaml_str(yaml: &str) -> Result<Self, ConfigError> {
        Ok(serde_saphyr::from_str(yaml)?)
    }

    /// Reads and parses an [`InstanceConfig`] from a file path.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_yaml_str(&contents)
    }
}

/// Database connection and migration role configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatabaseConfig {
    pub host: String,
    #[serde(default = "default_db_port")]
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_file: PathBuf,
    #[serde(default = "default_db_schema")]
    pub schema: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default)]
    pub migrate: MigrateRoleConfig,
}

impl DatabaseConfig {
    /// Reads the database password from `password_file` (trimming trailing newlines).
    pub fn read_password(&self) -> Result<SecretString, ConfigError> {
        read_secret_file(&self.password_file)
    }
}

/// Optional application-role management settings used by `dark-factory migrate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrateRoleConfig {
    #[serde(default = "default_app_role")]
    pub app_role: String,
    #[serde(default)]
    pub manage_app_role: bool,
    #[serde(default)]
    pub app_password_file: Option<PathBuf>,
}

impl Default for MigrateRoleConfig {
    fn default() -> Self {
        Self {
            app_role: default_app_role(),
            manage_app_role: false,
            app_password_file: None,
        }
    }
}

impl MigrateRoleConfig {
    /// Reads the application role password from `app_password_file` if configured.
    pub fn read_app_password(&self) -> Result<Option<SecretString>, ConfigError> {
        self.app_password_file
            .as_deref()
            .map(read_secret_file)
            .transpose()
    }
}

/// Control-plane server listener ports (design D7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    #[serde(default = "default_agent_port")]
    pub agent_port: u16,
    #[serde(default = "default_internal_grpc_port")]
    pub internal_grpc_port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_port: default_http_port(),
            agent_port: default_agent_port(),
            internal_grpc_port: default_internal_grpc_port(),
        }
    }
}

/// Egress gateway listener ports (design D7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayConfig {
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    #[serde(default = "default_internal_grpc_port")]
    pub internal_grpc_port: u16,
    #[serde(default = "default_http_port")]
    pub health_port: u16,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            proxy_port: default_proxy_port(),
            internal_grpc_port: default_internal_grpc_port(),
            health_port: default_http_port(),
        }
    }
}

/// Sandbox namespace and baseline provisioning section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSectionConfig {
    #[serde(default = "default_sandbox_namespace")]
    pub namespace: String,
    #[serde(default = "default_instance_namespace")]
    pub instance_namespace: String,
}

impl Default for SandboxSectionConfig {
    fn default() -> Self {
        Self {
            namespace: default_sandbox_namespace(),
            instance_namespace: default_instance_namespace(),
        }
    }
}

/// Reads a secret from a file, trimming a single trailing `\r\n` or `\n`.
pub fn read_secret_file(path: &Path) -> Result<SecretString, ConfigError> {
    let raw = fs::read_to_string(path).map_err(|source| ConfigError::SecretFile {
        path: path.to_path_buf(),
        source,
    })?;
    let trimmed = raw
        .strip_suffix("\r\n")
        .or_else(|| raw.strip_suffix('\n'))
        .unwrap_or(&raw);
    Ok(SecretString::from(trimmed.to_owned()))
}

fn default_instance_name() -> String {
    "dark-factory".to_string()
}

fn default_db_port() -> u16 {
    5432
}

fn default_db_schema() -> String {
    "dark_factory".to_string()
}

fn default_max_connections() -> u32 {
    10
}

fn default_app_role() -> String {
    "dark_factory_app".to_string()
}

fn default_http_port() -> u16 {
    8080
}

fn default_agent_port() -> u16 {
    9443
}

fn default_internal_grpc_port() -> u16 {
    9444
}

fn default_proxy_port() -> u16 {
    3128
}

fn default_sandbox_namespace() -> String {
    "dark-factory-sandboxes".to_string()
}

fn default_instance_namespace() -> String {
    "dark-factory".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::io::Write;
    use tempfile::NamedTempFile;

    const VALID_FIXTURE: &str = r#"
instanceName: homelab-dev
database:
  host: pg.dark-factory.svc
  port: 5432
  database: dark_factory
  user: dark_factory_app
  passwordFile: /run/secrets/db-password
  schema: dark_factory
  maxConnections: 16
  migrate:
    appRole: dark_factory_app
    manageAppRole: true
    appPasswordFile: /run/secrets/app-password
server:
  httpPort: 8080
  agentPort: 9443
  internalGrpcPort: 9444
gateway:
  proxyPort: 3128
  internalGrpcPort: 9444
  healthPort: 8080
sandbox:
  namespace: dark-factory-sandboxes
  instanceNamespace: dark-factory
"#;

    #[test]
    fn loads_valid_instance_config_fixture_and_reads_secret_file() {
        let mut pw_file = NamedTempFile::new().unwrap();
        writeln!(pw_file, "s3cr3t-pass").unwrap();

        let yaml =
            VALID_FIXTURE.replace("/run/secrets/db-password", pw_file.path().to_str().unwrap());
        let mut cfg_file = NamedTempFile::new().unwrap();
        write!(cfg_file, "{yaml}").unwrap();

        let cfg = InstanceConfig::from_file(cfg_file.path()).expect("should load valid config");
        assert_eq!(cfg.instance_name, "homelab-dev");
        assert_eq!(cfg.database.host, "pg.dark-factory.svc");
        assert_eq!(cfg.database.port, 5432);
        assert_eq!(cfg.database.database, "dark_factory");
        assert_eq!(cfg.database.user, "dark_factory_app");
        assert_eq!(cfg.database.schema, "dark_factory");
        assert_eq!(cfg.database.max_connections, 16);
        assert!(cfg.database.migrate.manage_app_role);
        assert_eq!(cfg.server.http_port, 8080);
        assert_eq!(cfg.server.agent_port, 9443);
        assert_eq!(cfg.server.internal_grpc_port, 9444);
        assert_eq!(cfg.gateway.proxy_port, 3128);

        let secret = cfg.database.read_password().unwrap();
        assert_eq!(secret.expose_secret(), "s3cr3t-pass");
    }

    #[test]
    fn missing_required_field_yields_error_naming_field() {
        let missing_password_file = r#"
database:
  host: pg.dark-factory.svc
  database: dark_factory
  user: dark_factory_app
"#;
        let err = InstanceConfig::from_yaml_str(missing_password_file).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("passwordFile"),
            "expected error to name missing field `passwordFile`, got: {msg}"
        );
    }
}
