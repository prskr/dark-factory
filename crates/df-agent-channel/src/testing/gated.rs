//! A sandbox provider that holds one kind of call until the test releases it.

use std::sync::Arc;

use df_core::testing::FakeSandboxProvider;
use df_core::{
    PeerIdentity, ProviderError, RunId, RunSandboxSpec, SandboxEvent, SandboxProvider,
    TaskVolumeSpec,
};
use tokio::sync::Notify;

use super::Env;

/// The provider call [`GatedProvider`] holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    Provision,
    DeleteTaskVolume,
    SandboxExists,
}

/// Forwards every call to the fake; the gated call signals `entered` after the fake recorded it
/// and then waits for `release`.
pub(crate) struct GatedProvider {
    inner: FakeSandboxProvider,
    gate: Gate,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl GatedProvider {
    /// Replaces the provider of `env.ctx` with one that gates `gate` in front of `env.provider`.
    /// Returns `(entered, release)`.
    pub(crate) fn install(env: &mut Env, gate: Gate) -> (Arc<Notify>, Arc<Notify>) {
        let (entered, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
        env.ctx.provider = Arc::new(Self {
            inner: env.provider.clone(),
            gate,
            entered: entered.clone(),
            release: release.clone(),
        });
        (entered, release)
    }

    async fn hold(&self, call: Gate) {
        if call == self.gate {
            self.entered.notify_one();
            self.release.notified().await;
        }
    }
}

#[async_trait::async_trait]
impl SandboxProvider for GatedProvider {
    async fn ensure_task_volume(&self, task: &TaskVolumeSpec) -> Result<(), ProviderError> {
        self.inner.ensure_task_volume(task).await
    }

    async fn delete_task_volume(&self, task: &TaskVolumeSpec) -> Result<(), ProviderError> {
        let result = self.inner.delete_task_volume(task).await;
        self.hold(Gate::DeleteTaskVolume).await;
        result
    }

    async fn provision(&self, spec: &RunSandboxSpec) -> Result<(), ProviderError> {
        let result = self.inner.provision(spec).await;
        self.hold(Gate::Provision).await;
        result
    }

    async fn teardown(&self, run_id: RunId) -> Result<(), ProviderError> {
        self.inner.teardown(run_id).await
    }

    async fn sandbox_exists(&self, run_id: RunId) -> Result<bool, ProviderError> {
        let result = self.inner.sandbox_exists(run_id).await;
        self.hold(Gate::SandboxExists).await;
        result
    }

    async fn resolve_identity(&self, peer: &PeerIdentity) -> Option<RunId> {
        self.inner.resolve_identity(peer).await
    }

    fn events(&self) -> futures::stream::BoxStream<'static, SandboxEvent> {
        self.inner.events()
    }
}
