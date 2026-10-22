//! [`ForgeClient`] over the Forgejo plugin: one fresh plugin instance per call, configured from
//! the project's repository URL and forge credential.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use df_core::{
    Comment, CredentialRef, ForgeClient, IntegrationError, Issue, NewPullRequest, ProjectId,
    PullRequest, PullRequestUpdate, RepoRef, SecretStore,
};

use crate::convert;
use crate::host::{InstanceSpec, PluginHost};
use crate::policy::HttpPolicy;
use crate::retry::with_retries;

/// Where a project's repository lives and who acts on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeTarget {
    pub repo_url: String,
    pub credential: Option<CredentialRef>,
}

/// Looks up the forge details of a project.
#[async_trait]
pub trait ProjectDirectory: Send + Sync {
    async fn forge_target(&self, project: &ProjectId) -> Result<ForgeTarget, IntegrationError>;
}

pub struct PluginForgeClient {
    host: PluginHost,
    projects: Arc<dyn ProjectDirectory>,
    secrets: Arc<dyn SecretStore>,
    settings: BTreeMap<String, String>,
    allow_plain_http: bool,
}

impl PluginForgeClient {
    pub fn new(
        host: PluginHost,
        projects: Arc<dyn ProjectDirectory>,
        secrets: Arc<dyn SecretStore>,
        settings: BTreeMap<String, String>,
    ) -> Self {
        Self {
            host,
            projects,
            secrets,
            settings,
            allow_plain_http: false,
        }
    }

    /// Lets plugins reach `http://` base URLs; for tests against a local mock server.
    pub fn allow_plain_http(mut self) -> Self {
        self.allow_plain_http = true;
        self
    }

    async fn resolve(
        &self,
        project: &ProjectId,
    ) -> Result<(RepoRef, impl Fn() -> Result<InstanceSpec, IntegrationError>), IntegrationError>
    {
        let target = self.projects.forge_target(project).await?;
        let (base_url, repo) = parse_repo_url(&target.repo_url)?;
        let credential = target.credential.ok_or_else(|| {
            IntegrationError::Config("the project has no forge credential".to_string())
        })?;
        let token = self
            .secrets
            .get(&credential)
            .await
            .map_err(|e| IntegrationError::Config(format!("reading forge credential: {e}")))?;
        let plugin_settings = self.settings.clone();
        let allow_plain_http = self.allow_plain_http;
        let spec = move || {
            let mut policy = HttpPolicy::for_base_url(&base_url, token.clone())
                .map_err(IntegrationError::Config)?;
            if allow_plain_http {
                policy = policy.allow_plain_http();
            }
            Ok(InstanceSpec {
                policy,
                settings: plugin_settings
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .chain([("base-url".to_string(), base_url.clone())])
                    .collect::<HashMap<_, _>>(),
            })
        };
        Ok((repo, spec))
    }
}

#[async_trait]
impl ForgeClient for PluginForgeClient {
    async fn create_pull_request(
        &self,
        p: &ProjectId,
        mut req: NewPullRequest,
    ) -> Result<PullRequest, IntegrationError> {
        let (repo, spec) = self.resolve(p).await?;
        req.repo = repo;
        with_retries(|| async {
            let request = convert::new_pull_request(req.clone());
            self.host
                .run(spec()?, move |plugin, store| {
                    Box::pin(async move {
                        let prs = plugin.darkfactory_integrations_pull_requests();
                        prs.call_create(store, &request).await
                    })
                })
                .await
                .map(convert::from_pull_request)
                .map_err(convert::from_error)
        })
        .await
    }

    async fn find_open_pull_request(
        &self,
        p: &ProjectId,
        head: &str,
        base: &str,
    ) -> Result<Option<PullRequest>, IntegrationError> {
        let (repo, spec) = self.resolve(p).await?;
        let repo = convert::repo(&repo);
        with_retries(|| async {
            let (repo, head, base) = (repo.clone(), head.to_string(), base.to_string());
            self.host
                .run(spec()?, move |plugin, store| {
                    Box::pin(async move {
                        let prs = plugin.darkfactory_integrations_pull_requests();
                        prs.call_find_open(store, &repo, &head, &base).await
                    })
                })
                .await
                .map(|pr| pr.map(convert::from_pull_request))
                .map_err(convert::from_error)
        })
        .await
    }

    async fn update_pull_request(
        &self,
        p: &ProjectId,
        number: u64,
        ch: PullRequestUpdate,
    ) -> Result<PullRequest, IntegrationError> {
        let (repo, spec) = self.resolve(p).await?;
        let id = convert::pull_request_ref(&repo, number);
        with_retries(|| async {
            let (id, changes) = (id.clone(), convert::pull_request_update(ch.clone()));
            self.host
                .run(spec()?, move |plugin, store| {
                    Box::pin(async move {
                        let prs = plugin.darkfactory_integrations_pull_requests();
                        prs.call_update(store, &id, &changes).await
                    })
                })
                .await
                .map(convert::from_pull_request)
                .map_err(convert::from_error)
        })
        .await
    }

    async fn get_issue(&self, p: &ProjectId, number: u64) -> Result<Issue, IntegrationError> {
        let (repo, spec) = self.resolve(p).await?;
        let id = convert::issue_ref(&repo, number);
        with_retries(|| async {
            let id = id.clone();
            self.host
                .run(spec()?, move |plugin, store| {
                    Box::pin(async move {
                        plugin
                            .darkfactory_integrations_issues()
                            .call_get(store, &id)
                            .await
                    })
                })
                .await
                .map(convert::from_issue)
                .map_err(convert::from_error)
        })
        .await
    }

    async fn add_issue_comment(
        &self,
        p: &ProjectId,
        number: u64,
        body: &str,
    ) -> Result<Comment, IntegrationError> {
        let (repo, spec) = self.resolve(p).await?;
        let id = convert::issue_ref(&repo, number);
        with_retries(|| async {
            let (id, body) = (id.clone(), body.to_string());
            self.host
                .run(spec()?, move |plugin, store| {
                    Box::pin(async move {
                        let issues = plugin.darkfactory_integrations_issues();
                        issues.call_add_comment(store, &id, &body).await
                    })
                })
                .await
                .map(convert::from_comment)
                .map_err(convert::from_error)
        })
        .await
    }
}

/// `https://git.example.org/owner/name.git` → (`https://git.example.org`, owner/name).
fn parse_repo_url(repo_url: &str) -> Result<(String, RepoRef), IntegrationError> {
    let invalid = || IntegrationError::Config(format!("unsupported repository URL `{repo_url}`"));
    let (scheme, rest) = repo_url.split_once("://").ok_or_else(invalid)?;
    let (authority, path) = rest.split_once('/').ok_or_else(invalid)?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/').ok_or_else(invalid)?;
    if authority.is_empty() || owner.is_empty() || name.is_empty() || name.contains('/') {
        return Err(invalid());
    }
    Ok((
        format!("{scheme}://{authority}"),
        RepoRef {
            owner: owner.to_string(),
            name: name.to_string(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repository_urls() {
        let (base, repo) =
            parse_repo_url("https://git.example.org/peter/dark-factory.git").unwrap();
        assert_eq!(base, "https://git.example.org");
        assert_eq!(
            (repo.owner.as_str(), repo.name.as_str()),
            ("peter", "dark-factory")
        );
        assert!(parse_repo_url("https://git.example.org:3000/o/r/").is_ok());
        assert!(parse_repo_url("git@git.example.org:o/r.git").is_err());
        assert!(parse_repo_url("https://git.example.org/only-owner").is_err());
        assert!(parse_repo_url("https://git.example.org/a/b/c").is_err());
    }
}
