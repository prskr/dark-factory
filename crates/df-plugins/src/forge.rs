//! The forge operations of one plugin: one fresh plugin instance per call, configured from a
//! resolved repository target and its forge credential.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use df_core::webhooks::ParsedEvent;
use df_core::{
    AuthScheme, AuthorRole, CiJobLog, CiStatus, Comment, CredentialRef, Feedback, IntegrationError,
    Issue, IssueFilter, IssuePage, ListedPullRequest, NewIssue, NewPullRequest, PullRequest,
    PullRequestFilter, PullRequestUpdate, RepoRef, SecretStore,
};
use futures::future::BoxFuture;
use secrecy::SecretString;
use wasmtime::Store;

use crate::convert;
use crate::host::{CiProvider, Export, Forge, InstanceSpec, PluginHost, wit};
use crate::policy::HttpPolicy;
use crate::retry::with_retries;
use crate::state::{State, WebhookCall};

/// A forge call's resolved target: API base URL, how the credential is sent, repository and the
/// credential acting on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoTarget {
    pub(crate) base_url: String,
    pub(crate) auth_scheme: AuthScheme,
    pub(crate) repo: RepoRef,
    pub(crate) credential: CredentialRef,
}

/// One loaded forge plugin with its settings.
pub struct PluginForgeClient {
    host: PluginHost,
    secrets: Arc<dyn SecretStore>,
    settings: BTreeMap<String, String>,
    webhook_secret: Option<SecretString>,
    allow_plain_http: bool,
    extra_hosts: Vec<String>,
}

/// Why a webhook delivery was not turned into events.
#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    /// The plugin is unknown, did not load, exports no `webhooks` or has no webhook secret.
    #[error("no webhook endpoint for this plugin")]
    NotFound,
    /// The signature check failed or the plugin skipped it.
    #[error("webhook signature not verified")]
    Unverified,
    /// The plugin rejected the payload.
    #[error("invalid webhook payload: {0}")]
    InvalidInput(String),
    /// The plugin call failed.
    #[error("webhook handling failed: {0}")]
    Failed(IntegrationError),
}

impl PluginForgeClient {
    pub fn new(
        host: PluginHost,
        secrets: Arc<dyn SecretStore>,
        settings: BTreeMap<String, String>,
    ) -> Self {
        Self {
            host,
            secrets,
            settings,
            webhook_secret: None,
            allow_plain_http: false,
            extra_hosts: Vec::new(),
        }
    }

    /// Lets the plugin reach `hosts` (`host` or `*.suffix`) without the forge credential.
    #[must_use]
    pub fn with_extra_hosts(mut self, hosts: Vec<String>) -> Self {
        self.extra_hosts = hosts;
        self
    }

    /// Accepts webhooks signed with `secret`.
    #[must_use]
    pub fn with_webhook_secret(mut self, secret: SecretString) -> Self {
        self.webhook_secret = Some(secret);
        self
    }

    /// Whether the plugin exports `webhooks`.
    pub fn exports_webhooks(&self) -> bool {
        self.host.capabilities().webhooks
    }

    /// The secret webhook deliveries are verified with, only for a plugin that exports
    /// `webhooks`; the settings page of a project reveals it to admins.
    pub fn webhook_secret(&self) -> Option<&SecretString> {
        self.webhook_secret
            .as_ref()
            .filter(|_| self.exports_webhooks())
    }

    /// Parses one webhook delivery on an instance without project, credential or allowed hosts.
    /// Events count only if the plugin verified the signature through the host.
    pub async fn handle_webhook(
        &self,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<Vec<ParsedEvent>, WebhookError> {
        let secret = self.webhook_secret().ok_or(WebhookError::NotFound)?;
        let spec = InstanceSpec {
            policy: None,
            settings: self.settings.clone().into_iter().collect(),
            webhook: Some(WebhookCall::new(secret, body)),
        };
        let (headers, body) = (headers.to_vec(), body.to_vec());
        let (events, verified) = self
            .host
            .run(spec, |plugin: &Forge, store| {
                Box::pin(async move {
                    let parsed = plugin
                        .darkfactory_integrations_webhooks()
                        .call_handle_webhook(&mut *store, &headers, &body)
                        .await?;
                    let verified = store.data().webhook_verified();
                    Ok(parsed.map(|events| (events, verified)))
                })
            })
            .await
            .map_err(|e| match e {
                wit::Error::Unauthorized(_) => WebhookError::Unverified,
                wit::Error::InvalidInput(m) => WebhookError::InvalidInput(m),
                other => WebhookError::Failed(convert::from_error(other)),
            })?;
        if !verified {
            return Err(WebhookError::Unverified);
        }
        Ok(events.into_iter().map(convert::from_parsed_event).collect())
    }

    /// Login of the account owning the target's credential.
    pub(crate) async fn whoami(&self, target: &RepoTarget) -> Result<String, IntegrationError> {
        self.call(target, |plugin: &Forge, store| {
            Box::pin(async move {
                plugin
                    .darkfactory_integrations_webhooks()
                    .call_whoami(store)
                    .await
            })
        })
        .await
    }

    /// `login`'s relationship to the target repository.
    pub(crate) async fn author_role(
        &self,
        target: &RepoTarget,
        login: &str,
    ) -> Result<AuthorRole, IntegrationError> {
        let repo = convert::repo(&target.repo);
        self.call(target, |plugin: &Forge, store| {
            let (repo, login) = (repo.clone(), login.to_string());
            Box::pin(async move {
                let webhooks = plugin.darkfactory_integrations_webhooks();
                webhooks.call_author_role(store, &repo, &login).await
            })
        })
        .await
        .map(convert::from_role)
    }

    /// Lets plugins reach `http://` base URLs; for tests against a local mock server.
    #[must_use]
    pub(crate) fn allow_plain_http(mut self) -> Self {
        self.allow_plain_http = true;
        self
    }

    pub(crate) async fn create_pull_request(
        &self,
        target: &RepoTarget,
        mut req: NewPullRequest,
    ) -> Result<PullRequest, IntegrationError> {
        req.repo = target.repo.clone();
        let request = convert::new_pull_request(req);
        self.call(target, |plugin: &Forge, store| {
            let request = request.clone();
            Box::pin(async move {
                let prs = plugin.darkfactory_integrations_pull_requests();
                prs.call_create(store, &request).await
            })
        })
        .await
        .map(convert::from_pull_request)
    }

    pub(crate) async fn find_open_pull_request(
        &self,
        target: &RepoTarget,
        head: &str,
        base: &str,
    ) -> Result<Option<PullRequest>, IntegrationError> {
        let repo = convert::repo(&target.repo);
        self.call(target, |plugin: &Forge, store| {
            let (repo, head, base) = (repo.clone(), head.to_string(), base.to_string());
            Box::pin(async move {
                let prs = plugin.darkfactory_integrations_pull_requests();
                prs.call_find_open(store, &repo, &head, &base).await
            })
        })
        .await
        .map(|pr| pr.map(convert::from_pull_request))
    }

    pub(crate) async fn list_open_pull_requests(
        &self,
        target: &RepoTarget,
        head: &str,
    ) -> Result<Vec<PullRequest>, IntegrationError> {
        let filter = PullRequestFilter::default();
        let listed = self.list_pull_requests(target, Some(head), &filter).await?;
        Ok(listed.into_iter().map(|l| l.pr).collect())
    }

    /// Open pull requests with author and labels; `head` is the exact head branch in the
    /// repository itself, `filter` the other criteria.
    pub(crate) async fn list_pull_requests(
        &self,
        target: &RepoTarget,
        head: Option<&str>,
        filter: &PullRequestFilter,
    ) -> Result<Vec<ListedPullRequest>, IntegrationError> {
        let repo = convert::repo(&target.repo);
        let filter = convert::open_filter(head, filter);
        self.call(target, |plugin: &Forge, store| {
            let (repo, filter) = (repo.clone(), filter.clone());
            Box::pin(async move {
                let prs = plugin.darkfactory_integrations_pull_requests();
                prs.call_list_open(store, &repo, &filter).await
            })
        })
        .await
        .map(|prs| prs.into_iter().map(convert::from_listed).collect())
    }

    pub(crate) async fn get_pull_request(
        &self,
        target: &RepoTarget,
        number: u64,
    ) -> Result<PullRequest, IntegrationError> {
        let id = convert::pull_request_ref(&target.repo, number);
        self.call(target, |plugin: &Forge, store| {
            let id = id.clone();
            Box::pin(async move {
                let prs = plugin.darkfactory_integrations_pull_requests();
                prs.call_get(store, &id).await
            })
        })
        .await
        .map(convert::from_pull_request)
    }

    /// The checks of `git_ref`; `Unsupported` if the plugin has no `ci` export.
    pub(crate) async fn ci_status(
        &self,
        target: &RepoTarget,
        git_ref: &str,
    ) -> Result<CiStatus, IntegrationError> {
        let repo = convert::repo(&target.repo);
        self.call(target, |plugin: &CiProvider, store| {
            let (repo, git_ref) = (repo.clone(), git_ref.to_string());
            Box::pin(async move {
                plugin
                    .darkfactory_integrations_ci()
                    .call_status(store, &repo, &git_ref)
                    .await
            })
        })
        .await
        .map(convert::from_ci_status)
    }

    /// The end of the log of `job_id`; `Unsupported` if the plugin has no `ci` export.
    pub(crate) async fn ci_job_log(
        &self,
        target: &RepoTarget,
        job_id: u64,
    ) -> Result<CiJobLog, IntegrationError> {
        let repo = convert::repo(&target.repo);
        self.call(target, |plugin: &CiProvider, store| {
            let repo = repo.clone();
            Box::pin(async move {
                plugin
                    .darkfactory_integrations_ci()
                    .call_job_log(store, &repo, job_id)
                    .await
            })
        })
        .await
        .map(|log| CiJobLog {
            text: log.text,
            head_dropped: log.head_dropped,
        })
    }

    pub(crate) async fn update_pull_request(
        &self,
        target: &RepoTarget,
        number: u64,
        ch: PullRequestUpdate,
    ) -> Result<PullRequest, IntegrationError> {
        let id = convert::pull_request_ref(&target.repo, number);
        let changes = convert::pull_request_update(ch);
        self.call(target, |plugin: &Forge, store| {
            let (id, changes) = (id.clone(), changes.clone());
            Box::pin(async move {
                let prs = plugin.darkfactory_integrations_pull_requests();
                prs.call_update(store, &id, &changes).await
            })
        })
        .await
        .map(convert::from_pull_request)
    }

    pub(crate) async fn merge_pull_request(
        &self,
        target: &RepoTarget,
        number: u64,
        head_sha: &str,
    ) -> Result<(), IntegrationError> {
        let id = convert::pull_request_ref(&target.repo, number);
        self.call(target, |plugin: &Forge, store| {
            let (id, head_sha) = (id.clone(), head_sha.to_string());
            Box::pin(async move {
                let prs = plugin.darkfactory_integrations_pull_requests();
                prs.call_merge(store, &id, &head_sha).await
            })
        })
        .await
    }

    pub(crate) async fn create_branch(
        &self,
        target: &RepoTarget,
        name: &str,
        from: &str,
    ) -> Result<(), IntegrationError> {
        let repo = convert::repo(&target.repo);
        self.call(target, |plugin: &Forge, store| {
            let (repo, name, from) = (repo.clone(), name.to_string(), from.to_string());
            Box::pin(async move {
                let prs = plugin.darkfactory_integrations_pull_requests();
                prs.call_create_branch(store, &repo, &name, &from).await
            })
        })
        .await
    }

    pub(crate) async fn list_pull_request_feedback(
        &self,
        target: &RepoTarget,
        number: u64,
        since: Option<&str>,
    ) -> Result<Feedback, IntegrationError> {
        let id = convert::pull_request_ref(&target.repo, number);
        self.call(target, |plugin: &Forge, store| {
            let (id, since) = (id.clone(), since.map(str::to_string));
            Box::pin(async move {
                let prs = plugin.darkfactory_integrations_pull_requests();
                prs.call_list_feedback(store, &id, since.as_deref()).await
            })
        })
        .await
        .map(convert::from_feedback)
    }

    pub(crate) async fn reply_to_review_thread(
        &self,
        target: &RepoTarget,
        number: u64,
        thread: &str,
        body: &str,
    ) -> Result<Comment, IntegrationError> {
        let id = convert::pull_request_ref(&target.repo, number);
        self.call(target, |plugin: &Forge, store| {
            let (id, thread, body) = (id.clone(), thread.to_string(), body.to_string());
            Box::pin(async move {
                let prs = plugin.darkfactory_integrations_pull_requests();
                prs.call_reply(store, &id, &thread, &body).await
            })
        })
        .await
        .map(convert::from_posted_comment)
    }

    pub(crate) async fn get_issue(
        &self,
        target: &RepoTarget,
        number: u64,
    ) -> Result<Issue, IntegrationError> {
        let id = convert::issue_ref(&target.repo, number);
        self.call(target, |plugin: &Forge, store| {
            let id = id.clone();
            Box::pin(async move {
                let issues = plugin.darkfactory_integrations_issues();
                issues.call_get(store, &id).await
            })
        })
        .await
        .map(convert::from_issue)
    }

    pub(crate) async fn list_issues(
        &self,
        target: &RepoTarget,
        filter: &IssueFilter,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<IssuePage, IntegrationError> {
        let repo = convert::repo(&target.repo);
        let filter = convert::issue_filter(filter);
        self.call(target, |plugin: &Forge, store| {
            let (repo, filter) = (repo.clone(), filter.clone());
            let cursor = cursor.map(str::to_string);
            Box::pin(async move {
                let issues = plugin.darkfactory_integrations_issues();
                issues
                    .call_list(store, &repo, &filter, limit, cursor.as_deref())
                    .await
            })
        })
        .await
        .map(convert::from_issue_page)
    }

    /// Not idempotent; only `rate-limited` and `transport` errors are retried, like every call.
    pub(crate) async fn create_issue(
        &self,
        target: &RepoTarget,
        issue: &NewIssue,
    ) -> Result<Issue, IntegrationError> {
        let repo = convert::repo(&target.repo);
        let issue = convert::new_issue(issue);
        self.call(target, |plugin: &Forge, store| {
            let (repo, issue) = (repo.clone(), issue.clone());
            Box::pin(async move {
                let issues = plugin.darkfactory_integrations_issues();
                issues.call_create(store, &repo, &issue).await
            })
        })
        .await
        .map(convert::from_issue)
    }

    pub(crate) async fn add_issue_comment(
        &self,
        target: &RepoTarget,
        number: u64,
        body: &str,
    ) -> Result<Comment, IntegrationError> {
        let id = convert::issue_ref(&target.repo, number);
        self.call(target, |plugin: &Forge, store| {
            let (id, body) = (id.clone(), body.to_string());
            Box::pin(async move {
                let issues = plugin.darkfactory_integrations_issues();
                issues.call_add_comment(store, &id, &body).await
            })
        })
        .await
        .map(convert::from_comment)
    }

    /// Reads the credential, then runs `op` on a fresh instance allowed to reach the target's base
    /// URL, retrying rate limits and transport errors.
    /// `E` is the export `op` calls (`Forge`, or `CiProvider`, which a component may lack).
    async fn call<E, R, F>(&self, target: &RepoTarget, op: F) -> Result<R, IntegrationError>
    where
        E: Export,
        F: for<'a> Fn(
                &'a E,
                &'a mut Store<State>,
            ) -> BoxFuture<'a, wasmtime::Result<Result<R, wit::Error>>>
            + Sync,
    {
        let token = self.credential(target).await?;
        with_retries(|| async {
            let spec = self.instance_spec(target, &token)?;
            self.host
                .run(spec, |plugin, store| op(plugin, store))
                .await
                .map_err(convert::from_error)
        })
        .await
    }

    async fn credential(&self, target: &RepoTarget) -> Result<SecretString, IntegrationError> {
        self.secrets
            .get(&target.credential.secret)
            .await
            .map_err(|e| IntegrationError::Config(format!("reading forge credential: {e}")))
    }

    fn instance_spec(
        &self,
        target: &RepoTarget,
        token: &SecretString,
    ) -> Result<InstanceSpec, IntegrationError> {
        let mut policy =
            HttpPolicy::for_base_url(&target.base_url, token.clone(), target.auth_scheme)
                .map_err(IntegrationError::Config)?
                .with_extra_hosts(&self.extra_hosts);
        if self.allow_plain_http {
            policy = policy.allow_plain_http();
        }
        let settings = self
            .settings
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .chain([("base-url".to_string(), target.base_url.clone())])
            .collect::<HashMap<_, _>>();
        Ok(InstanceSpec {
            policy: Some(policy),
            settings,
            webhook: None,
        })
    }
}
