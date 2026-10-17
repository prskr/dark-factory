//! In-memory fake implementations of [`SandboxProvider`], [`ForgeClient`], and [`SecretStore`]
//! for unit and integration testing across workstreams.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::BoxStream;
use secrecy::SecretString;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::ids::{ProjectId, RunId};
use crate::seams::{
    Comment, CredentialRef, ForgeClient, IntegrationError, Issue, NewPullRequest, PeerIdentity,
    ProviderError, PullRequest, PullRequestRef, PullRequestState, PullRequestUpdate,
    RunSandboxSpec, SandboxEvent, SandboxProvider, SecretError, SecretStore, TaskVolumeSpec,
};

// ---------------------------------------------------------------------------
// FakeSandboxProvider
// ---------------------------------------------------------------------------

/// Recorded method invocation on [`FakeSandboxProvider`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxCall {
    EnsureTaskVolume(TaskVolumeSpec),
    Provision(RunSandboxSpec),
    Teardown(RunId),
    ResolveIdentity(PeerIdentity),
}

#[derive(Default)]
struct SandboxState {
    calls: Vec<SandboxCall>,
    identities: HashMap<PeerIdentity, RunId>,
    ensure_volume_results: VecDeque<Result<(), ProviderError>>,
    provision_results: VecDeque<Result<(), ProviderError>>,
    teardown_results: VecDeque<Result<(), ProviderError>>,
}

/// In-memory fake [`SandboxProvider`] that records calls and allows scripting results and events.
#[derive(Clone)]
pub struct FakeSandboxProvider {
    state: Arc<Mutex<SandboxState>>,
    events_tx: broadcast::Sender<SandboxEvent>,
}

impl Default for FakeSandboxProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeSandboxProvider {
    pub fn new() -> Self {
        let (events_tx, _) = broadcast::channel(256);
        Self {
            state: Arc::new(Mutex::new(SandboxState::default())),
            events_tx,
        }
    }

    /// Returns all calls recorded so far in order.
    pub fn calls(&self) -> Vec<SandboxCall> {
        self.state.lock().expect("lock poisoned").calls.clone()
    }

    /// Registers a peer identity to resolve to `run_id`.
    pub fn bind_identity(&self, peer: PeerIdentity, run_id: RunId) {
        self.state
            .lock()
            .expect("lock poisoned")
            .identities
            .insert(peer, run_id);
    }

    /// Queues a scripted result for the next `ensure_task_volume` call.
    pub fn push_ensure_volume_result(&self, result: Result<(), ProviderError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .ensure_volume_results
            .push_back(result);
    }

    /// Queues a scripted result for the next `provision` call.
    pub fn push_provision_result(&self, result: Result<(), ProviderError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .provision_results
            .push_back(result);
    }

    /// Queues a scripted result for the next `teardown` call.
    pub fn push_teardown_result(&self, result: Result<(), ProviderError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .teardown_results
            .push_back(result);
    }

    /// Emits a [`SandboxEvent`] to all active stream subscribers.
    pub fn emit_event(&self, event: SandboxEvent) {
        let _ = self.events_tx.send(event);
    }
}

#[async_trait]
impl SandboxProvider for FakeSandboxProvider {
    async fn ensure_task_volume(&self, task: &TaskVolumeSpec) -> Result<(), ProviderError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state
            .calls
            .push(SandboxCall::EnsureTaskVolume(task.clone()));
        state.ensure_volume_results.pop_front().unwrap_or(Ok(()))
    }

    async fn provision(&self, spec: &RunSandboxSpec) -> Result<(), ProviderError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(SandboxCall::Provision(spec.clone()));
        state.provision_results.pop_front().unwrap_or(Ok(()))
    }

    async fn teardown(&self, run_id: RunId) -> Result<(), ProviderError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(SandboxCall::Teardown(run_id));
        state.teardown_results.pop_front().unwrap_or(Ok(()))
    }

    async fn resolve_identity(&self, peer: &PeerIdentity) -> Option<RunId> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(SandboxCall::ResolveIdentity(peer.clone()));
        state.identities.get(peer).copied()
    }

    fn events(&self) -> BoxStream<'static, SandboxEvent> {
        let rx = self.events_tx.subscribe();
        BroadcastStream::new(rx)
            .filter_map(|item| async move { item.ok() })
            .boxed()
    }
}

// ---------------------------------------------------------------------------
// FakeForgeClient
// ---------------------------------------------------------------------------

/// Recorded method invocation on [`FakeForgeClient`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeCall {
    CreatePullRequest {
        project_id: ProjectId,
        request: NewPullRequest,
    },
    FindOpenPullRequest {
        project_id: ProjectId,
        head: String,
        base: String,
    },
    UpdatePullRequest {
        project_id: ProjectId,
        number: u64,
        changes: PullRequestUpdate,
    },
    GetIssue {
        project_id: ProjectId,
        number: u64,
    },
    AddIssueComment {
        project_id: ProjectId,
        number: u64,
        body: String,
    },
}

#[derive(Default)]
struct ForgeState {
    calls: Vec<ForgeCall>,
    next_pr_number: u64,
    next_comment_id: u64,
    pull_requests: HashMap<(ProjectId, u64), PullRequest>,
    issues: HashMap<(ProjectId, u64), Issue>,
    create_pr_results: VecDeque<Result<PullRequest, IntegrationError>>,
    find_open_results: VecDeque<Result<Option<PullRequest>, IntegrationError>>,
    update_pr_results: VecDeque<Result<PullRequest, IntegrationError>>,
    get_issue_results: VecDeque<Result<Issue, IntegrationError>>,
    add_comment_results: VecDeque<Result<Comment, IntegrationError>>,
}

/// In-memory fake [`ForgeClient`] that records calls and supports both default
/// in-memory PR/issue tracking and scripted results/errors.
#[derive(Clone, Default)]
pub struct FakeForgeClient {
    state: Arc<Mutex<ForgeState>>,
}

impl FakeForgeClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all calls recorded so far in order.
    pub fn calls(&self) -> Vec<ForgeCall> {
        self.state.lock().expect("lock poisoned").calls.clone()
    }

    /// Seeds an issue into the fake tracker for `project_id`.
    pub fn insert_issue(&self, project_id: ProjectId, issue: Issue) {
        self.state
            .lock()
            .expect("lock poisoned")
            .issues
            .insert((project_id, issue.id.number), issue);
    }

    /// Queues a scripted result for `create_pull_request`.
    pub fn push_create_pull_request_result(&self, res: Result<PullRequest, IntegrationError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .create_pr_results
            .push_back(res);
    }

    /// Queues a scripted result for `find_open_pull_request`.
    pub fn push_find_open_pull_request_result(
        &self,
        res: Result<Option<PullRequest>, IntegrationError>,
    ) {
        self.state
            .lock()
            .expect("lock poisoned")
            .find_open_results
            .push_back(res);
    }

    /// Queues a scripted result for `update_pull_request`.
    pub fn push_update_pull_request_result(&self, res: Result<PullRequest, IntegrationError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .update_pr_results
            .push_back(res);
    }

    /// Queues a scripted result for `get_issue`.
    pub fn push_get_issue_result(&self, res: Result<Issue, IntegrationError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .get_issue_results
            .push_back(res);
    }

    /// Queues a scripted result for `add_issue_comment`.
    pub fn push_add_issue_comment_result(&self, res: Result<Comment, IntegrationError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .add_comment_results
            .push_back(res);
    }
}

#[async_trait]
impl ForgeClient for FakeForgeClient {
    async fn create_pull_request(
        &self,
        p: &ProjectId,
        req: NewPullRequest,
    ) -> Result<PullRequest, IntegrationError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(ForgeCall::CreatePullRequest {
            project_id: *p,
            request: req.clone(),
        });
        if let Some(scripted) = state.create_pr_results.pop_front() {
            if let Ok(ref pr) = scripted {
                state.next_pr_number = state.next_pr_number.max(pr.id.number);
                state.pull_requests.insert((*p, pr.id.number), pr.clone());
            }
            return scripted;
        }
        state.next_pr_number += 1;
        let number = state.next_pr_number;
        let pr = PullRequest {
            id: PullRequestRef {
                repo: req.repo.clone(),
                number,
            },
            url: format!(
                "https://forge.example.test/{}/{}/pulls/{number}",
                req.repo.owner, req.repo.name
            ),
            head: req.head,
            base: req.base,
            title: req.title,
            body: req.body,
            draft: req.draft,
            state: PullRequestState::Open,
            head_sha: "0000000000000000000000000000000000000000".to_string(),
        };
        state.pull_requests.insert((*p, number), pr.clone());
        Ok(pr)
    }

    async fn find_open_pull_request(
        &self,
        p: &ProjectId,
        head: &str,
        base: &str,
    ) -> Result<Option<PullRequest>, IntegrationError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(ForgeCall::FindOpenPullRequest {
            project_id: *p,
            head: head.to_string(),
            base: base.to_string(),
        });
        if let Some(scripted) = state.find_open_results.pop_front() {
            return scripted;
        }
        let found = state.pull_requests.iter().find_map(|((proj, _), pr)| {
            (proj == p && pr.state == PullRequestState::Open && pr.head == head && pr.base == base)
                .then(|| pr.clone())
        });
        Ok(found)
    }

    async fn update_pull_request(
        &self,
        p: &ProjectId,
        number: u64,
        ch: PullRequestUpdate,
    ) -> Result<PullRequest, IntegrationError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(ForgeCall::UpdatePullRequest {
            project_id: *p,
            number,
            changes: ch.clone(),
        });
        if let Some(scripted) = state.update_pr_results.pop_front() {
            return scripted;
        }
        let pr = state
            .pull_requests
            .get_mut(&(*p, number))
            .ok_or_else(|| IntegrationError::NotFound(format!("PR #{number} not found")))?;
        if let Some(title) = ch.title {
            pr.title = title;
        }
        if let Some(body) = ch.body {
            pr.body = body;
        }
        if let Some(draft) = ch.draft {
            pr.draft = draft;
        }
        Ok(pr.clone())
    }

    async fn get_issue(&self, p: &ProjectId, number: u64) -> Result<Issue, IntegrationError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(ForgeCall::GetIssue {
            project_id: *p,
            number,
        });
        if let Some(scripted) = state.get_issue_results.pop_front() {
            return scripted;
        }
        state
            .issues
            .get(&(*p, number))
            .cloned()
            .ok_or_else(|| IntegrationError::NotFound(format!("issue #{number} not found")))
    }

    async fn add_issue_comment(
        &self,
        p: &ProjectId,
        number: u64,
        body: &str,
    ) -> Result<Comment, IntegrationError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(ForgeCall::AddIssueComment {
            project_id: *p,
            number,
            body: body.to_string(),
        });
        if let Some(scripted) = state.add_comment_results.pop_front() {
            return scripted;
        }
        state.next_comment_id += 1;
        let id = state.next_comment_id.to_string();
        Ok(Comment {
            url: format!("https://forge.example.test/issues/{number}#comment-{id}"),
            id,
        })
    }
}

// ---------------------------------------------------------------------------
// FakeSecretStore
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SecretStoreState {
    calls: Vec<CredentialRef>,
    entries: HashMap<(String, String), Result<SecretString, SecretError>>,
}

/// In-memory fake [`SecretStore`] that records [`CredentialRef`] lookups and returns seeded secrets.
#[derive(Clone, Default)]
pub struct FakeSecretStore {
    state: Arc<Mutex<SecretStoreState>>,
}

impl FakeSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all [`CredentialRef`] lookups recorded so far.
    pub fn calls(&self) -> Vec<CredentialRef> {
        self.state.lock().expect("lock poisoned").calls.clone()
    }

    /// Seeds a secret value for `(secret_name, secret_key)`.
    pub fn insert(
        &self,
        secret_name: impl Into<String>,
        secret_key: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.state.lock().expect("lock poisoned").entries.insert(
            (secret_name.into(), secret_key.into()),
            Ok(SecretString::from(value.into())),
        );
    }

    /// Seeds an error for `(secret_name, secret_key)`.
    pub fn insert_error(
        &self,
        secret_name: impl Into<String>,
        secret_key: impl Into<String>,
        err: SecretError,
    ) {
        self.state
            .lock()
            .expect("lock poisoned")
            .entries
            .insert((secret_name.into(), secret_key.into()), Err(err));
    }
}

#[async_trait]
impl SecretStore for FakeSecretStore {
    async fn get(&self, r: &CredentialRef) -> Result<SecretString, SecretError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(r.clone());
        match state
            .entries
            .get(&(r.secret_name.clone(), r.secret_key.clone()))
        {
            Some(Ok(secret)) => Ok(secret.clone()),
            Some(Err(err)) => Err(err.clone()),
            None => Err(SecretError::NotFound {
                secret_name: r.secret_name.clone(),
                secret_key: r.secret_key.clone(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{CredentialRefId, PersonaId};
    use crate::keys::TaskKey;
    use crate::seams::{IssueRef, IssueState, RepoRef};
    use secrecy::ExposeSecret;
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn fake_sandbox_provider_records_calls_scripts_results_and_streams_events() {
        let provider = FakeSandboxProvider::new();
        let mut events = provider.events();

        let task_key: TaskKey = "DF-1".parse().unwrap();
        let vol_spec = TaskVolumeSpec::for_task(task_key.clone());
        provider.ensure_task_volume(&vol_spec).await.unwrap();

        let run_id = RunId::new();
        let sandbox_spec = RunSandboxSpec {
            run_id,
            run_number: 1,
            task_key: task_key.clone(),
            project_id: ProjectId::new(),
            project_key: task_key.project().clone(),
            persona_id: PersonaId::new(),
            image: "harbor.example.test/df/project:latest".into(),
            volume_name: vol_spec.volume_name.clone(),
            labels: BTreeMap::new(),
        };
        provider.provision(&sandbox_spec).await.unwrap();

        provider.push_provision_result(Err(ProviderError::Backend("quota exceeded".into())));
        assert_eq!(
            provider.provision(&sandbox_spec).await,
            Err(ProviderError::Backend("quota exceeded".into()))
        );

        let peer = PeerIdentity {
            uri_san: "spiffe://dark-factory/ns/sandboxes/pod/run-df-1-1/uid-1".into(),
            peer_ip: IpAddr::V4(Ipv4Addr::new(10, 244, 1, 15)),
        };
        assert_eq!(provider.resolve_identity(&peer).await, None);
        provider.bind_identity(peer.clone(), run_id);
        assert_eq!(provider.resolve_identity(&peer).await, Some(run_id));

        let started = SandboxEvent::Started {
            run_id,
            pod_ip: peer.peer_ip,
        };
        provider.emit_event(started.clone());
        let received = events.next().await.expect("should receive event");
        assert_eq!(received, started);

        provider.teardown(run_id).await.unwrap();
        assert_eq!(provider.calls().len(), 6);
    }

    #[tokio::test]
    async fn fake_forge_client_records_calls_and_supports_in_memory_and_scripted_responses() {
        let forge = FakeForgeClient::new();
        let project_id = ProjectId::new();
        let repo = RepoRef {
            owner: "prskr".into(),
            name: "dark-factory".into(),
        };

        let created = forge
            .create_pull_request(
                &project_id,
                NewPullRequest {
                    repo: repo.clone(),
                    head: "factory/task/DF-1".into(),
                    base: "main".into(),
                    title: "Initial implementation".into(),
                    body: "Details".into(),
                    draft: true,
                },
            )
            .await
            .unwrap();
        assert_eq!(created.id.number, 1);
        assert!(created.draft);

        let found = forge
            .find_open_pull_request(&project_id, "factory/task/DF-1", "main")
            .await
            .unwrap();
        assert_eq!(found, Some(created.clone()));

        let updated = forge
            .update_pull_request(
                &project_id,
                1,
                PullRequestUpdate {
                    title: Some("Ready PR".into()),
                    body: None,
                    draft: Some(false),
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.title, "Ready PR");
        assert!(!updated.draft);

        let issue = Issue {
            id: IssueRef {
                repo: repo.clone(),
                number: 7,
            },
            url: "https://forge.example.test/prskr/dark-factory/issues/7".into(),
            title: "Bug report".into(),
            body: "Steps to reproduce".into(),
            state: IssueState::Open,
            labels: vec!["bug".into()],
        };
        forge.insert_issue(project_id, issue.clone());
        assert_eq!(forge.get_issue(&project_id, 7).await.unwrap(), issue);

        let comment = forge
            .add_issue_comment(&project_id, 7, "Fixed in PR #1")
            .await
            .unwrap();
        assert_eq!(comment.id, "1");

        // Scripted Ok(PR #5) advances next_pr_number so the next unscripted PR gets #6
        let scripted_pr = PullRequest {
            id: PullRequestRef {
                repo: repo.clone(),
                number: 5,
            },
            url: "https://forge.example.test/prskr/dark-factory/pulls/5".into(),
            head: "factory/task/DF-5".into(),
            base: "main".into(),
            title: "Scripted PR".into(),
            body: String::new(),
            draft: true,
            state: PullRequestState::Open,
            head_sha: "abc".into(),
        };
        forge.push_create_pull_request_result(Ok(scripted_pr));
        let got_scripted = forge
            .create_pull_request(
                &project_id,
                NewPullRequest {
                    repo: repo.clone(),
                    head: "factory/task/DF-5".into(),
                    base: "main".into(),
                    title: "Scripted PR".into(),
                    body: String::new(),
                    draft: true,
                },
            )
            .await
            .unwrap();
        assert_eq!(got_scripted.id.number, 5);

        let next_unscripted = forge
            .create_pull_request(
                &project_id,
                NewPullRequest {
                    repo: repo.clone(),
                    head: "factory/task/DF-6".into(),
                    base: "main".into(),
                    title: "Next PR".into(),
                    body: String::new(),
                    draft: true,
                },
            )
            .await
            .unwrap();
        assert_eq!(next_unscripted.id.number, 6);

        forge.push_create_pull_request_result(Err(IntegrationError::Conflict(
            "PR already open".into(),
        )));
        let conflict = forge
            .create_pull_request(
                &project_id,
                NewPullRequest {
                    repo,
                    head: "factory/task/DF-1".into(),
                    base: "main".into(),
                    title: "Dup".into(),
                    body: String::new(),
                    draft: true,
                },
            )
            .await;
        assert_eq!(
            conflict,
            Err(IntegrationError::Conflict("PR already open".into()))
        );
        assert_eq!(forge.calls().len(), 8);
    }

    #[tokio::test]
    async fn fake_secret_store_records_calls_and_returns_seeded_secrets() {
        let store = FakeSecretStore::new();
        store.insert("claude-sub", "oauth-token", "sk-ant-oat01-secret");

        let cred_ref = CredentialRef {
            id: CredentialRefId::new(),
            name: "claude".into(),
            secret_name: "claude-sub".into(),
            secret_key: "oauth-token".into(),
        };
        let secret = store.get(&cred_ref).await.unwrap();
        assert_eq!(secret.expose_secret(), "sk-ant-oat01-secret");

        let missing_ref = CredentialRef {
            id: CredentialRefId::new(),
            name: "missing".into(),
            secret_name: "nope".into(),
            secret_key: "key".into(),
        };
        assert_eq!(
            store.get(&missing_ref).await.unwrap_err(),
            SecretError::NotFound {
                secret_name: "nope".into(),
                secret_key: "key".into()
            }
        );

        assert_eq!(store.calls(), vec![cred_ref, missing_ref]);
    }
}
