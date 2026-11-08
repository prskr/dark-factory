//! Control plane end of the agent protocol v1: the mTLS agent port, run scheduling and state
//! machine, transcript persistence, factory tools, pull request lifecycle.
//!
//! Every decision is derived from the database, so any replica can serve any reconnect.

use std::sync::Arc;

use df_core::config::{AgentConfig, GatewayConfig, HarnessConfig, RunsConfig, VolumesConfig};
use df_core::{ForgeClient, InstanceConfig, SandboxProvider, SecretStore};
use df_db::Notifier;
use df_db::lifecycle_events::EventSinks;
use sqlx::PgPool;

pub mod assign;
pub mod error;
pub mod identity;
pub mod metrics;
pub mod pr;
pub mod retention;
pub mod runs;
pub mod scheduler;
pub mod server;
pub mod session;
pub mod summary;
pub mod tools;
pub mod transcript;
pub mod webhooks;

#[cfg(feature = "test-util")]
pub mod testing;

#[cfg(test)]
mod lifecycle_event_tests;

pub use error::ChannelError;
pub use metrics::Metrics;
pub use retention::Retention;
pub use scheduler::Scheduler;
pub use server::{AgentService, MtlsIncoming, TlsError, TlsReloader, mtls_incoming, serve};

/// The sections of the instance configuration the agent channel reads.
#[derive(Debug, Clone, Default)]
pub struct ChannelConfig {
    pub agent: AgentConfig,
    pub harness: HarnessConfig,
    pub runs: RunsConfig,
    pub volumes: VolumesConfig,
    pub git_proxy: GitProxy,
    /// Receivers of lifecycle events; off by default.
    pub events: EventSinks,
    /// Limits of task artifacts.
    pub artifacts: df_core::ArtifactsConfig,
}

impl From<&InstanceConfig> for ChannelConfig {
    fn from(cfg: &InstanceConfig) -> Self {
        Self {
            agent: cfg.agent.clone(),
            harness: cfg.harness.clone(),
            runs: cfg.runs.clone(),
            volumes: cfg.volumes.clone(),
            git_proxy: GitProxy {
                host: cfg.gateway.git_host_name(&cfg.instance_name),
                port: cfg.gateway.git_port,
            },
            events: EventSinks::new(&cfg.lifecycle_events),
            artifacts: cfg.artifacts,
        }
    }
}

/// Host and port of the gateway's Git proxy as sandboxes reach it (design D2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitProxy {
    pub host: String,
    pub port: u16,
}

impl Default for GitProxy {
    fn default() -> Self {
        let gateway = GatewayConfig::default();
        Self {
            host: gateway.git_host_name("dark-factory"),
            port: gateway.git_port,
        }
    }
}

/// Everything the agent port, the sessions and the scheduler of one replica share.
#[derive(Clone)]
pub struct ChannelContext {
    pub pool: PgPool,
    pub notifier: Arc<Notifier>,
    pub provider: Arc<dyn SandboxProvider>,
    pub forge: Arc<dyn ForgeClient>,
    pub secrets: Arc<dyn SecretStore>,
    pub config: Arc<ChannelConfig>,
    pub metrics: Arc<Metrics>,
    /// The approval types of this instance, shared with the web UI. Empty until a change adds
    /// types; the tools that raise approvals (`request_approval`) resolve them here.
    pub approval_types: df_db::requests::ApprovalTypes,
}
