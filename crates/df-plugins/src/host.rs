//! Loading, validating and calling plugin components.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimitsBuilder};

use crate::policy::HttpPolicy;
use crate::state::State;

wasmtime::component::bindgen!({
    path: "../../contract/wit/integrations",
    world: "forge",
    exports: { default: async },
});

pub(crate) use darkfactory::integrations::types as wit;
pub(crate) use exports::darkfactory::integrations::{issues, pull_requests};

/// Interval of the epoch clock; call timeouts have this granularity.
const EPOCH_TICK: Duration = Duration::from_millis(50);

/// Interfaces a plugin may import (version stripped): world `plugin-base` plus `wasi:io` and
/// `wasi:http/types`, which come with `wasi:http/outgoing-handler`. Ambient authority
/// (`wasi:filesystem`, `wasi:sockets`) is not listed; the WASI context grants nothing for the
/// listed ones except `wasi:http` and stderr.
const ALLOWED_IMPORTS: &[&str] = &[
    "wasi:http/types",
    "wasi:http/outgoing-handler",
    "wasi:config/store",
    "wasi:io/poll",
    "wasi:io/error",
    "wasi:io/streams",
    "wasi:clocks/monotonic-clock",
    "wasi:clocks/wall-clock",
    "wasi:random/random",
    "wasi:random/insecure-seed",
    "wasi:cli/stderr",
    "wasi:cli/environment",
    "wasi:cli/exit",
    "wasi:cli/stdin",
    "wasi:cli/stdout",
    "wasi:cli/terminal-input",
    "wasi:cli/terminal-output",
    "wasi:cli/terminal-stdin",
    "wasi:cli/terminal-stdout",
    "wasi:cli/terminal-stderr",
    "darkfactory:integrations/types",
];

/// Why a component cannot be used as a plugin.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The component file cannot be read.
    #[error("reading plugin `{path}`: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    /// The Wasmtime engine cannot be created.
    #[error("creating the plugin engine: {0}")]
    Engine(String),
    /// Not a valid component, or it does not match world `forge`.
    #[error("invalid plugin component: {0}")]
    Invalid(String),
    /// An import outside the allow-list, e.g. `wasi:filesystem/preopens`.
    #[error("plugin imports `{0}`, which is not allowed")]
    ForbiddenImport(String),
    /// Exports neither `pull-requests` nor `issues`.
    #[error("plugin exports no capability interface (`pull-requests` or `issues`)")]
    NoCapability,
}

/// Capability interfaces a component exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Exports `darkfactory:integrations/pull-requests`.
    pub pull_requests: bool,
    /// Exports `darkfactory:integrations/issues`.
    pub issues: bool,
}

/// The Wasmtime engine shared by all plugins, with the epoch clock that enforces call timeouts.
/// Clones share the engine and the clock thread; the thread stops with the last clone.
#[derive(Clone)]
pub struct PluginEngine(Arc<EngineClock>);

struct EngineClock {
    engine: Engine,
    ticker: Option<std::thread::JoinHandle<()>>,
    stop_ticker: std::sync::mpsc::Sender<()>,
}

impl PluginEngine {
    /// Creates the engine and starts its epoch clock.
    pub fn new() -> Result<Self, LoadError> {
        let mut config = Config::new();
        config.epoch_interruption(true);
        let engine = Engine::new(&config).map_err(|e| LoadError::Engine(format!("{e:#}")))?;

        let (stop_ticker, stopped) = std::sync::mpsc::channel();
        let clock = engine.clone();
        let ticker = std::thread::spawn(move || {
            // Ends when the engine is dropped (sender gone) or told to stop.
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                stopped.recv_timeout(EPOCH_TICK)
            {
                clock.increment_epoch();
            }
        });
        Ok(Self(Arc::new(EngineClock {
            engine,
            ticker: Some(ticker),
            stop_ticker,
        })))
    }

    fn engine(&self) -> &Engine {
        &self.0.engine
    }
}

impl Drop for EngineClock {
    fn drop(&mut self) {
        self.stop_ticker.send(()).ok(); // the ticker may already be gone
        if let Some(ticker) = self.ticker.take() {
            ticker.join().ok(); // a panicked ticker has nothing left to clean up
        }
    }
}

/// One project's plugin instance: where it may connect, who it acts as, what it is configured with.
pub(crate) struct InstanceSpec {
    /// Where the instance may connect and the credential it carries.
    pub policy: HttpPolicy,
    /// Non-secret settings handed over through `wasi:config/store`.
    pub settings: HashMap<String, String>,
}

/// A loaded, validated plugin component that creates a fresh instance per call.
pub struct PluginHost {
    /// Plugin name from the configuration; tags the plugin's log output.
    name: String,
    engine: PluginEngine,
    pre: ForgePre<State>,
    capabilities: Capabilities,
    call_timeout: Duration,
    memory_limit: usize,
}

impl PluginHost {
    /// Reads and validates the component at `path` and logs its size and the load time
    /// (`compile_ms`): everything after the file read, i.e. import validation, compilation and
    /// pre-instantiation.
    pub fn load(
        engine: &PluginEngine,
        name: &str,
        path: &Path,
        call_timeout: Duration,
        memory_limit: usize,
    ) -> Result<Self, LoadError> {
        let bytes = std::fs::read(path).map_err(|source| LoadError::Read {
            path: path.display().to_string(),
            source,
        })?;
        let started = std::time::Instant::now();
        let host = Self::from_bytes(engine, name, &bytes, call_timeout, memory_limit)?;
        tracing::info!(
            plugin = name,
            path = %path.display(),
            bytes = bytes.len(),
            compile_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "plugin loaded"
        );
        Ok(host)
    }

    /// Validates `bytes` (a binary or text component) and prepares it for calls.
    pub fn from_bytes(
        engine: &PluginEngine,
        name: &str,
        bytes: &[u8],
        call_timeout: Duration,
        memory_limit: usize,
    ) -> Result<Self, LoadError> {
        let invalid = |e: wasmtime::Error| LoadError::Invalid(format!("{e:#}"));
        let component = Component::new(engine.engine(), bytes).map_err(invalid)?;

        let ty = component.component_type();
        if let Some((name, _)) = ty
            .imports(engine.engine())
            .find(|(name, _)| !ALLOWED_IMPORTS.contains(&name.split('@').next().unwrap_or(name)))
        {
            return Err(LoadError::ForbiddenImport(name.to_string()));
        }
        let exports: Vec<&str> = ty.exports(engine.engine()).map(|(name, _)| name).collect();
        let exports_interface = |interface: &str| {
            exports
                .iter()
                .any(|e| e.starts_with(&format!("darkfactory:integrations/{interface}@")))
        };
        let capabilities = Capabilities {
            pull_requests: exports_interface("pull-requests"),
            issues: exports_interface("issues"),
        };
        if !capabilities.pull_requests && !capabilities.issues {
            return Err(LoadError::NoCapability);
        }

        let mut linker = Linker::new(engine.engine());
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(invalid)?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker).map_err(invalid)?;
        wasmtime_wasi_config::add_to_linker(&mut linker, State::config).map_err(invalid)?;
        let pre =
            ForgePre::new(linker.instantiate_pre(&component).map_err(invalid)?).map_err(invalid)?;

        Ok(Self {
            name: name.to_string(),
            engine: engine.clone(),
            pre,
            capabilities,
            call_timeout,
            memory_limit,
        })
    }

    /// The capability interfaces the component exports.
    pub fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    /// Runs `call` on a fresh instance. Exceeding the timeout or memory limit fails only this
    /// call; the instance is dropped afterwards.
    pub(crate) async fn run<T>(
        &self,
        spec: InstanceSpec,
        call: impl for<'a> FnOnce(
            &'a Forge,
            &'a mut Store<State>,
        ) -> BoxFuture<'a, wasmtime::Result<Result<T, wit::Error>>>,
    ) -> Result<T, wit::Error> {
        let internal =
            |what: &str, e: wasmtime::Error| wit::Error::Internal(format!("{what}: {e:#}"));
        let limits = StoreLimitsBuilder::new()
            .memory_size(self.memory_limit)
            .build();
        let mut store = Store::new(
            self.engine.engine(),
            State::new(&self.name, spec.policy, spec.settings, limits),
        );
        store.limiter(|state| &mut state.limits);
        store.set_epoch_deadline(epoch_deadline(self.call_timeout));

        let invoke = async {
            let plugin = self
                .pre
                .instantiate_async(&mut store)
                .await
                .map_err(|e| internal("instantiating plugin", e))?;
            call(&plugin, &mut store)
                .await
                .map_err(|e| internal("plugin call failed", e))?
        };
        // The epoch deadline stops guest code; the timeout covers waiting on the network.
        tokio::time::timeout(self.call_timeout, invoke)
            .await
            .map_err(|_elapsed| wit::Error::Internal("plugin call timed out".to_string()))?
    }
}

/// Epoch ticks until `timeout` expires, plus one: the epoch advances at an arbitrary point within
/// a tick, so exactly `timeout / tick` ticks could fire early. Saturates for huge timeouts.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the float is a non-negative tick count and `as` saturates"
)]
fn epoch_deadline(timeout: Duration) -> u64 {
    (timeout.div_duration_f64(EPOCH_TICK).ceil() as u64).saturating_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_deadline_saturates_instead_of_overflowing() {
        assert_eq!(epoch_deadline(Duration::MAX), u64::MAX);
        assert_eq!(epoch_deadline(Duration::from_millis(100)), 3);
    }
}
