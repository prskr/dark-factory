//! The forge operations of one plugin: one fresh plugin instance per call, configured from a
//! resolved repository target and its forge credential.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use df_core::webhooks::ParsedEvent;
use df_core::{
    AuthScheme, AuthorRole, Comment, CredentialRef, Feedback, IntegrationError, Issue,
    NewPullRequest, PullRequest, PullRequestUpdate, RepoRef, SecretStore,
};
use futures::future::BoxFuture;
use secrecy::SecretString;
use wasmtime::Store;

use crate::convert;
use crate::host::{Forge, InstanceSpec, PluginHost, pull_requests::OpenFilter, wit};
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
        }
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
            .run(spec, |plugin, store| {
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
        self.call(target, |plugin, store| {
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
        self.call(target, |plugin, store| {
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
        self.call(target, |plugin, store| {
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
        self.call(target, |plugin, store| {
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
        let repo = convert::repo(&target.repo);
        self.call(target, |plugin, store| {
            let filter = OpenFilter {
                head: head.to_string(),
                head_prefix: None,
                author: None,
                label: None,
                base: None,
            };
            let repo = repo.clone();
            Box::pin(async move {
                let prs = plugin.darkfactory_integrations_pull_requests();
                prs.call_list_open(store, &repo, &filter).await
            })
        })
        .await
        .map(|prs| prs.into_iter().map(convert::from_pull_request).collect())
    }

    pub(crate) async fn update_pull_request(
        &self,
        target: &RepoTarget,
        number: u64,
        ch: PullRequestUpdate,
    ) -> Result<PullRequest, IntegrationError> {
        let id = convert::pull_request_ref(&target.repo, number);
        let changes = convert::pull_request_update(ch);
        self.call(target, |plugin, store| {
            let (id, changes) = (id.clone(), changes.clone());
            Box::pin(async move {
                let prs = plugin.darkfactory_integrations_pull_requests();
                prs.call_update(store, &id, &changes).await
            })
        })
        .await
        .map(convert::from_pull_request)
    }

    pub(crate) async fn list_pull_request_feedback(
        &self,
        target: &RepoTarget,
        number: u64,
        since: Option<&str>,
    ) -> Result<Feedback, IntegrationError> {
        let id = convert::pull_request_ref(&target.repo, number);
        self.call(target, |plugin, store| {
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
        self.call(target, |plugin, store| {
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
        self.call(target, |plugin, store| {
            let id = id.clone();
            Box::pin(async move {
                let issues = plugin.darkfactory_integrations_issues();
                issues.call_get(store, &id).await
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
        self.call(target, |plugin, store| {
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
    async fn call<R, F>(&self, target: &RepoTarget, op: F) -> Result<R, IntegrationError>
    where
        F: for<'a> Fn(
                &'a Forge,
                &'a mut Store<State>,
            ) -> BoxFuture<'a, wasmtime::Result<Result<R, wit::Error>>>
            + Sync,
    {
        let token = self
            .secrets
            .get(&target.credential.secret)
            .await
            .map_err(|e| IntegrationError::Config(format!("reading forge credential: {e}")))?;
        with_retries(|| async {
            let mut policy =
                HttpPolicy::for_base_url(&target.base_url, token.clone(), target.auth_scheme)
                    .map_err(IntegrationError::Config)?;
            if self.allow_plain_http {
                policy = policy.allow_plain_http();
            }
            let settings = self
                .settings
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .chain([("base-url".to_string(), target.base_url.clone())])
                .collect::<HashMap<_, _>>();
            self.host
                .run(
                    InstanceSpec {
                        policy: Some(policy),
                        settings,
                        webhook: None,
                    },
                    |plugin, store| op(plugin, store),
                )
                .await
                .map_err(convert::from_error)
        })
        .await
    }
}
