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
use df_core::testing::FakeSecretStore;
use df_core::{CredentialRef, CredentialRefId, IntegrationError, ProjectId};
use df_plugins::{ForgeTarget, PluginForgeClient, PluginHost, ProjectDirectory};

pub(crate) const TOKEN: &str = "s3cret-token";

/// A prebuilt plugin component: `env_var` (set by Bazel), else the output of
/// `cargo build --target wasm32-wasip2 -p forgejo-plugin -p test-probe-plugin`. The tests don't
/// build components themselves, see the README.
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
            "reading {}: {e}; run `cargo build --target wasm32-wasip2 -p forgejo-plugin -p test-probe-plugin` first",
            path.display()
        )
    })
}

pub(crate) fn forgejo() -> Vec<u8> {
    component("FORGEJO_PLUGIN_WASM", "forgejo_plugin.wasm")
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

pub(crate) type Client = PluginForgeClient;

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
    let secrets = FakeSecretStore::new();
    secrets.insert("forge-token", "token", token);
    Ok(PluginForgeClient::new(
        PluginHost::from_bytes("forgejo", component, timeout, memory_limit)?,
        Arc::new(FixedDirectory {
            repo_url: repo_url.into(),
        }),
        Arc::new(secrets),
        BTreeMap::default(),
    ))
}
