//! Placeholders for the sandbox provider, forge client and secret store until their real
//! implementations (`iter1-sandbox-provisioning`, `iter1-integration-plugins`) are wired in.
//! Every operation fails with an explanatory error, so queued tasks end as `provisioning_failed`
//! instead of waiting forever.

use async_trait::async_trait;
use df_core::{
    Comment, CredentialRef, ForgeClient, IntegrationError, Issue, NewPullRequest, PeerIdentity,
    ProjectId, ProviderError, PullRequest, PullRequestUpdate, RunId, RunSandboxSpec, SandboxEvent,
    SandboxProvider, SecretError, SecretStore, TaskVolumeSpec,
};
use futures::stream::{self, BoxStream, StreamExt};
use secrecy::SecretString;

const NOT_CONFIGURED: &str = "no sandbox provider is configured in this build";

pub struct UnconfiguredSandboxProvider;

#[async_trait]
impl SandboxProvider for UnconfiguredSandboxProvider {
    async fn ensure_task_volume(&self, _task: &TaskVolumeSpec) -> Result<(), ProviderError> {
        Err(ProviderError::Backend(NOT_CONFIGURED.into()))
    }

    async fn provision(&self, _spec: &RunSandboxSpec) -> Result<(), ProviderError> {
        Err(ProviderError::Backend(NOT_CONFIGURED.into()))
    }

    async fn teardown(&self, _run_id: RunId) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn resolve_identity(&self, _peer: &PeerIdentity) -> Option<RunId> {
        None
    }

    fn events(&self) -> BoxStream<'static, SandboxEvent> {
        stream::pending().boxed()
    }
}

pub struct UnconfiguredForgeClient;

fn forge_unavailable<T>() -> Result<T, IntegrationError> {
    Err(IntegrationError::Config(
        "no forge integration is configured in this build".into(),
    ))
}

#[async_trait]
impl ForgeClient for UnconfiguredForgeClient {
    async fn create_pull_request(
        &self,
        _p: &ProjectId,
        _req: NewPullRequest,
    ) -> Result<PullRequest, IntegrationError> {
        forge_unavailable()
    }

    async fn find_open_pull_request(
        &self,
        _p: &ProjectId,
        _head: &str,
        _base: &str,
    ) -> Result<Option<PullRequest>, IntegrationError> {
        forge_unavailable()
    }

    async fn update_pull_request(
        &self,
        _p: &ProjectId,
        _number: u64,
        _ch: PullRequestUpdate,
    ) -> Result<PullRequest, IntegrationError> {
        forge_unavailable()
    }

    async fn get_issue(&self, _p: &ProjectId, _number: u64) -> Result<Issue, IntegrationError> {
        forge_unavailable()
    }

    async fn add_issue_comment(
        &self,
        _p: &ProjectId,
        _number: u64,
        _body: &str,
    ) -> Result<Comment, IntegrationError> {
        forge_unavailable()
    }
}

pub struct UnconfiguredSecretStore;

#[async_trait]
impl SecretStore for UnconfiguredSecretStore {
    async fn get(&self, _r: &CredentialRef) -> Result<SecretString, SecretError> {
        Err(SecretError::Backend(
            "no secret store is configured in this build".into(),
        ))
    }
}
