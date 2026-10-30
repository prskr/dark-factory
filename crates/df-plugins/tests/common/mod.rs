//! Shared fixtures: plugin components, fake project directory and a client wired to them.

#![expect(dead_code, reason = "each test binary uses a subset")]
#![expect(
    clippy::panic,
    reason = "a missing plugin build is a setup error of the test"
)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use df_core::routing::RoutingTable;
use df_core::testing::FakeSecretStore;
use df_core::{CredentialRef, CredentialRefId, IntegrationError, PluginConfig, ProjectId};
use df_plugins::{
    ForgeRouter, ForgeTarget, PluginEngine, PluginForgeClient, PluginHost, ProjectDirectory,
};

pub(crate) const TOKEN: &str = "s3cret-token";

/// A prebuilt plugin component: `env_var` (set by Bazel), else the output of
/// `cargo build --target wasm32-wasip2 -p forgejo-plugin -p test-probe-plugin -p github-plugin`.
/// The tests don't build components themselves, see the README.
pub(crate) fn component(env_var: &str, file: &str) -> Vec<u8> {
    let path = std::env::var_os(env_var).map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/wasm32-wasip2/debug")
                .join(file)
        },
        PathBuf::from,
    );
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "reading {}: {e}; run `cargo build --target wasm32-wasip2 -p forgejo-plugin -p test-probe-plugin -p github-plugin` first",
            path.display()
        )
    })
}

pub(crate) fn forgejo() -> Vec<u8> {
    component("FORGEJO_PLUGIN_WASM", "forgejo_plugin.wasm")
}

pub(crate) fn github() -> Vec<u8> {
    component("GITHUB_PLUGIN_WASM", "github_plugin.wasm")
}

pub(crate) fn probe() -> Vec<u8> {
    component("TEST_PROBE_WASM", "test_probe_plugin.wasm")
}

pub(crate) struct FixedDirectory {
    pub repo_url: String,
}

#[async_trait]
impl ProjectDirectory for FixedDirectory {
    async fn forge_target(&self, _: &ProjectId) -> Result<ForgeTarget, IntegrationError> {
        Ok(ForgeTarget {
            repo_url: self.repo_url.clone(),
            credential: Some(CredentialRef {
                id: CredentialRefId::new(),
                name: "forge".into(),
                secret_name: "forge-token".into(),
                secret_key: "token".into(),
            }),
        })
    }
}

pub(crate) type Client = ForgeRouter;

/// Projects on different repository URLs, all acting with the same forge credential.
pub(crate) struct ProjectRepos(pub Vec<(ProjectId, String)>);

#[async_trait]
impl ProjectDirectory for ProjectRepos {
    async fn forge_target(&self, project: &ProjectId) -> Result<ForgeTarget, IntegrationError> {
        let (_, repo_url) = self
            .0
            .iter()
            .find(|(id, _)| id == project)
            .ok_or_else(|| IntegrationError::Config(format!("unknown project {project}")))?;
        FixedDirectory {
            repo_url: repo_url.clone(),
        }
        .forge_target(project)
        .await
    }
}

pub(crate) fn client(
    component: &[u8],
    repo_url: &str,
    timeout: Duration,
    memory_limit: usize,
) -> anyhow::Result<Client> {
    client_with_token(component, repo_url, TOKEN, timeout, memory_limit)
}

pub(crate) fn client_with_token(
    component: &[u8],
    repo_url: &str,
    token: &str,
    timeout: Duration,
    memory_limit: usize,
) -> anyhow::Result<Client> {
    client_on(
        &PluginEngine::new()?,
        component,
        repo_url,
        token,
        timeout,
        memory_limit,
    )
}

/// Like [`client_with_token`], on a shared engine.
pub(crate) fn client_on(
    engine: &PluginEngine,
    component: &[u8],
    repo_url: &str,
    token: &str,
    timeout: Duration,
    memory_limit: usize,
) -> anyhow::Result<Client> {
    router(
        plugin_client(engine, component, token, timeout, memory_limit)?,
        repo_url,
    )
}

/// The `forgejo` plugin client reading `token` as forge credential.
pub(crate) fn plugin_client(
    engine: &PluginEngine,
    component: &[u8],
    token: &str,
    timeout: Duration,
    memory_limit: usize,
) -> anyhow::Result<PluginForgeClient> {
    let secrets = FakeSecretStore::new();
    secrets.insert("forge-token", "token", token);
    Ok(PluginForgeClient::new(
        PluginHost::from_bytes(engine, "forgejo", component, timeout, memory_limit)?,
        Arc::new(secrets),
        BTreeMap::default(),
    ))
}

/// A router with `client` as the default plugin `forgejo`, for projects on `repo_url`.
pub(crate) fn router(client: PluginForgeClient, repo_url: &str) -> anyhow::Result<Client> {
    router_with_config(client, PluginConfig::new("/forgejo.wasm"), repo_url)
}

/// Like [`router`], with `config` as the plugin's entry under `plugins`.
pub(crate) fn router_with_config(
    client: PluginForgeClient,
    config: PluginConfig,
    repo_url: &str,
) -> anyhow::Result<Client> {
    named_router("forgejo", client, config, repo_url)
}

/// Like [`router_with_config`], with `client` as the plugin `name`.
pub(crate) fn named_router(
    name: &str,
    client: PluginForgeClient,
    config: PluginConfig,
    repo_url: &str,
) -> anyhow::Result<Client> {
    let plugins = BTreeMap::from([(name.to_string(), config)]);
    Ok(ForgeRouter::new(
        Arc::new(FixedDirectory {
            repo_url: repo_url.into(),
        }),
        RoutingTable::from_plugins_allowing_plain_http(&plugins)?,
        BTreeMap::from([(name.to_string(), client)]),
    ))
}

/// Like [`router`], with the production routing table, which rejects plain HTTP.
pub(crate) fn https_only_router(
    client: PluginForgeClient,
    repo_url: &str,
) -> anyhow::Result<Client> {
    let plugins = BTreeMap::from([("forgejo".to_string(), PluginConfig::new("/forgejo.wasm"))]);
    Ok(ForgeRouter::new(
        Arc::new(FixedDirectory {
            repo_url: repo_url.into(),
        }),
        RoutingTable::from_plugins(&plugins)?,
        BTreeMap::from([("forgejo".to_string(), client)]),
    ))
}
