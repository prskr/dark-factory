//! In-memory fake implementations of [`SandboxProvider`], [`ForgeClient`], and [`SecretStore`]
//! for unit and integration testing across workstreams.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::BoxStream;
use secrecy::SecretString;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::ids::{ProjectId, RunId};
use crate::seams::{
    AuthorRole, Comment, Feedback, ForgeClient, IntegrationError, Issue, NewPullRequest,
    PeerIdentity, ProviderError, PullRequest, PullRequestRef, PullRequestState, PullRequestUpdate,
    RunSandboxSpec, SandboxEvent, SandboxProvider, SecretError, SecretKeyRef, SecretStore,
    TaskVolumeSpec,
};

// ---------------------------------------------------------------------------
// FakeSandboxProvider
// ---------------------------------------------------------------------------

/// Recorded method invocation on [`FakeSandboxProvider`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxCall {
    EnsureTaskVolume(TaskVolumeSpec),
    DeleteTaskVolume(TaskVolumeSpec),
    Provision(RunSandboxSpec),
    Teardown(RunId),
    ResolveIdentity(PeerIdentity),
}

#[derive(Default)]
struct SandboxState {
    calls: Vec<SandboxCall>,
    identities: HashMap<PeerIdentity, RunId>,
    ensure_volume_results: VecDeque<Result<(), ProviderError>>,
    delete_volume_results: VecDeque<Result<(), ProviderError>>,
    provision_results: VecDeque<Result<(), ProviderError>>,
    teardown_results: VecDeque<Result<(), ProviderError>>,
    exists_results: VecDeque<Result<bool, ProviderError>>,
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

    /// Queues a scripted result for the next `delete_task_volume` call.
    pub fn push_delete_volume_result(&self, result: Result<(), ProviderError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .delete_volume_results
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

    /// Queues a scripted result for the next `sandbox_exists` call; without one the answer is
    /// whether a `provision` call of the run was recorded and not followed by a `teardown`.
    pub fn push_exists_result(&self, result: Result<bool, ProviderError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .exists_results
            .push_back(result);
    }

    /// Emits a [`SandboxEvent`] to all active stream subscribers.
    pub fn emit_event(&self, event: SandboxEvent) {
        self.events_tx.send(event).ok(); // no subscriber is fine
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

    async fn delete_task_volume(&self, task: &TaskVolumeSpec) -> Result<(), ProviderError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state
            .calls
            .push(SandboxCall::DeleteTaskVolume(task.clone()));
        state.delete_volume_results.pop_front().unwrap_or(Ok(()))
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

    async fn sandbox_exists(&self, run_id: RunId) -> Result<bool, ProviderError> {
        let mut state = self.state.lock().expect("lock poisoned");
        if let Some(result) = state.exists_results.pop_front() {
            return result;
        }
        let last = state.calls.iter().rev().find_map(|call| match call {
            SandboxCall::Provision(spec) if spec.run_id == run_id => Some(true),
            SandboxCall::Teardown(id) if *id == run_id => Some(false),
            _ => None,
        });
        Ok(last.unwrap_or(false))
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
    ListPullRequestFeedback {
        project_id: ProjectId,
        number: u64,
        since: Option<String>,
    },
    ReplyToReviewThread {
        project_id: ProjectId,
        number: u64,
        thread: String,
        body: String,
    },
    Whoami {
        project_id: ProjectId,
    },
    AuthorRole {
        project_id: ProjectId,
        login: String,
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
    hang_find_open: bool,
    hang_update_pr: bool,
    find_open_gate: Option<Arc<tokio::sync::Semaphore>>,
    comment_gate: Option<Arc<tokio::sync::Semaphore>>,
    update_pr_results: VecDeque<Result<PullRequest, IntegrationError>>,
    get_issue_results: VecDeque<Result<Issue, IntegrationError>>,
    add_comment_results: VecDeque<Result<Comment, IntegrationError>>,
    feedback: HashMap<(ProjectId, u64), Feedback>,
    feedback_results: VecDeque<Result<Feedback, IntegrationError>>,
    reply_results: VecDeque<Result<Comment, IntegrationError>>,
    bot_login: Option<String>,
    roles: HashMap<String, AuthorRole>,
    whoami_results: VecDeque<Result<String, IntegrationError>>,
}

/// In-memory fake [`ForgeClient`] that records calls and supports both default
/// in-memory PR/issue tracking and scripted results/errors.
#[derive(Clone, Default)]
pub struct FakeForgeClient {
    state: Arc<Mutex<ForgeState>>,
}

impl FakeForgeClient {
    /// Login `whoami` answers unless [`FakeForgeClient::set_bot_login`] changed it.
    pub const DEFAULT_BOT_LOGIN: &str = "df-bot";

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

    /// Makes `find_open_pull_request` record the call and then never return, like a forge that
    /// does not answer.
    pub fn hang_find_open_pull_request(&self) {
        self.state.lock().expect("lock poisoned").hang_find_open = true;
    }

    /// Makes `update_pull_request` record the call and then never return, like a forge that does
    /// not answer.
    pub fn hang_update_pull_request(&self) {
        self.state.lock().expect("lock poisoned").hang_update_pr = true;
    }

    /// Makes `add_issue_comment` record the call and then wait until a permit of the returned
    /// semaphore is added, like a slow forge.
    pub fn hold_issue_comments(&self) -> Arc<tokio::sync::Semaphore> {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        self.state.lock().expect("lock poisoned").comment_gate = Some(gate.clone());
        gate
    }

    /// Makes `find_open_pull_request` record the call and then wait until a permit of the returned
    /// semaphore is added, like a slow forge.
    pub fn hold_find_open_pull_request(&self) -> Arc<tokio::sync::Semaphore> {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        self.state.lock().expect("lock poisoned").find_open_gate = Some(gate.clone());
        gate
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

    /// Seeds the feedback of pull request `number`; `list_pull_request_feedback` returns it as is,
    /// ignoring `since`, and `reply_to_review_thread` accepts its thread ids.
    pub fn insert_feedback(&self, project_id: ProjectId, number: u64, feedback: Feedback) {
        self.state
            .lock()
            .expect("lock poisoned")
            .feedback
            .insert((project_id, number), feedback);
    }

    /// Queues a scripted result for `list_pull_request_feedback`.
    pub fn push_list_pull_request_feedback_result(&self, res: Result<Feedback, IntegrationError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .feedback_results
            .push_back(res);
    }

    /// Queues a scripted result for `reply_to_review_thread`.
    pub fn push_reply_to_review_thread_result(&self, res: Result<Comment, IntegrationError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .reply_results
            .push_back(res);
    }

    /// Sets the login `whoami` answers; [`FakeForgeClient::DEFAULT_BOT_LOGIN`] without one.
    pub fn set_bot_login(&self, login: impl Into<String>) {
        self.state.lock().expect("lock poisoned").bot_login = Some(login.into());
    }

    /// Queues a scripted result for `whoami`.
    pub fn push_whoami_result(&self, res: Result<String, IntegrationError>) {
        self.state
            .lock()
            .expect("lock poisoned")
            .whoami_results
            .push_back(res);
    }

    /// Sets the role `author_role` answers for `login`; [`AuthorRole::Outside`] without one.
    pub fn set_author_role(&self, login: impl Into<String>, role: AuthorRole) {
        self.state
            .lock()
            .expect("lock poisoned")
            .roles
            .insert(login.into(), role);
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
        let (hang, gate) = {
            let mut state = self.state.lock().expect("lock poisoned");
            state.calls.push(ForgeCall::FindOpenPullRequest {
                project_id: *p,
                head: head.to_string(),
                base: base.to_string(),
            });
            (state.hang_find_open, state.find_open_gate.clone())
        };
        if hang {
            return std::future::pending().await;
        }
        // The answer reflects the forge as of the call; the gate only delays its delivery.
        let answer = {
            let mut state = self.state.lock().expect("lock poisoned");
            if let Some(scripted) = state.find_open_results.pop_front() {
                scripted
            } else {
                Ok(state.pull_requests.iter().find_map(|((proj, _), pr)| {
                    (proj == p
                        && pr.state == PullRequestState::Open
                        && pr.head == head
                        && pr.base == base)
                        .then(|| pr.clone())
                }))
            }
        };
        if let Some(gate) = gate {
            // The semaphore is never closed.
            if let Ok(permit) = gate.acquire().await {
                permit.forget();
            }
        }
        answer
    }

    async fn update_pull_request(
        &self,
        p: &ProjectId,
        number: u64,
        ch: PullRequestUpdate,
    ) -> Result<PullRequest, IntegrationError> {
        let hang = {
            let mut state = self.state.lock().expect("lock poisoned");
            state.calls.push(ForgeCall::UpdatePullRequest {
                project_id: *p,
                number,
                changes: ch.clone(),
            });
            state.hang_update_pr
        };
        if hang {
            return std::future::pending().await;
        }
        let mut state = self.state.lock().expect("lock poisoned");
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
        let gate = {
            let mut state = self.state.lock().expect("lock poisoned");
            state.calls.push(ForgeCall::AddIssueComment {
                project_id: *p,
                number,
                body: body.to_string(),
            });
            state.comment_gate.clone()
        };
        if let Some(gate) = gate {
            // The semaphore is never closed.
            if let Ok(permit) = gate.acquire().await {
                permit.forget();
            }
        }
        let mut state = self.state.lock().expect("lock poisoned");
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

    /// Records `since` but returns the seeded feedback unfiltered; the plugins apply `since` and
    /// test it.
    async fn list_pull_request_feedback(
        &self,
        p: &ProjectId,
        number: u64,
        since: Option<&str>,
    ) -> Result<Feedback, IntegrationError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(ForgeCall::ListPullRequestFeedback {
            project_id: *p,
            number,
            since: since.map(str::to_string),
        });
        if let Some(scripted) = state.feedback_results.pop_front() {
            return scripted;
        }
        Ok(state
            .feedback
            .get(&(*p, number))
            .cloned()
            .unwrap_or_default())
    }

    async fn reply_to_review_thread(
        &self,
        p: &ProjectId,
        number: u64,
        thread: &str,
        body: &str,
    ) -> Result<Comment, IntegrationError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(ForgeCall::ReplyToReviewThread {
            project_id: *p,
            number,
            thread: thread.to_string(),
            body: body.to_string(),
        });
        if let Some(scripted) = state.reply_results.pop_front() {
            return scripted;
        }
        let known = state
            .feedback
            .get(&(*p, number))
            .is_some_and(|f| f.threads.iter().any(|t| t.id == thread));
        if !known {
            return Err(IntegrationError::InvalidInput(format!(
                "review thread `{thread}` is not on PR #{number}"
            )));
        }
        state.next_comment_id += 1;
        let id = state.next_comment_id.to_string();
        Ok(Comment {
            url: format!("https://forge.example.test/pulls/{number}#comment-{id}"),
            id,
        })
    }

    async fn whoami(&self, p: &ProjectId) -> Result<String, IntegrationError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(ForgeCall::Whoami { project_id: *p });
        if let Some(scripted) = state.whoami_results.pop_front() {
            return scripted;
        }
        Ok(state
            .bot_login
            .clone()
            .unwrap_or_else(|| Self::DEFAULT_BOT_LOGIN.to_string()))
    }

    async fn author_role(
        &self,
        p: &ProjectId,
        login: &str,
    ) -> Result<AuthorRole, IntegrationError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(ForgeCall::AuthorRole {
            project_id: *p,
            login: login.to_string(),
        });
        Ok(state
            .roles
            .get(login)
            .copied()
            .unwrap_or(AuthorRole::Outside))
    }
}

// ---------------------------------------------------------------------------
// FakeSecretStore
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SecretStoreState {
    calls: Vec<SecretKeyRef>,
    entries: HashMap<(String, String), Result<SecretString, SecretError>>,
    created: Vec<(SecretKeyRef, BTreeMap<String, String>)>,
}

/// In-memory fake [`SecretStore`] that records [`SecretKeyRef`] lookups and returns seeded secrets.
#[derive(Clone, Default)]
pub struct FakeSecretStore {
    state: Arc<Mutex<SecretStoreState>>,
}

impl FakeSecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all [`SecretKeyRef`] lookups recorded so far.
    pub fn calls(&self) -> Vec<SecretKeyRef> {
        self.state.lock().expect("lock poisoned").calls.clone()
    }

    /// Returns the Secrets created by `get_or_create` with the labels they were created with.
    pub fn created(&self) -> Vec<(SecretKeyRef, BTreeMap<String, String>)> {
        self.state.lock().expect("lock poisoned").created.clone()
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

    /// Deletes the seeded or created entry for `(secret_name, secret_key)`.
    pub fn remove(&self, secret_name: &str, secret_key: &str) {
        self.state
            .lock()
            .expect("lock poisoned")
            .entries
            .remove(&(secret_name.to_string(), secret_key.to_string()));
    }
}

#[async_trait]
impl SecretStore for FakeSecretStore {
    async fn get(&self, r: &SecretKeyRef) -> Result<SecretString, SecretError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(r.clone());
        match state.entries.get(&(r.name.clone(), r.key.clone())) {
            Some(Ok(secret)) => Ok(secret.clone()),
            Some(Err(err)) => Err(err.clone()),
            None => Err(SecretError::NotFound(r.clone())),
        }
    }

    async fn get_or_create(
        &self,
        r: &SecretKeyRef,
        labels: &BTreeMap<String, String>,
    ) -> Result<SecretString, SecretError> {
        let mut state = self.state.lock().expect("lock poisoned");
        state.calls.push(r.clone());
        let key = (r.name.clone(), r.key.clone());
        if let Some(entry) = state.entries.get(&key) {
            return entry.clone();
        }
        // The Secret exists without the key: never patch it (as `K8sSecretStore`).
        if state.entries.keys().any(|(name, _key)| *name == r.name) {
            return Err(SecretError::NotFound(r.clone()));
        }
        let value = SecretString::from(format!("generated-{}", state.created.len()));
        state.entries.insert(key, Ok(value.clone()));
        state.created.push((r.clone(), labels.clone()));
        Ok(value)
    }
}

#[cfg(test)]
#[expect(
    clippy::too_many_lines,
    reason = "scenario tests set up and check a whole run"
)]
mod tests {
    use super::*;
    use crate::ids::PersonaId;
    use crate::keys::TaskKey;
    use crate::seams::{IssueRef, IssueState, RepoRef, ReviewThread};
    use secrecy::ExposeSecret;
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn fake_sandbox_provider_records_volume_deletions_and_scripts_their_results() {
        let provider = FakeSandboxProvider::new();
        let volume = TaskVolumeSpec::for_task("DF-1".parse().unwrap());
        provider.push_delete_volume_result(Err(ProviderError::Backend("down".into())));

        let first = provider.delete_task_volume(&volume).await;
        let second = provider.delete_task_volume(&volume).await;

        assert_eq!(first, Err(ProviderError::Backend("down".into())));
        assert_eq!(second, Ok(()));
        assert_eq!(
            provider.calls(),
            vec![
                SandboxCall::DeleteTaskVolume(volume.clone()),
                SandboxCall::DeleteTaskVolume(volume),
            ]
        );
    }

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
            harness: None,
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
    async fn fake_forge_client_lists_seeded_feedback_and_replies_only_in_its_threads() {
        let forge = FakeForgeClient::new();
        let project_id = ProjectId::new();
        let feedback = Feedback {
            threads: vec![ReviewThread {
                id: "t1".into(),
                path: "src/lib.rs".into(),
                line: Some(3),
                resolved: false,
                comments: Vec::new(),
            }],
            ..Feedback::default()
        };
        forge.insert_feedback(project_id, 4, feedback.clone());

        let listed = forge
            .list_pull_request_feedback(&project_id, 4, Some("2026-11-01T00:00:00Z"))
            .await;
        let empty = forge.list_pull_request_feedback(&project_id, 5, None).await;
        let reply = forge
            .reply_to_review_thread(&project_id, 4, "t1", "done")
            .await
            .unwrap();
        let foreign = forge
            .reply_to_review_thread(&project_id, 5, "t1", "done")
            .await;
        forge.push_reply_to_review_thread_result(Err(IntegrationError::NotFound("gone".into())));
        let scripted = forge
            .reply_to_review_thread(&project_id, 4, "t1", "again")
            .await;

        assert_eq!(listed, Ok(feedback));
        assert_eq!(empty, Ok(Feedback::default()));
        assert!(!reply.id.is_empty());
        assert!(matches!(foreign, Err(IntegrationError::InvalidInput(_))));
        assert_eq!(scripted, Err(IntegrationError::NotFound("gone".into())));
        assert_eq!(
            forge.calls().first(),
            Some(&ForgeCall::ListPullRequestFeedback {
                project_id,
                number: 4,
                since: Some("2026-11-01T00:00:00Z".into()),
            })
        );
        assert!(forge.calls().contains(&ForgeCall::ReplyToReviewThread {
            project_id,
            number: 4,
            thread: "t1".into(),
            body: "done".into(),
        }));
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

        let cred_ref = SecretKeyRef {
            name: "claude-sub".into(),
            key: "oauth-token".into(),
        };
        let secret = store.get(&cred_ref).await.unwrap();
        assert_eq!(secret.expose_secret(), "sk-ant-oat01-secret");

        let missing_ref = SecretKeyRef {
            name: "nope".into(),
            key: "key".into(),
        };
        assert_eq!(
            store.get(&missing_ref).await.unwrap_err(),
            SecretError::NotFound(missing_ref.clone())
        );

        assert_eq!(store.calls(), vec![cred_ref, missing_ref]);
    }

    #[tokio::test]
    async fn fake_secret_store_get_or_create_creates_missing_and_keeps_existing() {
        let store = FakeSecretStore::new();
        store.insert("present", "k", "kept");
        let labels = BTreeMap::from([("a".to_string(), "b".to_string())]);
        let present = SecretKeyRef {
            name: "present".into(),
            key: "k".into(),
        };
        let missing = SecretKeyRef {
            name: "missing".into(),
            key: "k".into(),
        };

        let kept = store.get_or_create(&present, &labels).await.unwrap();
        assert_eq!(kept.expose_secret(), "kept");
        assert!(store.created().is_empty());

        let other_key = SecretKeyRef {
            name: "present".into(),
            key: "other".into(),
        };
        let err = store.get_or_create(&other_key, &labels).await.unwrap_err();
        assert_eq!(err, SecretError::NotFound(other_key));
        assert!(store.created().is_empty());

        let made = store.get_or_create(&missing, &labels).await.unwrap();
        assert_eq!(store.created(), vec![(missing.clone(), labels.clone())]);
        assert_eq!(
            store.get(&missing).await.unwrap().expose_secret(),
            made.expose_secret()
        );
    }
}
