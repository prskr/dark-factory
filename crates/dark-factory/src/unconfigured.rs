//! Placeholders for the sandbox provider and secret store that fail every operation, for
//! `serve --no-cluster` and tests of the control plane that do not talk to a cluster.

pub(crate) mod sandbox_placeholders {
    use async_trait::async_trait;
    use df_core::{
        CredentialRef, PeerIdentity, ProviderError, RunId, RunSandboxSpec, SandboxEvent,
        SandboxProvider, SecretError, SecretStore, TaskVolumeSpec,
    };
    use futures::stream::{self, BoxStream, StreamExt};
    use secrecy::SecretString;

    const NOT_CONFIGURED: &str = "no sandbox provider is configured in this build";

    pub(crate) struct UnconfiguredSandboxProvider;

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

    pub(crate) struct UnconfiguredSecretStore;

    #[async_trait]
    impl SecretStore for UnconfiguredSecretStore {
        async fn get(&self, _r: &CredentialRef) -> Result<SecretString, SecretError> {
            Err(SecretError::Backend(
                "no secret store is configured in this build".into(),
            ))
        }
    }
}
