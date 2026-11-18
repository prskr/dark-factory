//! Control plane end of the agent protocol v1: the mTLS agent port, agent sessions with
//! transcript persistence, the `Assign` message and the factory tools. Run scheduling, the run
//! end, the pull request lifecycle and retention are `df-orchestrator`.
//!
//! Every decision is derived from the database, so any replica can serve any reconnect.

#![cfg_attr(
    test,
    expect(
        clippy::disallowed_methods,
        reason = "test fixtures read and seed the database directly (AGENTS.md, \"Database access\")"
    )
)]

use std::sync::Arc;

use df_core::InstanceConfig;
use df_core::config::{AgentConfig, GatewayConfig, HarnessConfig};
use df_orchestrator::OrchestratorContext;

pub mod assign;
pub mod error;
pub mod metrics;
pub mod server;
pub mod session;
pub mod summary;
pub mod tools;

#[cfg(feature = "test-util")]
pub mod testing;

pub use error::ChannelError;
pub use metrics::Metrics;
pub use server::{AgentService, serve};

/// The sections of the instance configuration the agent port reads; the orchestration's are
/// `df_orchestrator::OrchestratorConfig`.
#[derive(Debug, Clone, Default)]
pub struct ChannelConfig {
    pub agent: AgentConfig,
    pub harness: HarnessConfig,
    pub git_proxy: GitProxy,
    /// Limits of task artifacts.
    pub artifacts: df_core::ArtifactsConfig,
    /// Whether the sandboxes confine the harness; the system prompt tells the agent.
    pub landlock: df_sandbox_contract::LandlockMode,
}

impl From<&InstanceConfig> for ChannelConfig {
    fn from(cfg: &InstanceConfig) -> Self {
        Self {
            agent: cfg.agent.clone(),
            harness: cfg.harness.clone(),
            git_proxy: GitProxy {
                host: cfg.gateway.git_host_name(&cfg.instance_name),
                port: cfg.gateway.git_port,
            },
            artifacts: cfg.artifacts,
            landlock: cfg.sandbox.provisioning.landlock,
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

/// Everything the agent port and the sessions of one replica share: the orchestration context
/// (`pool`, `notifier`, `provider`, `forge`, `secrets` are reachable on it through `Deref`) and
/// the port's own parts.
#[derive(Clone)]
pub struct ChannelContext {
    pub orch: OrchestratorContext,
    pub config: Arc<ChannelConfig>,
    pub metrics: Arc<Metrics>,
    /// The approval types of this instance, shared with the web UI. Empty until a change adds
    /// types; the tools that raise approvals (`request_approval`) resolve them here.
    pub approval_types: df_db::requests::ApprovalTypes,
}

impl std::ops::Deref for ChannelContext {
    type Target = OrchestratorContext;

    fn deref(&self) -> &OrchestratorContext {
        &self.orch
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn channel_config_takes_the_landlock_mode_from_the_sandbox_section() {
        let database = "database:\n  host: pg\n  database: d\n  user: u\n  passwordFile: /p\n";
        let landlock = |extra: &str| {
            let cfg = InstanceConfig::from_yaml_str(&format!("{database}{extra}")).unwrap();
            ChannelConfig::from(&cfg).landlock
        };
        assert_eq!(landlock(""), df_sandbox_contract::LandlockMode::Enforce);
        assert_eq!(
            landlock("sandbox:\n  landlock: disabled\n"),
            df_sandbox_contract::LandlockMode::Disabled
        );
    }
}
