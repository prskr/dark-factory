//! Instance configuration (`InstanceConfig`) loaded from YAML (`--config`).
//!
//! Secrets are always referenced via file paths (`*_file`), never inline values.

use std::collections::BTreeMap;
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
    #[serde(default)]
    pub cluster_targets: ClusterTargetsConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub harness: HarnessConfig,
    #[serde(default)]
    pub runs: RunsConfig,
    /// Integration plugins by name (`forgejo`, ...).
    #[serde(default)]
    pub plugins: BTreeMap<String, PluginConfig>,
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

/// Delivery mechanism for the factory agent runtime binaries into the sandbox.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeDelivery {
    /// OCI image volume mounted at `/opt/factory` (read-only).
    #[default]
    ImageVolume,
    /// Init container copying binaries into an `emptyDir` at `/opt/factory`.
    InitContainer,
}

/// Namespaces in which sandboxes may be granted egress to cluster Services. Mirrors the chart's
/// `clusterTargets` value and therefore the RBAC the control plane has: it watches Services and
/// EndpointSlices exactly there.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClusterTargetsConfig {
    /// The control plane may read Services in every namespace (ClusterRole).
    pub any_namespace: bool,
    /// Namespaces it may read when `any_namespace` is `false`.
    pub namespaces: Vec<String>,
}

/// The `sandbox` configuration section: provisioning settings plus the (optional) addresses of
/// the control plane and gateway. Both parts are flattened into one YAML table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSectionConfig {
    #[serde(flatten)]
    pub provisioning: SandboxProvisioningConfig,
    #[serde(flatten)]
    pub endpoints: SandboxEndpointsConfig,
}

/// Where sandboxes run and how they are provisioned. Handed to the Kubernetes provider as is.
///
/// This is the single place for provisioning defaults: every key missing from the YAML takes
/// its value from [`Default`]. `storageClass` and `runtimeClassName` default to `None` (empty
/// string counts as unset) and then fall back to the cluster's default StorageClass and runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SandboxProvisioningConfig {
    /// Namespace holding sandboxes and task PVCs.
    pub namespace: String,
    /// Namespace of the control plane release (Leases, Services, Secrets).
    pub instance_namespace: String,
    pub service_account_name: String,
    #[serde(deserialize_with = "empty_string_is_none")]
    pub storage_class: Option<String>,
    pub pvc_size: String,
    #[serde(deserialize_with = "empty_string_is_none")]
    pub runtime_class_name: Option<String>,
    pub issuer_name: String,
    pub issuer_kind: String,
    pub issuer_group: String,
    pub certificate_duration: String,
    pub certificate_renew_before: String,
    pub runtime_image: String,
    pub runtime_delivery: RuntimeDelivery,
    pub cluster_domain: String,
}

/// Optional overrides for the addresses sandboxes use to reach the control plane and the
/// gateway. They are only read at startup to resolve the final values, which then live in the
/// Kubernetes provider's configuration and nowhere else.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SandboxEndpointsConfig {
    /// Fixed control plane ClusterIP; resolved from the Service when unset.
    pub control_plane_ip: Option<std::net::IpAddr>,
    /// Fixed gateway ClusterIP; resolved from the Service when unset.
    pub gateway_ip: Option<std::net::IpAddr>,
    /// Service name used to look up the control plane ClusterIP. Without it, `dark-factory`
    /// tries `<instanceName>-control-plane` and `dark-factory-control-plane`.
    pub control_plane_service_name: Option<String>,
    /// Service name used to look up the gateway ClusterIP. Without it, `dark-factory` tries
    /// `<instanceName>-gateway`, `<instanceName>-egress-gateway` and `dark-factory-gateway`.
    pub gateway_service_name: Option<String>,
    /// TLS server name of the control plane agent endpoint; defaults to
    /// `<instanceName>-control-plane`.
    pub control_plane_tls_server_name: Option<String>,
}

fn empty_string_is_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.trim().is_empty()))
}

impl Default for SandboxProvisioningConfig {
    fn default() -> Self {
        Self {
            namespace: default_sandbox_namespace(),
            instance_namespace: default_instance_namespace(),
            service_account_name: "dark-factory-sandbox".to_string(),
            storage_class: None,
            pvc_size: "20Gi".to_string(),
            runtime_class_name: None,
            issuer_name: "dark-factory-ca-issuer".to_string(),
            issuer_kind: "ClusterIssuer".to_string(),
            issuer_group: "cert-manager.io".to_string(),
            certificate_duration: "6h".to_string(),
            certificate_renew_before: "2h".to_string(),
            runtime_image: "ghcr.io/prskr/dark-factory/runtime:0.1.0".to_string(),
            runtime_delivery: RuntimeDelivery::default(),
            cluster_domain: "cluster.local".to_string(),
        }
    }
}

/// Agent port settings: server certificate, commit identity and Git credential user name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentConfig {
    /// Server certificate and client CA for the mTLS agent port. Required by `serve`.
    #[serde(default)]
    pub tls: Option<AgentTlsConfig>,
    #[serde(default)]
    pub commit_identity: CommitIdentityConfig,
    /// User name sent with the project's forge token in a Git credential grant.
    #[serde(default = "default_git_username")]
    pub git_username: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            tls: None,
            commit_identity: CommitIdentityConfig::default(),
            git_username: default_git_username(),
        }
    }
}

/// PEM files of the agent port: server certificate chain, its key and the CA bundle that
/// signs the sandbox pods' client certificates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTlsConfig {
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
    pub client_ca_file: PathBuf,
}

/// Author and committer identity of commits made by agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitIdentityConfig {
    #[serde(default = "default_commit_name")]
    pub name: String,
    #[serde(default = "default_commit_email")]
    pub email: String,
}

impl Default for CommitIdentityConfig {
    fn default() -> Self {
        Self {
            name: default_commit_name(),
            email: default_commit_email(),
        }
    }
}

/// Instance-wide harness defaults; persona fields override them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HarnessConfig {
    #[serde(default)]
    pub claude_code: ClaudeCodeConfig,
}

/// Defaults for the `claude-code` harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaudeCodeConfig {
    #[serde(default = "default_adapter_command")]
    pub adapter_command: Vec<String>,
    /// Non-secret environment, e.g. `DISABLE_AUTOUPDATER=1`.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Claude Code `settings.json` content.
    #[serde(default)]
    pub settings_json: String,
}

impl Default for ClaudeCodeConfig {
    fn default() -> Self {
        Self {
            adapter_command: default_adapter_command(),
            env: BTreeMap::new(),
            settings_json: String::new(),
        }
    }
}

/// Run budgets and timeouts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunsConfig {
    /// Hard limit handed to the runtime in `Assign.budgets`.
    #[serde(default = "default_max_wall_clock")]
    pub max_wall_clock_seconds: u64,
    /// Time from run creation until the runtime must have opened its stream.
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_seconds: u64,
    /// Extra time after the wall-clock budget before `Cancel`, and again before teardown.
    #[serde(default = "default_cancel_grace")]
    pub cancel_grace_seconds: u64,
}

impl Default for RunsConfig {
    fn default() -> Self {
        Self {
            max_wall_clock_seconds: default_max_wall_clock(),
            connect_timeout_seconds: default_connect_timeout(),
            cancel_grace_seconds: default_cancel_grace(),
        }
    }
}

/// One integration plugin: its component, per-call limits and free-form settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginConfig {
    /// Path of the compiled component.
    pub path: PathBuf,
    /// Upper bound of a single plugin call, retries excluded.
    #[serde(default = "default_plugin_call_timeout")]
    pub call_timeout_seconds: u64,
    /// Linear memory limit of a plugin instance.
    #[serde(default = "default_plugin_memory_mib")]
    pub memory_mib: u64,
    /// Non-secret settings the plugin reads through `wasi:config/store`. The host does not
    /// interpret them; the plugin documents its keys. `base-url` is set by the host and wins.
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

impl PluginConfig {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            call_timeout_seconds: default_plugin_call_timeout(),
            memory_mib: default_plugin_memory_mib(),
            settings: BTreeMap::new(),
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

fn default_git_username() -> String {
    "dark-factory".to_string()
}

fn default_commit_name() -> String {
    "Dark Factory".to_string()
}

fn default_commit_email() -> String {
    "dark-factory@localhost".to_string()
}

fn default_adapter_command() -> Vec<String> {
    vec!["/opt/harness/bin/claude-agent-acp".to_string()]
}

fn default_plugin_call_timeout() -> u64 {
    30
}

fn default_plugin_memory_mib() -> u64 {
    64
}

fn default_max_wall_clock() -> u64 {
    2 * 60 * 60
}

fn default_connect_timeout() -> u64 {
    10 * 60
}

fn default_cancel_grace() -> u64 {
    5 * 60
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
    fn plugins_are_a_map_with_free_form_settings() {
        let yaml = format!(
            "{VALID_FIXTURE}plugins:\n  forgejo:\n    path: /p.wasm\n    callTimeoutSeconds: 5\n    settings:\n      wip-prefixes: 'WIP:'\n  other:\n    path: /o.wasm\n"
        );
        let cfg = InstanceConfig::from_yaml_str(&yaml).unwrap();
        let forgejo = &cfg.plugins["forgejo"];
        assert_eq!(forgejo.path, PathBuf::from("/p.wasm"));
        assert_eq!((forgejo.call_timeout_seconds, forgejo.memory_mib), (5, 64));
        assert_eq!(forgejo.settings["wip-prefixes"], "WIP:");
        assert!(cfg.plugins["other"].settings.is_empty());
        assert!(
            InstanceConfig::from_yaml_str(VALID_FIXTURE)
                .unwrap()
                .plugins
                .is_empty()
        );
    }

    #[test]
    fn cluster_targets_default_to_no_namespaces() {
        let cfg: ClusterTargetsConfig = serde_saphyr::from_str("namespaces: [a, b]").unwrap();
        assert!(!cfg.any_namespace);
        assert_eq!(cfg.namespaces, ["a", "b"]);
        assert_eq!(ClusterTargetsConfig::default().namespaces.len(), 0);
    }

    #[test]
    fn sandbox_section_defaults_leave_runtime_class_and_storage_class_unset() {
        let section: SandboxSectionConfig = serde_saphyr::from_str("namespace: sb").unwrap();
        let p = &section.provisioning;
        assert_eq!(p.namespace, "sb");
        assert_eq!(p.storage_class, None);
        assert_eq!(p.runtime_class_name, None);
        assert_eq!(p.pvc_size, "20Gi");
        assert_eq!(p.issuer_kind, "ClusterIssuer");
        assert_eq!(p.runtime_delivery, RuntimeDelivery::ImageVolume);
        assert_eq!(section.endpoints, SandboxEndpointsConfig::default());
        assert_eq!(
            section.provisioning,
            SandboxProvisioningConfig {
                namespace: "sb".into(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn sandbox_section_empty_strings_mean_unset() {
        let section: SandboxSectionConfig =
            serde_saphyr::from_str("storageClass: \"\"\nruntimeClassName: \"  \"").unwrap();
        assert_eq!(section.provisioning.storage_class, None);
        assert_eq!(section.provisioning.runtime_class_name, None);

        let section: SandboxSectionConfig =
            serde_saphyr::from_str("storageClass: fast\nruntimeClassName: kata").unwrap();
        assert_eq!(section.provisioning.storage_class.as_deref(), Some("fast"));
        assert_eq!(
            section.provisioning.runtime_class_name.as_deref(),
            Some("kata")
        );
    }

    #[test]
    fn sandbox_section_reads_provisioning_and_endpoint_keys_from_one_table() {
        let section: SandboxSectionConfig = serde_saphyr::from_str(
            "namespace: sb\ncontrolPlaneIp: 10.0.0.1\ngatewayServiceName: gw\ncontrolPlaneTlsServerName: cp.local",
        )
        .unwrap();
        assert_eq!(section.provisioning.namespace, "sb");
        assert_eq!(
            section.endpoints.control_plane_ip,
            Some("10.0.0.1".parse().unwrap())
        );
        assert_eq!(
            section.endpoints.gateway_service_name.as_deref(),
            Some("gw")
        );
        assert_eq!(
            section.endpoints.control_plane_tls_server_name.as_deref(),
            Some("cp.local")
        );
    }

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
    fn run_orchestration_sections_have_defaults_and_can_be_overridden() {
        let minimal = r#"
database:
  host: pg
  database: d
  user: u
  passwordFile: /p
"#;
        let cfg = InstanceConfig::from_yaml_str(minimal).unwrap();
        assert!(cfg.agent.tls.is_none());
        assert_eq!(cfg.agent.commit_identity.name, "Dark Factory");
        assert_eq!(cfg.agent.git_username, "dark-factory");
        assert_eq!(cfg.runs.max_wall_clock_seconds, 7200);
        assert_eq!(cfg.runs.connect_timeout_seconds, 600);
        assert_eq!(cfg.runs.cancel_grace_seconds, 300);
        assert_eq!(
            cfg.harness.claude_code.adapter_command,
            ["/opt/harness/bin/claude-agent-acp"]
        );

        let custom = format!(
            r#"{minimal}
agent:
  tls:
    certFile: /tls/tls.crt
    keyFile: /tls/tls.key
    clientCaFile: /tls/ca.crt
runs:
  maxWallClockSeconds: 60
harness:
  claudeCode:
    env:
      DISABLE_AUTOUPDATER: "1"
"#
        );
        let cfg = InstanceConfig::from_yaml_str(&custom).unwrap();
        let tls = cfg.agent.tls.unwrap();
        assert_eq!(tls.client_ca_file, PathBuf::from("/tls/ca.crt"));
        assert_eq!(cfg.runs.max_wall_clock_seconds, 60);
        assert_eq!(cfg.runs.connect_timeout_seconds, 600);
        assert_eq!(cfg.harness.claude_code.env["DISABLE_AUTOUPDATER"], "1");
    }

    #[test]
    fn typos_in_agent_and_harness_sections_are_rejected() {
        let base = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";
        for section in [
            "agent:\n  gitUserName: bot\n",
            "harness:\n  claude_code:\n    env: {}\n",
        ] {
            let err = InstanceConfig::from_yaml_str(&format!("{base}{section}")).unwrap_err();
            assert!(
                err.to_string().contains("unknown field"),
                "{section}: {err}"
            );
        }
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
