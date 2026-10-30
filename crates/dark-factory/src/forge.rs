//! Loads every configured forge plugin and routes [`ForgeClient`] calls through [`ForgeRouter`].

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use async_trait::async_trait;
use df_core::routing::{ForgeRoutes, RoutingTable};
use df_core::{ForgeClient, IntegrationError, PluginConfig, ProjectId, SecretStore};
use df_db::PgPool;
use df_plugins::{
    ForgeRouter, ForgeTarget, PluginEngine, PluginForgeClient, PluginHost, ProjectDirectory,
};

const MIB: u64 = 1024 * 1024;
/// The plugin entry used when `plugins` is empty: the Forgejo component of the image.
const IMPLICIT_PLUGIN: &str = "forgejo";
const IMPLICIT_PLUGIN_PATH: &str = "/plugins/forgejo/plugin.wasm";

/// The validated routing table and the forge plugins that loaded, by name.
pub(crate) struct ForgePlugins {
    routes: RoutingTable,
    loaded: BTreeMap<String, (PluginHost, PluginConfig)>,
}

/// Validates the routes of `plugins` (with no entry: the implicit Forgejo default), then loads
/// every plugin (design D3, D6). A plugin that does not load is logged and skipped; its hosts stay
/// routed to it. Fails on a configuration error, before loading, or if no forge plugin loaded,
/// listing every load error. Needs no database, so `serve` calls it before connecting.
pub(crate) fn load_forge_plugins(
    engine: &PluginEngine,
    plugins: &BTreeMap<String, PluginConfig>,
) -> anyhow::Result<ForgePlugins> {
    let implicit;
    let plugins = if plugins.is_empty() {
        implicit = BTreeMap::from([(
            IMPLICIT_PLUGIN.to_string(),
            PluginConfig::new(IMPLICIT_PLUGIN_PATH),
        )]);
        &implicit
    } else {
        plugins
    };
    let routes = RoutingTable::from_plugins(plugins).context("invalid `plugins` configuration")?;
    let mut loaded = BTreeMap::new();
    let mut errors = Vec::new();
    for (name, config) in plugins {
        match load_forge_plugin(engine, name, config) {
            Ok(host) => {
                loaded.insert(name.clone(), (host, config.clone()));
            }
            Err(error) => {
                let error = format!("{error:#}");
                tracing::error!(
                    plugin = %name,
                    path = %config.path.display(),
                    error = %error,
                    "forge plugin not loaded"
                );
                errors.push(error);
            }
        }
    }
    if loaded.is_empty() {
        anyhow::bail!("no forge plugin loaded: {}", errors.join("; "));
    }
    let plugins = ForgePlugins { routes, loaded };
    plugins.log_routes();
    Ok(plugins)
}

impl ForgePlugins {
    /// The routing table with the loaded plugins, for the web UI (design D7).
    pub(crate) fn forge_routes(&self) -> ForgeRoutes {
        ForgeRoutes::new(self.routes.clone(), self.loaded.keys().cloned())
    }

    fn log_routes(&self) {
        for (host, route) in self.routes.hosts() {
            tracing::info!(
                host,
                plugin = %route.plugin,
                available = self.loaded.contains_key(&route.plugin),
                "forge route"
            );
        }
        if let Some(route) = self.routes.default_route() {
            tracing::info!(
                plugin = %route.plugin,
                available = self.loaded.contains_key(&route.plugin),
                "default forge route"
            );
        } else {
            tracing::info!("no default forge route");
        }
    }
}

/// Loads the forge plugin `name`; fails naming the configured path if the component is missing,
/// invalid or exports no `pull-requests`.
pub(crate) fn load_forge_plugin(
    engine: &PluginEngine,
    name: &str,
    config: &PluginConfig,
) -> anyhow::Result<PluginHost> {
    let memory = config
        .memory_mib
        .checked_mul(MIB)
        .and_then(|bytes| usize::try_from(bytes).ok())
        .with_context(|| format!("`plugins.{name}.memoryMib` is too large"))?;
    let path = config.path.display();
    let host = PluginHost::load(
        engine,
        name,
        &config.path,
        Duration::from_secs(config.call_timeout_seconds),
        memory,
    )
    .with_context(|| format!("cannot load the `{name}` plugin from `{path}`"))?;
    if !host.capabilities().pull_requests {
        anyhow::bail!(
            "the `{name}` plugin from `{path}` is not a forge plugin: it exports no `pull-requests`"
        );
    }
    Ok(host)
}

/// The [`ForgeClient`] that routes the projects in `pool` to the loaded forge plugins.
pub(crate) fn forge_client(
    plugins: ForgePlugins,
    pool: PgPool,
    secrets: Arc<dyn SecretStore>,
) -> Arc<dyn ForgeClient> {
    let clients = plugins
        .loaded
        .into_iter()
        .map(|(name, (host, config))| {
            let client = PluginForgeClient::new(host, secrets.clone(), config.settings);
            (name, client)
        })
        .collect();
    Arc::new(ForgeRouter::new(
        Arc::new(DbProjects { pool }),
        plugins.routes,
        clients,
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
        let err = load_forge_plugin(&PluginEngine::new().expect("engine"), "forgejo", &config)
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
        let err = load_forge_plugin(&PluginEngine::new().expect("engine"), "forgejo", &config)
            .err()
            .expect("must fail");
        let message = format!("{err:#}");
        assert!(message.contains("/nonexistent/forgejo.wasm"), "{message}");
        assert!(message.contains("`forgejo`"), "{message}");
    }

    fn engine() -> PluginEngine {
        PluginEngine::new().expect("engine")
    }

    fn with_hosts(path: impl Into<std::path::PathBuf>, hosts: &[&str]) -> PluginConfig {
        let mut config = PluginConfig::new(path);
        config.hosts = hosts
            .iter()
            .map(|host| df_core::config::HostRoute::Host((*host).to_string()))
            .collect();
        config
    }

    #[test]
    fn single_missing_plugin_fails_startup_naming_plugin_and_path() {
        let plugins = BTreeMap::from([(
            "forgejo".to_string(),
            PluginConfig::new("/nonexistent/forgejo.wasm"),
        )]);
        let err = load_forge_plugins(&engine(), &plugins)
            .err()
            .expect("must fail");
        let message = format!("{err:#}");
        assert!(message.contains("no forge plugin loaded"), "{message}");
        assert!(message.contains("`forgejo`"), "{message}");
        assert!(message.contains("/nonexistent/forgejo.wasm"), "{message}");
    }

    #[test]
    fn without_plugins_the_implicit_forgejo_entry_is_used() {
        let err = load_forge_plugins(&engine(), &BTreeMap::new())
            .err()
            .expect("the image path does not exist here");
        let message = format!("{err:#}");
        assert!(message.contains(IMPLICIT_PLUGIN_PATH), "{message}");
        assert!(message.contains("`forgejo`"), "{message}");
    }

    #[test]
    fn missing_plugin_is_skipped_and_keeps_its_hosts() {
        let plugins = BTreeMap::from([
            (
                "forgejo".to_string(),
                PluginConfig::new(crate::tests::forgejo_component()),
            ),
            (
                "github".to_string(),
                with_hosts("/nonexistent/github.wasm", &["github.com"]),
            ),
        ]);
        let plugins = load_forge_plugins(&engine(), &plugins).expect("forgejo loads");
        assert_eq!(
            plugins.loaded.keys().collect::<Vec<_>>(),
            vec!["forgejo"],
            "only forgejo is available"
        );
        let route = plugins
            .routes
            .resolve("https://github.com/o/r")
            .expect("routed");
        assert_eq!(route.plugin, "github");
        let default = plugins.routes.default_route().expect("default");
        assert_eq!(default.plugin, "forgejo");
    }

    /// The fields of the captured JSON records with `message`.
    fn events(captured: &df_telemetry::testing::Captured, message: &str) -> Vec<serde_json::Value> {
        captured
            .text()
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .map(|record| record.get("fields").cloned().unwrap_or(record))
            .filter(|fields| fields["message"] == message)
            .collect()
    }

    fn forgejo_and_missing_github() -> BTreeMap<String, PluginConfig> {
        BTreeMap::from([
            (
                "forgejo".to_string(),
                PluginConfig::new(crate::tests::forgejo_component()),
            ),
            (
                "github".to_string(),
                with_hosts(
                    "/nonexistent/github.wasm",
                    &["github.com", "ghe.example.com"],
                ),
            ),
        ])
    }

    #[test]
    fn missing_plugin_is_logged_as_an_error_naming_plugin_and_path() {
        let (captured, _guard) = df_telemetry::testing::capture("error");
        load_forge_plugins(&engine(), &forgejo_and_missing_github()).expect("forgejo loads");
        let errors = events(&captured, "forge plugin not loaded");
        assert_eq!(errors.len(), 1, "{}", captured.text());
        assert_eq!(errors[0]["plugin"], "github");
        assert_eq!(errors[0]["path"], "/nonexistent/github.wasm");
    }

    #[test]
    fn routing_table_is_logged_once_per_route_with_availability() {
        let (captured, _guard) = df_telemetry::testing::capture("info");
        load_forge_plugins(&engine(), &forgejo_and_missing_github()).expect("forgejo loads");
        let routes: Vec<_> = events(&captured, "forge route")
            .iter()
            .map(|f| {
                (
                    f["host"].clone(),
                    f["plugin"].clone(),
                    f["available"].clone(),
                )
            })
            .collect();
        assert_eq!(
            routes,
            vec![
                ("ghe.example.com".into(), "github".into(), false.into()),
                ("github.com".into(), "github".into(), false.into()),
            ],
            "{}",
            captured.text()
        );
        let defaults = events(&captured, "default forge route");
        assert_eq!(defaults.len(), 1, "{}", captured.text());
        assert_eq!(defaults[0]["plugin"], "forgejo");
        assert_eq!(defaults[0]["available"], true);
    }

    #[test]
    fn host_claimed_twice_fails_before_loading() {
        let plugins = BTreeMap::from([
            (
                "github".to_string(),
                with_hosts("/nonexistent/github.wasm", &["github.com"]),
            ),
            (
                "mirror".to_string(),
                with_hosts("/nonexistent/mirror.wasm", &["GitHub.com"]),
            ),
        ]);
        let err = load_forge_plugins(&engine(), &plugins)
            .err()
            .expect("must fail");
        let message = format!("{err:#}");
        assert!(
            message.contains("invalid `plugins` configuration"),
            "{message}"
        );
        assert!(message.contains("`github`"), "{message}");
        assert!(message.contains("`mirror`"), "{message}");
        assert!(!message.contains("cannot load"), "{message}");
    }
}
