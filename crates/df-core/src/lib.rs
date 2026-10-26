//! Core domain types, instance configuration, and provider trait seams for Dark Factory.

pub mod config;
pub mod domain;
pub mod forge;
pub mod host;
pub mod ids;
pub mod keys;
pub mod seams;
#[cfg(any(test, feature = "test-util"))]
pub mod testing;
pub mod title;

pub use config::{
    AuthConfig, CLAUDE_CODE_HARNESS, ClusterTargetsConfig, ConfigError, DatabaseConfig,
    InstanceConfig, PluginConfig, SandboxEndpointsConfig, SandboxProvisioningConfig,
    SandboxSectionConfig,
};
pub use domain::{
    ParseEnumError, Principal, RunOutcome, RunState, StatusGroup, StatusSet, TaskStatus,
};
pub use forge::ForgeKind;
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
pub use title::{
    FALLBACK_TITLE, TITLE_MAX_CHARS, TitleSource, placeholder_title, sanitize_title, title_too_long,
};
