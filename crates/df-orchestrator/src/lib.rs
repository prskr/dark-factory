//! Control plane orchestration: run scheduling and teardown, run context loading, the pull
//! request lifecycle, forge webhook application and task volume retention.
//!
//! Nothing here knows the agent protocol; the agent port (`df-agent-channel`) builds on it.
//! Every decision is derived from the database, so any replica can schedule, provision and tear
//! down runs and apply webhooks. Task volume retention and the pull request outbox handlers
//! (`pr::sync`, `pr::mark_draft`) run only on the elected leader.

#![cfg_attr(
    test,
    expect(
        clippy::disallowed_methods,
        reason = "test fixtures read and seed the database directly (AGENTS.md, \"Database access\")"
    )
)]

use std::sync::Arc;

use df_core::config::{RunsConfig, SchedulingConfig, VolumesConfig};
use df_core::{ForgeClient, InstanceConfig, SandboxProvider, SecretStore};
use df_db::Notifier;
use df_db::lifecycle_events::EventSinks;
use sqlx::PgPool;

pub mod error;
pub mod pr;
pub mod retention;
pub mod run_context;
pub mod runs;
pub mod scheduler;
pub mod webhooks;

#[cfg(feature = "test-util")]
pub mod testing;

#[cfg(test)]
mod lifecycle_event_tests;

pub use error::OrchestratorError;
pub use retention::Retention;
pub use scheduler::Scheduler;

/// The sections of the instance configuration the orchestration reads.
#[derive(Debug, Clone, Default)]
pub struct OrchestratorConfig {
    pub runs: RunsConfig,
    pub volumes: VolumesConfig,
    /// Receivers of lifecycle events; off by default.
    pub events: EventSinks,
    /// The scheduler's tick and the resume backoff.
    pub scheduling: SchedulingConfig,
}

impl From<&InstanceConfig> for OrchestratorConfig {
    fn from(cfg: &InstanceConfig) -> Self {
        Self {
            runs: cfg.runs.clone(),
            volumes: cfg.volumes.clone(),
            events: EventSinks::new(&cfg.lifecycle_events),
            scheduling: cfg.scheduling,
        }
    }
}

/// Everything the scheduler, retention and the run end of one replica share.
#[derive(Clone)]
pub struct OrchestratorContext {
    pub pool: PgPool,
    pub notifier: Arc<Notifier>,
    pub provider: Arc<dyn SandboxProvider>,
    pub forge: Arc<dyn ForgeClient>,
    pub secrets: Arc<dyn SecretStore>,
    pub config: Arc<OrchestratorConfig>,
}
