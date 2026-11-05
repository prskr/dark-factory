//! Applies parsed forge webhook events (`iter3-webhooks` design D3, D4, D8): matches the
//! projects by repository, routes the event to their tasks, applies the bot and wake filters,
//! then queues a message for the event or completes the task of a merged PR.

use std::sync::Arc;

use df_core::webhooks::{ParsedEvent, WebhookEvent, WebhookWake, repository_matches};
use df_core::{AuthorRole, ForgeClient, TaskId};
use df_db::webhooks::{
    WebhookProject, complete_on_merge_tx, queue_event_tx, target_tasks, webhook_projects,
};
use sqlx::PgPool;

use crate::error::ChannelError;

/// Applies the events of a webhook delivery.
pub struct WebhookApplier {
    pool: PgPool,
    forge: Arc<dyn ForgeClient>,
}

impl WebhookApplier {
    pub fn new(pool: PgPool, forge: Arc<dyn ForgeClient>) -> Self {
        Self { pool, forge }
    }

    /// Applies `events` to every project on the event's repository. An event that names no
    /// task or does not pass the filters is dropped with a debug event. Fails when the database
    /// or a needed forge lookup fails.
    pub async fn apply(&self, events: &[ParsedEvent]) -> Result<(), ChannelError> {
        if events.is_empty() {
            return Ok(());
        }
        let projects = webhook_projects(&self.pool).await?;
        for parsed in events {
            let matching: Vec<_> = projects
                .iter()
                .filter(|project| repository_matches(&project.repo_url, &parsed.repo_url))
                .collect();
            if matching.is_empty() {
                tracing::debug!(repo_url = %parsed.repo_url, "webhook repository matches no project");
            }
            for project in matching {
                self.apply_to_project(project, &parsed.event).await?;
            }
        }
        Ok(())
    }

    async fn apply_to_project(
        &self,
        project: &WebhookProject,
        event: &WebhookEvent,
    ) -> Result<(), ChannelError> {
        let Some(target) = event.target() else {
            tracing::debug!(project_id = %project.id, ?event, "webhook event names no factory task");
            return Ok(());
        };
        let tasks = target_tasks(&self.pool, project.id, &target).await?;
        if tasks.is_empty() {
            tracing::debug!(project_id = %project.id, ?event, "webhook event matches no task");
            return Ok(());
        }
        if !self.wakes(project, event).await? {
            return Ok(());
        }
        for task_id in tasks {
            self.apply_to_task(task_id, event).await?;
        }
        Ok(())
    }

    async fn apply_to_task(
        &self,
        task_id: TaskId,
        event: &WebhookEvent,
    ) -> Result<(), ChannelError> {
        let mut tx = self.pool.begin().await?;
        if let WebhookEvent::PullRequestClosed {
            merged: true,
            actor,
            ..
        } = event
        {
            if !complete_on_merge_tx(&mut tx, task_id, actor).await? {
                tracing::debug!(task_id = %task_id, "merge already applied");
            }
        } else {
            queue_event_tx(&mut tx, task_id, event).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The bot and wake filters (spec "Bot authored events dropped", "Wake policy per
    /// project"). Only reviews and comments are filtered; they ask the forge for the bot login
    /// and, under the `members` policy, for the author's role.
    async fn wakes(
        &self,
        project: &WebhookProject,
        event: &WebhookEvent,
    ) -> Result<bool, ChannelError> {
        let Some(author) = event.feedback_author() else {
            return Ok(true);
        };
        let bot = self.forge.whoami(&project.id).await?;
        if author.eq_ignore_ascii_case(&bot) {
            tracing::debug!(project_id = %project.id, reason = "bot", "webhook event dropped");
            return Ok(false);
        }
        if !event.addresses(&bot) {
            tracing::debug!(project_id = %project.id, reason = "no mention", "webhook event dropped");
            return Ok(false);
        }
        let wakes = match project.wake {
            WebhookWake::None => false,
            WebhookWake::Anyone => true,
            WebhookWake::Members => {
                self.forge.author_role(&project.id, author).await? == AuthorRole::Member
            }
        };
        if !wakes {
            tracing::debug!(project_id = %project.id, wake = %project.wake, "webhook author below the wake policy");
        }
        Ok(wakes)
    }
}

#[cfg(test)]
mod tests;
