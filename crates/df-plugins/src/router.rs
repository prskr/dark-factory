//! [`ForgeClient`] over the loaded forge plugins: resolves the project once per call and routes it
//! by the host of its repository URL (design D4).

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use df_core::routing::RoutingTable;
use df_core::webhooks::ParsedEvent;
use df_core::{
    AuthorRole, Comment, CredentialRef, Feedback, ForgeClient, IntegrationError, Issue,
    NewPullRequest, ProjectId, PullRequest, PullRequestUpdate,
};

use crate::forge::{PluginForgeClient, RepoTarget, WebhookError};

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

/// Picks the plugin client for a project by the host of its repository URL.
pub struct ForgeRouter {
    projects: Arc<dyn ProjectDirectory>,
    routes: RoutingTable,
    clients: BTreeMap<String, PluginForgeClient>,
}

impl ForgeRouter {
    /// `clients` are the loaded plugins by their name under `plugins`. They may reach `http://`
    /// base URLs only if `routes` allows plain HTTP.
    pub fn new(
        projects: Arc<dyn ProjectDirectory>,
        routes: RoutingTable,
        clients: BTreeMap<String, PluginForgeClient>,
    ) -> Self {
        let clients = if routes.allows_plain_http() {
            clients
                .into_iter()
                .map(|(name, client)| (name, client.allow_plain_http()))
                .collect()
        } else {
            clients
        };
        Self {
            projects,
            routes,
            clients,
        }
    }

    /// Whether `plugin` (the name under `plugins`) loaded.
    pub fn has_plugin(&self, plugin: &str) -> bool {
        self.clients.contains_key(plugin)
    }

    /// The loaded plugin `plugin`, the name under `plugins`.
    pub fn client(&self, plugin: &str) -> Option<&PluginForgeClient> {
        self.clients.get(plugin)
    }

    /// Parses a webhook delivery with the plugin `plugin`, the name under `plugins`.
    pub async fn handle_webhook(
        &self,
        plugin: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<Vec<ParsedEvent>, WebhookError> {
        let client = self.clients.get(plugin).ok_or(WebhookError::NotFound)?;
        client.handle_webhook(headers, body).await
    }

    async fn resolve(
        &self,
        project: &ProjectId,
    ) -> Result<(&PluginForgeClient, RepoTarget), IntegrationError> {
        let target = self.projects.forge_target(project).await?;
        let resolved = self
            .routes
            .resolve(&target.repo_url)
            .map_err(|e| IntegrationError::Config(e.to_string()))?;
        // An unloaded claimer never falls through to the default: the credential is for its forge.
        let client = self.clients.get(&resolved.plugin).ok_or_else(|| {
            IntegrationError::Config(format!(
                "forge plugin `{}` serving host `{}` did not load",
                resolved.plugin, resolved.host
            ))
        })?;
        let credential = target.credential.ok_or_else(|| {
            IntegrationError::Config("the project has no forge credential".to_string())
        })?;
        Ok((
            client,
            RepoTarget {
                base_url: resolved.base_url,
                auth_scheme: resolved.auth_scheme,
                repo: resolved.repo,
                credential,
            },
        ))
    }
}

#[async_trait]
impl ForgeClient for ForgeRouter {
    async fn create_pull_request(
        &self,
        p: &ProjectId,
        req: NewPullRequest,
    ) -> Result<PullRequest, IntegrationError> {
        let (client, target) = self.resolve(p).await?;
        client.create_pull_request(&target, req).await
    }

    async fn find_open_pull_request(
        &self,
        p: &ProjectId,
        head: &str,
        base: &str,
    ) -> Result<Option<PullRequest>, IntegrationError> {
        let (client, target) = self.resolve(p).await?;
        client.find_open_pull_request(&target, head, base).await
    }

    async fn list_open_pull_requests(
        &self,
        p: &ProjectId,
        head: &str,
    ) -> Result<Vec<PullRequest>, IntegrationError> {
        let (client, target) = self.resolve(p).await?;
        client.list_open_pull_requests(&target, head).await
    }

    async fn update_pull_request(
        &self,
        p: &ProjectId,
        number: u64,
        ch: PullRequestUpdate,
    ) -> Result<PullRequest, IntegrationError> {
        let (client, target) = self.resolve(p).await?;
        client.update_pull_request(&target, number, ch).await
    }

    async fn list_pull_request_feedback(
        &self,
        p: &ProjectId,
        number: u64,
        since: Option<&str>,
    ) -> Result<Feedback, IntegrationError> {
        let (client, target) = self.resolve(p).await?;
        client
            .list_pull_request_feedback(&target, number, since)
            .await
    }

    async fn reply_to_review_thread(
        &self,
        p: &ProjectId,
        number: u64,
        thread: &str,
        body: &str,
    ) -> Result<Comment, IntegrationError> {
        let (client, target) = self.resolve(p).await?;
        client
            .reply_to_review_thread(&target, number, thread, body)
            .await
    }

    async fn get_issue(&self, p: &ProjectId, number: u64) -> Result<Issue, IntegrationError> {
        let (client, target) = self.resolve(p).await?;
        client.get_issue(&target, number).await
    }

    async fn add_issue_comment(
        &self,
        p: &ProjectId,
        number: u64,
        body: &str,
    ) -> Result<Comment, IntegrationError> {
        let (client, target) = self.resolve(p).await?;
        client.add_issue_comment(&target, number, body).await
    }

    async fn whoami(&self, p: &ProjectId) -> Result<String, IntegrationError> {
        let (client, target) = self.resolve(p).await?;
        client.whoami(&target).await
    }

    async fn author_role(
        &self,
        p: &ProjectId,
        login: &str,
    ) -> Result<AuthorRole, IntegrationError> {
        let (client, target) = self.resolve(p).await?;
        client.author_role(&target, login).await
    }
}
