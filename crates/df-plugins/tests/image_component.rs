//! The component as shipped in the plugin image. Under Bazel `FORGEJO_PLUGIN_WASM` is the
//! optimized build of `//deploy:forgejo_plugin`, the exact file of the image.

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
