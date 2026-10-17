//! Core domain types, instance configuration, and provider trait seams for Dark Factory.

pub mod config;
pub mod domain;
pub mod ids;
pub mod keys;
pub mod seams;
#[cfg(any(test, feature = "test-util"))]
pub mod testing;

pub use config::{ConfigError, DatabaseConfig, InstanceConfig};
pub use domain::{ParseEnumError, Principal, RunOutcome, RunState, TaskStatus};
pub use ids::{
    CredentialRefId, EgressEntryId, PersonaId, ProjectId, PromptId, RunId, TaskId, ToolCallId,
    UserId,
};
pub use keys::{KeyError, ProjectKey, TaskKey};
pub use seams::{
    Comment, CredentialRef, ForgeClient, IntegrationError, Issue, IssueRef, IssueState,
    NewPullRequest, PeerIdentity, ProviderError, PullRequest, PullRequestRef, PullRequestState,
    PullRequestUpdate, RateLimit, RepoRef, RunSandboxSpec, SandboxEvent, SandboxProvider,
    SecretError, SecretStore, TaskVolumeSpec, UpstreamError,
};
