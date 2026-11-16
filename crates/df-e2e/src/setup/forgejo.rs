//! `setup forgejo`: users, tokens and repositories of the suite, through the Forgejo API. Runs on
//! a fresh Forgejo and on a kept one alike (existing users are kept, their `e2e` token and the
//! repositories are replaced).

use anyhow::{Context, Result};
use serde::Serialize;

use crate::data;
use crate::forgejo::Forgejo;

/// The events of `docs/content/integrations/forges/webhooks.md`; `pull_request_only` is the
/// "Modification" checkbox of the webhook form.
const WEBHOOK_EVENTS: [&str; 5] = [
    "issue_comment",
    "pull_request_only",
    "pull_request_comment",
    "pull_request_review",
    "action_run_failure",
];

/// What the setup hands to the runner: the keys are the JSON the script reads.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Tokens {
    #[serde(rename = "forgejo-token")]
    pub forgejo_token: String,
    #[serde(rename = "reviewer-token")]
    pub reviewer_token: String,
}

/// Prepares the Forgejo at `base_url`, with the admin the chart created (`admin_user`,
/// `admin_password`); `webhook_url` is where the repositories' webhooks post to.
pub async fn run(
    base_url: &str,
    admin_user: &str,
    admin_password: &str,
    webhook_url: &str,
) -> Result<Tokens> {
    let admin = Forgejo::with_password(base_url, admin_user, admin_password)?;
    let bot = Login {
        name: data::FORGE_OWNER,
        password: data::FORGE_OWNER_PASSWORD,
    };
    let reviewer = Login {
        name: data::FORGE_REVIEWER,
        password: data::FORGE_REVIEWER_PASSWORD,
    };
    let forgejo_token = prepare_user(&admin, base_url, &bot).await?;
    let reviewer_token = prepare_user(&admin, base_url, &reviewer).await?;

    let owner = Forgejo::with_password(base_url, bot.name, bot.password)?;
    for repository in data::FORGE_REPOSITORIES {
        prepare_repository(&owner, repository, webhook_url).await?;
    }
    Ok(Tokens {
        forgejo_token,
        reviewer_token,
    })
}

/// A user of the suite and the password `setup forgejo` creates it with.
struct Login<'a> {
    name: &'a str,
    password: &'a str,
}

/// Creates the user when missing and replaces its token; returns the new token.
async fn prepare_user(admin: &Forgejo, base_url: &str, login: &Login<'_>) -> Result<String> {
    let Login { name, password } = *login;
    if !admin.user_exists(name).await? {
        admin
            .create_user(name, password, &format!("{name}@example.com"))
            .await?;
    }
    let user = Forgejo::with_password(base_url, name, password)?;
    user.delete_token(name, data::FORGE_TOKEN_NAME).await?;
    user.create_token(name, data::FORGE_TOKEN_NAME)
        .await
        .with_context(|| format!("token of {name}"))
}

/// Deletes and creates repository `name` with the reviewer as collaborator and the webhook; `demo`
/// also gets the issue the demo tasks link to and the file the hidden-path project hides.
async fn prepare_repository(owner: &Forgejo, name: &str, webhook_url: &str) -> Result<()> {
    owner.delete_repository(name).await?;
    owner.create_repository(name).await?;
    let repository = owner.clone().for_repository(name);
    repository.add_collaborator(data::FORGE_REVIEWER).await?;
    repository
        .create_webhook(webhook_url, data::WEBHOOK_SECRET, &WEBHOOK_EVENTS)
        .await?;
    if name == data::FORGE_REPO {
        repository
            .create_issue(data::FORGE_ISSUE_TITLE, data::FORGE_ISSUE_BODY)
            .await?;
        repository
            .create_file(
                data::BASE_BRANCH,
                data::HIDDEN_PATH,
                data::HIDDEN_CONTENT,
                "Add a secret file",
            )
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
