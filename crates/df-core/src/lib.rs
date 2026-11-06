//! Core domain types, instance configuration, and provider trait seams for Dark Factory.

pub mod config;
pub mod domain;
pub mod forge;
pub mod harness;
pub mod header_template;
pub mod host;
pub mod ids;
pub mod keys;
pub mod lifecycle;
pub mod model_provider;
pub mod public_address;
pub mod routing;
pub mod seams;
pub mod task_transition;
#[cfg(any(test, feature = "test-util"))]
pub mod testing;
pub mod title;
pub mod webhooks;

pub use config::{
    AuthConfig, AuthScheme, CLAUDE_CODE_HARNESS, ClusterTargetsConfig, ConfigError, DatabaseConfig,
    InstanceConfig, NetworkConfig, PluginConfig, SandboxEndpointsConfig, SandboxProvisioningConfig,
    SandboxSectionConfig,
};
pub use domain::{
    CAPACITY_DETAIL, DependencyKind, PERSONA_DESCRIPTION_MAX_CHARS, ParseEnumError, Principal,
    RunOutcome, RunState, StatusGroup, StatusSet, TaskStatus, is_satisfied,
};
pub use forge::ForgeKind;
pub use ids::{
    CredentialRefId, EgressEntryId, PersonaId, ProjectId, PromptId, RequestId, RunId, TaskId,
    ToolCallId, UserId,
};
pub use keys::{KeyError, ProjectKey, TaskKey};
pub use lifecycle::{EventType, InstanceSubscription, LifecycleEventsConfig};
pub use seams::{
    Author, AuthorRole, Comment, CredentialRef, Feedback, ForgeClient, IntegrationError, Issue,
    IssueRef, IssueState, NewPullRequest, PeerIdentity, PrComment, ProviderError, PullRequest,
    PullRequestRef, PullRequestState, PullRequestUpdate, RateLimit, RepoRef, Review, ReviewComment,
    ReviewThread, ReviewVerdict, RunSandboxSpec, SandboxEvent, SandboxProvider, SecretError,
    SecretStore, TaskVolumeSpec, UpstreamError,
};
pub use title::{
    FALLBACK_TITLE, TITLE_MAX_CHARS, TitleSource, placeholder_title, sanitize_title, title_too_long,
};
