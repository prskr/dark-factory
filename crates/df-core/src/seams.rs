//! Provider and integration trait seams (`SandboxProvider`, `ForgeClient`, `SecretStore`)
//! per design D3.

use std::collections::BTreeMap;
use std::net::IpAddr;

use async_trait::async_trait;
use futures::stream::BoxStream;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ids::{CredentialRefId, PersonaId, ProjectId, RunId};
use crate::keys::{ProjectKey, TaskKey};

// ---------------------------------------------------------------------------
// SandboxProvider
// ---------------------------------------------------------------------------

/// Specification for ensuring a task's persistent workspace volume exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskVolumeSpec {
    pub task_key: TaskKey,
    pub volume_name: String,
    pub storage_size: Option<String>,
}

impl TaskVolumeSpec {
    /// Creates a [`TaskVolumeSpec`] with the derived volume name for `task_key`.
    pub fn for_task(task_key: TaskKey) -> Self {
        let volume_name = task_key.volume_name();
        Self {
            task_key,
            volume_name,
            storage_size: None,
        }
    }
}

/// Specification for provisioning a per-run sandbox pod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSandboxSpec {
    pub run_id: RunId,
    pub run_number: u64,
    pub task_key: TaskKey,
    pub project_id: ProjectId,
    pub project_key: ProjectKey,
    pub persona_id: PersonaId,
    pub image: String,
    /// Harness whose image the sandbox mounts read-only at `/opt/harness/<harness>`; `None` when
    /// the project image brings its own adapter.
    pub harness: Option<String>,
    pub volume_name: String,
    pub labels: BTreeMap<String, String>,
}

/// Identity extracted from a sandbox peer's mTLS connection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerIdentity {
    /// SPIFFE URI SAN from the client certificate, e.g.
    /// `spiffe://dark-factory/ns/<ns>/pod/<name>/<uid>`.
    pub uri_san: String,
    /// Source IP address of the TCP connection.
    pub peer_ip: IpAddr,
}

/// Lifecycle events emitted by [`SandboxProvider::events`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxEvent {
    Started { run_id: RunId, pod_ip: IpAddr },
    Gone { run_id: RunId, reason: String },
}

/// Errors returned by [`SandboxProvider`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProviderError {
    #[error("sandbox resource not found: {0}")]
    NotFound(String),
    #[error("invalid sandbox specification: {0}")]
    InvalidSpec(String),
    #[error("kubernetes / provider failure: {0}")]
    Backend(String),
}

/// Abstracts sandbox and task volume provisioning (Kubernetes `agent-sandbox` in production).
#[async_trait]
pub trait SandboxProvider: Send + Sync {
    async fn ensure_task_volume(&self, task: &TaskVolumeSpec) -> Result<(), ProviderError>;
    async fn provision(&self, spec: &RunSandboxSpec) -> Result<(), ProviderError>;
    async fn teardown(&self, run_id: RunId) -> Result<(), ProviderError>;
    /// Peer = URI SAN from the client cert + peer IP. `None` = reject.
    async fn resolve_identity(&self, peer: &PeerIdentity) -> Option<RunId>;
    /// Stream of `Started { run_id, pod_ip }` and `Gone { run_id, reason }` transitions.
    fn events(&self) -> BoxStream<'static, SandboxEvent>;
}

// ---------------------------------------------------------------------------
// ForgeClient (mirrors darkfactory:integrations@0.1.0)
// ---------------------------------------------------------------------------

/// Repository owner and name (`darkfactory:integrations/types.repo-ref`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}

/// Identifies a pull request (`darkfactory:integrations/types.pull-request-ref`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PullRequestRef {
    pub repo: RepoRef,
    pub number: u64,
}

/// Identifies an issue (`darkfactory:integrations/types.issue-ref`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IssueRef {
    pub repo: RepoRef,
    pub number: u64,
}

/// Rate-limit metadata (`darkfactory:integrations/types.rate-limit`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimit {
    pub retry_after_seconds: Option<u32>,
    pub message: String,
}

/// Upstream HTTP error metadata (`darkfactory:integrations/types.upstream-error`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpstreamError {
    pub status: u16,
    pub message: String,
}

/// Mirrors `darkfactory:integrations@0.1.0` `types.error` variant one-to-one.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum IntegrationError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("rate limited: {}", .0.message)]
    RateLimited(RateLimit),
    #[error("upstream error (HTTP {}): {}", .0.status, .0.message)]
    Upstream(UpstreamError),
    #[error("plugin configuration error: {0}")]
    Config(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("internal plugin error: {0}")]
    Internal(String),
}

/// Pull request state (`darkfactory:integrations/pull-requests.pull-request-state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

/// Pull request record (`darkfactory:integrations/pull-requests.pull-request`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    pub id: PullRequestRef,
    pub url: String,
    pub head: String,
    pub base: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
    pub state: PullRequestState,
    pub head_sha: String,
}

/// Request to open a pull request (`darkfactory:integrations/pull-requests.new-pull-request`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewPullRequest {
    pub repo: RepoRef,
    pub head: String,
    pub base: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

/// Partial update to a pull request (`darkfactory:integrations/pull-requests.pull-request-update`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestUpdate {
    pub title: Option<String>,
    pub body: Option<String>,
    pub draft: Option<bool>,
}

/// Issue state (`darkfactory:integrations/issues.issue-state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueState {
    Open,
    Closed,
}

/// Issue record (`darkfactory:integrations/issues.issue`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: IssueRef,
    pub url: String,
    pub title: String,
    pub body: String,
    pub state: IssueState,
    pub labels: Vec<String>,
}

/// Comment record (`darkfactory:integrations/issues.comment`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub url: String,
}

/// Host-side interface to code forge and issue tracker capability plugins.
#[async_trait]
pub trait ForgeClient: Send + Sync {
    async fn create_pull_request(
        &self,
        p: &ProjectId,
        req: NewPullRequest,
    ) -> Result<PullRequest, IntegrationError>;

    async fn find_open_pull_request(
        &self,
        p: &ProjectId,
        head: &str,
        base: &str,
    ) -> Result<Option<PullRequest>, IntegrationError>;

    async fn update_pull_request(
        &self,
        p: &ProjectId,
        number: u64,
        ch: PullRequestUpdate,
    ) -> Result<PullRequest, IntegrationError>;

    async fn get_issue(&self, p: &ProjectId, number: u64) -> Result<Issue, IntegrationError>;

    async fn add_issue_comment(
        &self,
        p: &ProjectId,
        number: u64,
        body: &str,
    ) -> Result<Comment, IntegrationError>;
}

// ---------------------------------------------------------------------------
// SecretStore
// ---------------------------------------------------------------------------

/// A reference to a secret stored in the cluster (Kubernetes Secret name + key).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CredentialRef {
    pub id: CredentialRefId,
    pub name: String,
    pub secret_name: String,
    pub secret_key: String,
}

/// Errors returned by [`SecretStore`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SecretError {
    #[error("secret `{secret_name}` (key `{secret_key}`) not found")]
    NotFound {
        secret_name: String,
        secret_key: String,
    },
    #[error("failed to read secret: {0}")]
    Backend(String),
}

/// Reads credential values from the backing secret store (Kubernetes Secrets in production).
#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn get(&self, r: &CredentialRef) -> Result<SecretString, SecretError>;
}
