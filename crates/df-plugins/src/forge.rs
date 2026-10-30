//! The forge operations of one plugin: one fresh plugin instance per call, configured from a
//! resolved repository target and its forge credential.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use df_core::{
    AuthScheme, Comment, CredentialRef, IntegrationError, Issue, NewPullRequest, PullRequest,
    PullRequestUpdate, RepoRef, SecretStore,
};
use futures::future::BoxFuture;
use wasmtime::Store;

use crate::convert;
use crate::host::{Forge, InstanceSpec, PluginHost, wit};
use crate::policy::HttpPolicy;
use crate::retry::with_retries;
use crate::state::State;

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
    allow_plain_http: bool,
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
            allow_plain_http: false,
        }
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
            .get(&target.credential)
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
                .run(InstanceSpec { policy, settings }, |plugin, store| {
                    op(plugin, store)
                })
                .await
                .map_err(convert::from_error)
        })
        .await
    }
}
