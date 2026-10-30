//! The components as shipped in the plugin images. Under Bazel `FORGEJO_PLUGIN_WASM` and
//! `GITHUB_PLUGIN_WASM` are the optimized builds of `//deploy:forgejo_plugin` and
//! `//deploy:github_plugin`, the exact files of the images.

mod common;

use std::time::Duration;

use df_plugins::{PluginEngine, PluginHost};

#[test]
fn shipped_forge_plugin_reports_both_capabilities() -> anyhow::Result<()> {
    let host = PluginHost::from_bytes(
        &PluginEngine::new()?,
        "forgejo",
        &common::forgejo(),
        Duration::from_secs(30),
        64 << 20,
    )?;
    let capabilities = host.capabilities();
    assert!(capabilities.pull_requests && capabilities.issues);
    Ok(())
}

#[test]
fn shipped_github_plugin_reports_both_capabilities() -> anyhow::Result<()> {
    let host = PluginHost::from_bytes(
        &PluginEngine::new()?,
        "github",
        &common::github(),
        Duration::from_secs(30),
        64 << 20,
    )?;
    let capabilities = host.capabilities();
    assert!(capabilities.pull_requests && capabilities.issues);
    Ok(())
}
