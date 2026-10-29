//! The control plane's [`ForgeClient`]: a forge plugin, configured from the projects table.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use async_trait::async_trait;
use df_core::{ForgeClient, IntegrationError, PluginConfig, ProjectId, SecretStore};
use df_db::PgPool;
use df_plugins::{ForgeTarget, PluginForgeClient, PluginHost, ProjectDirectory};

const MIB: u64 = 1024 * 1024;

/// Loads the forge plugin `name`; fails naming the configured path if the component is missing or
/// invalid, so a misconfigured control plane doesn't start. Needs no database, so `serve` calls it
/// before connecting.
///
/// Fail-fast is the behaviour for the single supported plugin. Once several plugins can be
/// configured, startup should only fail if no forge plugin could be loaded at all.
pub(crate) fn load_forge_plugin(name: &str, config: &PluginConfig) -> anyhow::Result<PluginHost> {
    let memory = config
        .memory_mib
        .checked_mul(MIB)
        .and_then(|bytes| usize::try_from(bytes).ok())
        .with_context(|| format!("`plugins.{name}.memoryMib` is too large"))?;
    PluginHost::load(
        name,
        &config.path,
        Duration::from_secs(config.call_timeout_seconds),
        memory,
    )
    .with_context(|| {
        format!(
            "cannot load the `{name}` plugin from `{}`",
            config.path.display()
        )
    })
}

/// The [`ForgeClient`] that calls `host` for the projects in `pool`.
pub(crate) fn forge_client(
    host: PluginHost,
    config: &PluginConfig,
    pool: PgPool,
    secrets: Arc<dyn SecretStore>,
) -> Arc<dyn ForgeClient> {
    Arc::new(PluginForgeClient::new(
        host,
        Arc::new(DbProjects { pool }),
        secrets,
        config.settings.clone(),
    ))
}

struct DbProjects {
    pool: PgPool,
}

#[async_trait]
impl ProjectDirectory for DbProjects {
    async fn forge_target(&self, project: &ProjectId) -> Result<ForgeTarget, IntegrationError> {
        let target = df_db::forge_target(&self.pool, project)
            .await
            .map_err(|e| IntegrationError::Internal(format!("reading project: {e}")))?
            .ok_or_else(|| IntegrationError::NotFound(format!("project {project}")))?;
        Ok(ForgeTarget {
            repo_url: target.repo_url,
            credential: target.credential,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_limit_overflow_is_a_config_error() {
        let mut config = PluginConfig::new("/nonexistent/forgejo.wasm");
        config.memory_mib = u64::MAX;
        let err = load_forge_plugin("forgejo", &config)
            .err()
            .expect("must fail");
        assert!(
            format!("{err:#}").contains("plugins.forgejo.memoryMib"),
            "{err:#}"
        );
    }

    #[test]
    fn missing_component_is_reported_with_its_path() {
        let config = PluginConfig::new("/nonexistent/forgejo.wasm");
        let err = load_forge_plugin("forgejo", &config)
            .err()
            .expect("must fail");
        let message = format!("{err:#}");
        assert!(message.contains("/nonexistent/forgejo.wasm"), "{message}");
        assert!(message.contains("`forgejo`"), "{message}");
    }
}
