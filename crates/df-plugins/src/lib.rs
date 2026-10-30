//! Executes integration plugins (WebAssembly components implementing `darkfactory:integrations`)
//! and exposes the Forgejo plugin as a [`df_core::ForgeClient`].
//!
//! Plugins get standard WASI without capabilities; `wasi:http` is their only way out and the host
//! decides where it may go and injects the credential (ADR-0002, ADR-0028).

mod convert;
mod forge;
mod host;
mod policy;
mod retry;
mod state;

pub use forge::{ForgeTarget, PluginForgeClient, ProjectDirectory};
pub use host::{Capabilities, LoadError, PluginEngine, PluginHost};
