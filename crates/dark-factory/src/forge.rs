//! Loads every configured forge plugin and routes [`ForgeClient`] calls through [`ForgeRouter`].

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use async_trait::async_trait;
use df_agent_channel::webhooks::WebhookApplier;
use df_core::config::WebhookSecretConfig;
use df_core::routing::{ForgeRoutes, RoutingTable};
use df_core::{IntegrationError, PluginConfig, ProjectId, SecretKeyRef, SecretStore};
use df_db::PgPool;
use df_plugins::{
    ForgeRouter, ForgeTarget, PluginEngine, PluginForgeClient, PluginHost, ProjectDirectory,
    WebhookError,
};
use df_web::webhooks::{WebhookReceiver, WebhookRejection, Webhooks};
use secrecy::{ExposeSecret as _, SecretString};

const MIB: u64 = 1024 * 1024;
/// The plugin entry used when `plugins` is empty: the Forgejo component of the image.
const IMPLICIT_PLUGIN: &str = "forgejo";
const IMPLICIT_PLUGIN_PATH: &str = "/plugins/forgejo/plugin.wasm";

/// The validated routing table, the forge plugins that loaded, by name, and those that did not.
pub(crate) struct ForgePlugins {
    routes: RoutingTable,
    loaded: BTreeMap<String, (PluginHost, PluginConfig)>,
    failures: Vec<LoadFailure>,
    /// Webhook secrets by plugin name, filled by [`ForgePlugins::resolve_webhook_secrets`].
    webhook_secrets: BTreeMap<String, SecretString>,
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
///
/// With `allow_plain_http` (`network.allowPlainHttp`) the routing table accepts `http://` repository
/// URLs and `apiUrl`s, and [`ForgeRouter`] lets every plugin client reach `http://` base URLs.
pub(crate) fn load_forge_plugins(
    engine: &PluginEngine,
    plugins: &BTreeMap<String, PluginConfig>,
    allow_plain_http: bool,
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
    let routes = if allow_plain_http {
        RoutingTable::from_plugins_allowing_plain_http(plugins)
    } else {
        RoutingTable::from_plugins(plugins)
    }
    .context("invalid `plugins` configuration")?;
    if allow_plain_http {
        tracing::warn!(
            setting = "network.allowPlainHttp",
            "plain HTTP allowed for forges: traffic and tokens may travel unencrypted; use it for throwaway clusters only"
        );
    }
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
        webhook_secrets: BTreeMap::new(),
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

    /// Reads (`generate: false`) or get-or-creates (`generate: true`) the webhook secret of every
    /// loaded plugin that exports `webhooks` and sets `webhookSecret` (design D2). Fails naming
    /// `plugins.<name>.webhookSecret`, the Secret and the key. Run it once the store is available,
    /// before [`forge_client`].
    pub(crate) async fn resolve_webhook_secrets(
        &mut self,
        store: &dyn SecretStore,
        instance_name: &str,
    ) -> anyhow::Result<()> {
        let entries: Vec<_> = self
            .loaded
            .iter()
            .filter_map(|(name, (host, config))| {
                // No fixture component lacks a `webhooks` export (a plugin must export
                // `pull-requests` to load, and all of ours export `webhooks`), so this filter is
                // covered only through `resolve_secrets` with `exports_webhooks: false`.
                Some(WebhookSecretEntry {
                    plugin: name.as_str(),
                    config: config.webhook_secret.as_ref()?,
                    exports_webhooks: host.capabilities().webhooks,
                })
            })
            .collect();
        self.webhook_secrets = resolve_secrets(&entries, store, instance_name).await?;
        Ok(())
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
) -> Arc<ForgeRouter> {
    let mut webhook_secrets = plugins.webhook_secrets;
    let clients = plugins
        .loaded
        .into_iter()
        .map(|(name, (host, config))| {
            let mut client = PluginForgeClient::new(host, secrets.clone(), config.settings);
            if let Some(secret) = webhook_secrets.remove(&name) {
                client = client.with_webhook_secret(secret);
            }
            (name, client)
        })
        .collect();
    Arc::new(ForgeRouter::new(
        Arc::new(DbProjects { pool }),
        plugins.routes,
        clients,
    ))
}

/// The webhook route's receiver over `forge`, counting on the `dark-factory` meter.
pub(crate) fn webhooks(forge: &Arc<ForgeRouter>, pool: &PgPool) -> Webhooks {
    let receiver = ForgeWebhooks::new(forge.clone(), pool.clone());
    Webhooks::new(
        Arc::new(receiver),
        &opentelemetry::global::meter("dark-factory"),
    )
}

/// The `webhookSecret` of one loaded plugin.
struct WebhookSecretEntry<'a> {
    plugin: &'a str,
    config: &'a WebhookSecretConfig,
    exports_webhooks: bool,
}

async fn resolve_secrets(
    entries: &[WebhookSecretEntry<'_>],
    store: &dyn SecretStore,
    instance_name: &str,
) -> anyhow::Result<BTreeMap<String, SecretString>> {
    let mut secrets = BTreeMap::new();
    for entry in entries {
        let WebhookSecretEntry {
            plugin,
            config,
            exports_webhooks,
        } = *entry;
        if !exports_webhooks {
            tracing::info!(
                plugin,
                "plugin exports no webhooks: `webhookSecret` ignored"
            );
            continue;
        }
        let key_ref = SecretKeyRef {
            name: config.name.clone(),
            key: config.key.clone(),
        };
        let setting = format!("plugins.{plugin}.webhookSecret");
        let read = if config.generate {
            store
                .get_or_create(&key_ref, &generated_secret_labels(plugin, instance_name))
                .await
        } else {
            store.get(&key_ref).await
        };
        let secret = read.with_context(|| {
            format!(
                "cannot read `{setting}`: Secret `{}`, key `{}`",
                config.name, config.key
            )
        })?;
        // A value written with `echo` ends in a line break the forge never signs with.
        let secret = SecretString::from(secret.expose_secret().trim_end_matches(['\r', '\n']));
        if secret.expose_secret().is_empty() {
            anyhow::bail!(
                "`{setting}`: key `{}` of Secret `{}` is empty",
                config.key,
                config.name
            );
        }
        secrets.insert(plugin.to_string(), secret);
    }
    Ok(secrets)
}

/// Labels of a Secret the control plane generated (design D5).
fn generated_secret_labels(plugin: &str, instance_name: &str) -> BTreeMap<String, String> {
    [
        ("app.kubernetes.io/name", "dark-factory"),
        ("app.kubernetes.io/managed-by", "dark-factory"),
        ("app.kubernetes.io/component", "webhook-secret"),
        ("app.kubernetes.io/instance", instance_name),
        ("dark-factory.io/plugin", plugin),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

struct DbProjects {
    pool: PgPool,
}

/// Verifies and parses webhook deliveries with the forge plugins, then applies their events.
struct ForgeWebhooks {
    forge: Arc<ForgeRouter>,
    applier: WebhookApplier,
}

impl ForgeWebhooks {
    fn new(forge: Arc<ForgeRouter>, pool: PgPool) -> Self {
        let applier = WebhookApplier::new(pool, forge.clone());
        Self { forge, applier }
    }
}

#[async_trait]
impl WebhookReceiver for ForgeWebhooks {
    fn is_loaded(&self, plugin: &str) -> bool {
        self.forge.has_plugin(plugin)
    }

    fn has_endpoint(&self, plugin: &str) -> bool {
        self.forge.has_webhook(plugin)
    }

    async fn receive(
        &self,
        plugin: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<(), WebhookRejection> {
        let events = self
            .forge
            .handle_webhook(plugin, headers, body)
            .await
            .map_err(|error| webhook_rejection(plugin, error))?;
        self.applier.apply(&events).await.map_err(|error| {
            tracing::warn!(plugin, error = %error, "applying webhook events failed");
            WebhookRejection::Failed
        })
    }
}

/// The response to a delivery the plugin did not turn into events.
fn webhook_rejection(plugin: &str, error: WebhookError) -> WebhookRejection {
    match error {
        WebhookError::NotFound => WebhookRejection::NotFound,
        WebhookError::Unverified => {
            tracing::debug!(plugin, "webhook signature not verified");
            WebhookRejection::Unauthorized
        }
        WebhookError::InvalidInput(message) => {
            tracing::debug!(plugin, message, "invalid webhook payload");
            WebhookRejection::Invalid
        }
        WebhookError::Failed(error) => {
            tracing::warn!(plugin, error = %error, "webhook handling failed");
            WebhookRejection::Failed
        }
    }
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
    use df_core::SecretError;
    use df_core::testing::FakeSecretStore;

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
    fn webhook_errors_map_to_their_rejection() {
        let cases = [
            (WebhookError::NotFound, WebhookRejection::NotFound),
            (WebhookError::Unverified, WebhookRejection::Unauthorized),
            (
                WebhookError::InvalidInput("no action".into()),
                WebhookRejection::Invalid,
            ),
            (
                WebhookError::Failed(IntegrationError::Internal("trap".into())),
                WebhookRejection::Failed,
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(webhook_rejection("forgejo", error), expected);
        }
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
    fn plain_http_repository_urls_route_only_when_allowed() {
        let plugins = BTreeMap::from([(
            "forgejo".to_string(),
            PluginConfig::new(crate::tests::forgejo_component()),
        )]);
        let url = "http://forge.test:3000/o/r";
        let strict = load_forge_plugins(&engine(), &plugins, false).expect("loads");
        assert!(strict.routes.resolve(url).is_err());
        let allowed = load_forge_plugins(&engine(), &plugins, true).expect("loads");
        assert_eq!(
            allowed.routes.resolve(url).expect("routed").plugin,
            "forgejo"
        );
    }

    #[test]
    fn single_missing_plugin_fails_startup_naming_plugin_and_path() {
        let plugins = BTreeMap::from([(
            "forgejo".to_string(),
            PluginConfig::new("/nonexistent/forgejo.wasm"),
        )]);
        let err = load_forge_plugins(&engine(), &plugins, false)
            .err()
            .expect("must fail");
        let message = format!("{err:#}");
        assert!(message.contains("no forge plugin loaded"), "{message}");
        assert!(message.contains("`forgejo`"), "{message}");
        assert!(message.contains("/nonexistent/forgejo.wasm"), "{message}");
    }

    #[test]
    fn without_plugins_the_implicit_forgejo_entry_is_used() {
        let err = load_forge_plugins(&engine(), &BTreeMap::new(), false)
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
        let plugins = load_forge_plugins(&engine(), &plugins, false).expect("forgejo loads");
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
        let plugins = load_forge_plugins(&engine(), &forgejo_and_missing_github(), false)
            .expect("forgejo loads");
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
        let plugins = load_forge_plugins(&engine(), &forgejo_and_missing_github(), false)
            .expect("forgejo loads");
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

    fn webhook_config(generate: bool) -> WebhookSecretConfig {
        WebhookSecretConfig {
            name: "wh-forgejo".into(),
            key: "secret".into(),
            generate,
        }
    }

    async fn resolve(
        store: &FakeSecretStore,
        config: &WebhookSecretConfig,
        exports_webhooks: bool,
    ) -> anyhow::Result<BTreeMap<String, SecretString>> {
        let entry = WebhookSecretEntry {
            plugin: "forgejo",
            config,
            exports_webhooks,
        };
        resolve_secrets(&[entry], store, "homelab").await
    }

    #[tokio::test]
    async fn generate_creates_the_missing_secret_with_instance_and_plugin_labels() {
        let store = FakeSecretStore::new();
        let secrets = resolve(&store, &webhook_config(true), true).await.unwrap();
        assert!(secrets.contains_key("forgejo"));
        let [(key_ref, labels)] = store.created().try_into().unwrap();
        assert_eq!(key_ref.name, "wh-forgejo");
        assert_eq!(labels["app.kubernetes.io/instance"], "homelab");
        assert_eq!(labels["dark-factory.io/plugin"], "forgejo");
        assert_eq!(labels["app.kubernetes.io/component"], "webhook-secret");
    }

    #[tokio::test]
    async fn existing_secret_is_read_when_generate_is_false() {
        let store = FakeSecretStore::new();
        store.insert("wh-forgejo", "secret", "s3cret");
        let secrets = resolve(&store, &webhook_config(false), true).await.unwrap();
        assert_eq!(secrets["forgejo"].expose_secret(), "s3cret");
        assert!(store.created().is_empty());
    }

    #[tokio::test]
    async fn trailing_line_breaks_of_an_existing_secret_are_dropped() {
        let store = FakeSecretStore::new();
        store.insert("wh-forgejo", "secret", "s3cret\r\n");
        let secrets = resolve(&store, &webhook_config(false), true).await.unwrap();
        assert_eq!(secrets["forgejo"].expose_secret(), "s3cret");

        store.insert("wh-forgejo", "secret", "\n\n");
        assert!(resolve(&store, &webhook_config(false), true).await.is_err());
    }

    #[tokio::test]
    async fn missing_or_empty_secret_fails_naming_setting_secret_and_key() {
        let store = FakeSecretStore::new();
        let missing = resolve(&store, &webhook_config(false), true).await;
        store.insert("wh-forgejo", "secret", "");
        let empty = resolve(&store, &webhook_config(false), true).await;
        for err in [missing.unwrap_err(), empty.unwrap_err()] {
            let message = format!("{err:#}");
            for part in ["plugins.forgejo.webhookSecret", "wh-forgejo", "secret"] {
                assert!(message.contains(part), "{message}");
            }
        }
        assert!(store.created().is_empty());
    }

    #[tokio::test]
    async fn plugin_without_webhooks_export_is_skipped_and_creates_nothing() {
        let store = FakeSecretStore::new();
        let secrets = resolve(&store, &webhook_config(true), false).await.unwrap();
        assert!(secrets.is_empty());
        assert!(store.created().is_empty() && store.calls().is_empty());
    }

    #[tokio::test]
    async fn plugin_without_webhook_secret_reads_and_creates_nothing() {
        let plugins = BTreeMap::from([(
            "forgejo".to_string(),
            PluginConfig::new(crate::tests::forgejo_component()),
        )]);
        let mut loaded = load_forge_plugins(&engine(), &plugins, false).expect("loads");
        let store = FakeSecretStore::new();
        loaded
            .resolve_webhook_secrets(&store, "homelab")
            .await
            .unwrap();
        assert!(loaded.webhook_secrets.is_empty());
        assert!(store.calls().is_empty() && store.created().is_empty());
    }

    #[tokio::test]
    async fn secret_store_error_fails_startup_naming_the_setting_and_the_cause() {
        let store = FakeSecretStore::new();
        store.insert_error(
            "wh-forgejo",
            "secret",
            SecretError::Backend("secrets is forbidden: cannot create".into()),
        );
        let err = resolve(&store, &webhook_config(true), true)
            .await
            .unwrap_err();
        let message = format!("{err:#}");
        assert!(
            message.contains("plugins.forgejo.webhookSecret"),
            "{message}"
        );
        assert!(message.contains("forbidden"), "{message}");
    }

    #[tokio::test]
    async fn deleted_generated_secret_is_created_again_with_a_new_value() {
        let store = FakeSecretStore::new();
        let first = resolve(&store, &webhook_config(true), true).await.unwrap();
        store.remove("wh-forgejo", "secret");
        let second = resolve(&store, &webhook_config(true), true).await.unwrap();
        assert_eq!(store.created().len(), 2);
        assert_ne!(
            first["forgejo"].expose_secret(),
            second["forgejo"].expose_secret()
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
        let err = load_forge_plugins(&engine(), &plugins, false)
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

    /// Forgejo's recorded close of the PR from `factory/task/DF-1`, signed with `secret`.
    fn closed_pr_delivery(secret: &str) -> (Vec<(String, String)>, Vec<u8>) {
        let body = include_str!(
            "../../../plugins/forgejo/tests/fixtures/webhooks/pull_request_closed.json"
        )
        .replace("factory/task/DF-2", "factory/task/DF-1")
        .into_bytes();
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
        let signature = hex::encode(ring::hmac::sign(&key, &body));
        let headers = vec![
            ("X-Forgejo-Event".to_string(), "pull_request".to_string()),
            ("X-Forgejo-Signature".to_string(), signature),
        ];
        (headers, body)
    }

    #[tokio::test]
    async fn signed_forgejo_delivery_is_verified_parsed_and_queued_for_its_task() {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let env = df_agent_channel::testing::Env::new(pool.clone()).await;
        sqlx::query("UPDATE projects SET repo_url = 'http://localhost:13000/peter/demo'")
            .execute(&pool)
            .await
            .unwrap();
        let mut config = PluginConfig::new(crate::tests::forgejo_component());
        config.webhook_secret = Some(webhook_config(false));
        let plugins = BTreeMap::from([("forgejo".to_string(), config)]);
        let mut plugins = load_forge_plugins(&engine(), &plugins, true).unwrap();
        env.secrets.insert("wh-forgejo", "secret", "s3cret");
        plugins
            .resolve_webhook_secrets(&env.secrets, "homelab")
            .await
            .unwrap();
        let forge = forge_client(plugins, pool.clone(), Arc::new(env.secrets.clone()));
        let receiver = ForgeWebhooks::new(forge, pool.clone());

        let (headers, body) = closed_pr_delivery("other");
        let rejected = receiver.receive("forgejo", &headers, &body).await;
        assert_eq!(rejected, Err(WebhookRejection::Unauthorized));
        let (headers, body) = closed_pr_delivery("s3cret");
        receiver.receive("forgejo", &headers, &body).await.unwrap();

        let messages = df_db::tasks::list_undelivered_messages(&pool, env.task.id)
            .await
            .unwrap();
        let from_webhook = messages
            .iter()
            .filter(|m| m.author == df_core::Principal::system(df_core::system_reason::WEBHOOK))
            .count();
        assert_eq!(from_webhook, 1);
    }

    /// The forge router `serve` builds after resolving `webhookSecret` from `store`.
    async fn forge_with_resolved_secret(
        store: &FakeSecretStore,
        generate: bool,
    ) -> Arc<ForgeRouter> {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let mut config = PluginConfig::new(crate::tests::forgejo_component());
        config.webhook_secret = Some(webhook_config(generate));
        let plugins = BTreeMap::from([("forgejo".to_string(), config)]);
        let mut plugins = load_forge_plugins(&engine(), &plugins, true).unwrap();
        plugins
            .resolve_webhook_secrets(store, "homelab")
            .await
            .unwrap();
        forge_client(plugins, pool, Arc::new(store.clone()))
    }

    async fn accepts(forge: &ForgeRouter, signed_with: &str) -> bool {
        let (headers, body) = closed_pr_delivery(signed_with);
        match forge.handle_webhook("forgejo", &headers, &body).await {
            Ok(_) => true,
            Err(WebhookError::Unverified) => false,
            Err(error) => panic!("unexpected webhook error: {error}"),
        }
    }

    #[tokio::test]
    async fn running_forge_keeps_verifying_with_the_secret_read_at_startup() {
        let store = FakeSecretStore::new();
        store.insert("wh-forgejo", "secret", "original");
        let forge = forge_with_resolved_secret(&store, false).await;

        store.insert("wh-forgejo", "secret", "changed");

        assert!(accepts(&forge, "original").await);
        assert!(!accepts(&forge, "changed").await);
    }

    #[tokio::test]
    async fn deliveries_signed_with_a_deleted_generated_secret_are_rejected_after_restart() {
        let store = FakeSecretStore::new();
        let before = forge_with_resolved_secret(&store, true).await;
        let old = store.get(&webhook_key_ref()).await.unwrap();
        assert!(accepts(&before, old.expose_secret()).await);

        store.remove("wh-forgejo", "secret");
        let after = forge_with_resolved_secret(&store, true).await;
        let new = store.get(&webhook_key_ref()).await.unwrap();

        assert_ne!(old.expose_secret(), new.expose_secret());
        assert!(!accepts(&after, old.expose_secret()).await);
        assert!(accepts(&after, new.expose_secret()).await);
    }

    fn webhook_key_ref() -> SecretKeyRef {
        SecretKeyRef {
            name: "wh-forgejo".into(),
            key: "secret".into(),
        }
    }
}
