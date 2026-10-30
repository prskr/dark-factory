//! Loads every configured forge plugin and routes [`ForgeClient`] calls through [`ForgeRouter`].

use std::collections::BTreeMap;
use std::path::PathBuf;
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

/// The validated routing table, the forge plugins that loaded, by name, and those that did not.
pub(crate) struct ForgePlugins {
    routes: RoutingTable,
    loaded: BTreeMap<String, (PluginHost, PluginConfig)>,
    failures: Vec<LoadFailure>,
}

/// A configured forge plugin that did not load.
#[derive(Debug, PartialEq, Eq)]
struct LoadFailure {
    plugin: String,
    path: PathBuf,
    error: String,
}

/// A route of the routing table and whether its plugin loaded.
#[derive(Debug, PartialEq, Eq)]
struct RouteStatus {
    /// The claimed host; `None` for the default route.
    host: Option<String>,
    plugin: String,
    available: bool,
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
    let mut failures = Vec::new();
    for (name, config) in plugins {
        match load_forge_plugin(engine, name, config) {
            Ok(host) => {
                loaded.insert(name.clone(), (host, config.clone()));
            }
            Err(error) => {
                let failure = LoadFailure {
                    plugin: name.clone(),
                    path: config.path.clone(),
                    error: format!("{error:#}"),
                };
                tracing::error!(
                    plugin = %failure.plugin,
                    path = %failure.path.display(),
                    error = %failure.error,
                    "forge plugin not loaded"
                );
                failures.push(failure);
            }
        }
    }
    let plugins = ForgePlugins {
        routes,
        loaded,
        failures,
    };
    if plugins.loaded.is_empty() {
        let errors: Vec<&str> = plugins.failures.iter().map(|f| f.error.as_str()).collect();
        anyhow::bail!("no forge plugin loaded: {}", errors.join("; "));
    }
    plugins.log_routes();
    Ok(plugins)
}

impl ForgePlugins {
    /// The routing table with the loaded plugins, for the web UI (design D7).
    pub(crate) fn forge_routes(&self) -> ForgeRoutes {
        ForgeRoutes::new(self.routes.clone(), self.loaded.keys().cloned())
    }

    /// Every host route, ordered by host, then the default route, each with whether its plugin
    /// loaded.
    fn routing_summary(&self) -> Vec<RouteStatus> {
        let status = |host: Option<&str>, plugin: &str| RouteStatus {
            host: host.map(str::to_string),
            plugin: plugin.to_string(),
            available: self.loaded.contains_key(plugin),
        };
        self.routes
            .hosts()
            .map(|(host, route)| status(Some(host), &route.plugin))
            .chain(
                self.routes
                    .default_route()
                    .map(|route| status(None, &route.plugin)),
            )
            .collect()
    }

    fn log_routes(&self) {
        let summary = self.routing_summary();
        for route in &summary {
            if let Some(host) = &route.host {
                tracing::info!(
                    host = host.as_str(),
                    plugin = %route.plugin,
                    available = route.available,
                    "forge route"
                );
            } else {
                tracing::info!(
                    plugin = %route.plugin,
                    available = route.available,
                    "default forge route"
                );
            }
        }
        if summary.iter().all(|route| route.host.is_some()) {
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

    fn engine() -> PluginEngine {
        PluginEngine::new().expect("engine")
    }

    fn with_hosts(path: impl Into<PathBuf>, hosts: &[&str]) -> PluginConfig {
        let mut config = PluginConfig::new(path);
        config.hosts = hosts
            .iter()
            .map(|host| df_core::config::HostRoute::Host((*host).to_string()))
            .collect();
        config
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
    fn memory_limit_overflow_is_a_config_error() {
        let mut config = PluginConfig::new("/nonexistent/forgejo.wasm");
        config.memory_mib = u64::MAX;
        let err = load_forge_plugin(&engine(), "forgejo", &config)
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
        let err = load_forge_plugin(&engine(), "forgejo", &config)
            .err()
            .expect("must fail");
        let message = format!("{err:#}");
        assert!(message.contains("/nonexistent/forgejo.wasm"), "{message}");
        assert!(message.contains("`forgejo`"), "{message}");
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

    #[test]
    fn missing_plugin_is_reported_as_a_load_failure_naming_plugin_and_path() {
        let plugins =
            load_forge_plugins(&engine(), &forgejo_and_missing_github()).expect("forgejo loads");
        let [failure] = plugins.failures.as_slice() else {
            panic!("one failure expected: {:?}", plugins.failures);
        };
        assert_eq!(failure.plugin, "github");
        assert_eq!(failure.path, PathBuf::from("/nonexistent/github.wasm"));
        assert!(
            failure.error.contains("/nonexistent/github.wasm"),
            "{}",
            failure.error
        );
    }

    #[test]
    fn routing_summary_lists_each_route_with_availability_and_the_default() {
        let plugins =
            load_forge_plugins(&engine(), &forgejo_and_missing_github()).expect("forgejo loads");
        let route = |host: Option<&str>, plugin: &str, available| RouteStatus {
            host: host.map(str::to_string),
            plugin: plugin.to_string(),
            available,
        };
        assert_eq!(
            plugins.routing_summary(),
            vec![
                route(Some("ghe.example.com"), "github", false),
                route(Some("github.com"), "github", false),
                route(None, "forgejo", true),
            ]
        );
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
