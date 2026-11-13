//! Forge webhook events as the plugins normalize them (`darkfactory:integrations/webhooks`) and
//! the forge-neutral rules the control plane applies to them: repository match, routing, the
//! bot and mention filters, and the message text.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::domain::ParseEnumError;
use crate::seams::ReviewVerdict;

/// The audit action of an admin reading the webhook secret on a project's settings page
/// (`resource_type` `webhook_secret`; the details name actor, project and plugin, never the secret).
pub const WEBHOOK_SECRET_READ_ACTION: &str = "webhook_secret.read";

/// One event a plugin parsed from a webhook delivery, with the repository it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedEvent {
    /// Web URL of the repository from the payload.
    pub repo_url: String,
    pub event: WebhookEvent,
}

/// A forge event the control plane acts on. Carries no review, comment or log text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WebhookEvent {
    ReviewSubmitted {
        number: u64,
        head: String,
        actor: String,
        verdict: ReviewVerdict,
    },
    PullRequestClosed {
        number: u64,
        head: String,
        actor: String,
        merged: bool,
    },
    CiFailed {
        head: String,
        check: String,
        url: String,
    },
    CommentCreated {
        number: u64,
        is_pull_request: bool,
        actor: String,
        mentions: Vec<String>,
    },
}

impl WebhookEvent {
    /// The tasks the event names.
    pub fn target(&self) -> Option<Target> {
        match self {
            Self::ReviewSubmitted { head, .. }
            | Self::PullRequestClosed { head, .. }
            | Self::CiFailed { head, .. } => Some(Target::Branch(
                head.strip_prefix("refs/heads/").unwrap_or(head).to_owned(),
            )),
            Self::CommentCreated {
                number,
                is_pull_request: true,
                ..
            } => Some(Target::PullRequest(*number)),
            Self::CommentCreated { number, .. } => Some(Target::Issue(*number)),
        }
    }

    /// The author of a review or comment, the events the bot and wake filters apply to. Closes,
    /// merges and CI failures apply whoever the actor is.
    pub fn feedback_author(&self) -> Option<&str> {
        match self {
            Self::ReviewSubmitted { actor, .. } | Self::CommentCreated { actor, .. } => Some(actor),
            Self::PullRequestClosed { .. } | Self::CiFailed { .. } => None,
        }
    }

    /// Whether the event is meant for the bot: a comment only when it mentions `bot_login`
    /// (case-insensitively, as on both forges), every other event always.
    pub fn addresses(&self, bot_login: &str) -> bool {
        match self {
            Self::CommentCreated { mentions, .. } => {
                mentions.iter().any(|m| m.eq_ignore_ascii_case(bot_login))
            }
            _ => true,
        }
    }
}

/// The tasks an event names (spec "Routing").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The task that holds the event's head branch (`tasks.branch`): the open one, else the most
    /// recently created one.
    Branch(String),
    /// The project's task with this PR number.
    PullRequest(u64),
    /// Every non-final task of the project linked to this issue.
    Issue(u64),
}

/// A project's `webhook_wake` setting: whose reviews and mentions wake its tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(into = "&'static str")]
pub enum WebhookWake {
    /// No review or mention wakes a task.
    None,
    /// Authors with write access or more.
    #[default]
    Members,
    /// Every author.
    Anyone,
}

impl WebhookWake {
    pub const ALL: &'static [Self] = &[Self::None, Self::Members, Self::Anyone];

    /// The stored form (`projects.webhook_wake`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Members => "members",
            Self::Anyone => "anyone",
        }
    }
}

impl fmt::Display for WebhookWake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for WebhookWake {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|wake| wake.as_str() == s)
            .ok_or_else(|| ParseEnumError {
                kind: "WebhookWake",
                value: s.to_string(),
            })
    }
}

impl TryFrom<String> for WebhookWake {
    type Error = ParseEnumError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<WebhookWake> for &'static str {
    fn from(wake: WebhookWake) -> Self {
        wake.as_str()
    }
}

/// Whether two repository URLs name the same repository: same host and port, same path
/// compared case-insensitively, without surrounding slashes and a trailing `.git`. A URL
/// without host or without an owner/name path matches nothing.
pub fn repository_matches(project_repo_url: &str, event_repo_url: &str) -> bool {
    match (
        repository_identity(project_repo_url),
        repository_identity(event_repo_url),
    ) {
        (Some(project), Some(event)) => project == event,
        _ => false,
    }
}

/// The message that wakes a task for one event (design D8), built only from logins, verdicts,
/// check names, numbers and URLs.
pub fn render_message(event: &WebhookEvent) -> String {
    let mut message = format!("Forge event on your task: {}\n", event_line(event));
    if event.feedback_author().is_some() {
        message.push_str(
            "Read reviews and comments with read_pull_request_feedback. \
             Pass the next_since of your last read as since.\n",
        );
    }
    message
}

fn event_line(event: &WebhookEvent) -> String {
    match event {
        WebhookEvent::ReviewSubmitted {
            number,
            actor,
            verdict,
            ..
        } => {
            let verdict = match verdict {
                ReviewVerdict::Approved => "approved",
                ReviewVerdict::ChangesRequested => "changes requested",
                ReviewVerdict::Commented => "commented",
            };
            format!("{actor} submitted a review on PR #{number}: {verdict}.")
        }
        WebhookEvent::PullRequestClosed {
            number,
            actor,
            merged: true,
            ..
        } => format!("{actor} merged PR #{number}."),
        WebhookEvent::PullRequestClosed { number, actor, .. } => {
            format!("{actor} closed PR #{number} without merging.")
        }
        WebhookEvent::CiFailed { head, check, url } if url.is_empty() => {
            format!("CI check \"{check}\" failed on {head}.")
        }
        WebhookEvent::CiFailed { head, check, url } => {
            format!("CI check \"{check}\" failed on {head}: {url}")
        }
        WebhookEvent::CommentCreated {
            number,
            is_pull_request,
            actor,
            ..
        } => {
            let target = if *is_pull_request { "PR" } else { "issue" };
            format!("{actor} commented on {target} #{number}.")
        }
    }
}

fn repository_identity(raw: &str) -> Option<(String, Option<u16>, String)> {
    let url = Url::parse(raw).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let path = url.path().trim_matches('/').to_ascii_lowercase();
    let path = path.strip_suffix(".git").unwrap_or(&path);
    path.contains('/')
        .then(|| (host, url.port_or_known_default(), path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(head: &str, actor: &str) -> WebhookEvent {
        WebhookEvent::ReviewSubmitted {
            number: 12,
            head: head.into(),
            actor: actor.into(),
            verdict: ReviewVerdict::Approved,
        }
    }

    fn comment(is_pull_request: bool, actor: &str, mentions: &[&str]) -> WebhookEvent {
        WebhookEvent::CommentCreated {
            number: 7,
            is_pull_request,
            actor: actor.into(),
            mentions: mentions.iter().map(|m| (*m).to_string()).collect(),
        }
    }

    fn closed(merged: bool, actor: &str) -> WebhookEvent {
        WebhookEvent::PullRequestClosed {
            number: 12,
            head: "factory/task/DF-1".into(),
            actor: actor.into(),
            merged,
        }
    }

    fn ci(head: &str) -> WebhookEvent {
        WebhookEvent::CiFailed {
            head: head.into(),
            check: "test".into(),
            url: String::new(),
        }
    }

    #[test]
    fn webhook_repository_match_ignores_case_trailing_slash_and_git_suffix() {
        let project = "https://git.example.org/peter/dark-factory.git";
        assert!(repository_matches(
            project,
            "https://Git.Example.org/Peter/Dark-Factory/"
        ));
        assert!(repository_matches(
            "http://forgejo.svc:3000/df/demo",
            "http://forgejo.svc:3000/df/demo.git"
        ));
        assert!(repository_matches(
            "https://github.com/o/r",
            "https://github.com:443/o/r"
        ));
    }

    #[test]
    fn webhook_repository_match_rejects_another_repository_or_host() {
        let project = "https://git.example.org/peter/dark-factory";
        assert!(!repository_matches(
            project,
            "https://git.example.org/eve/other"
        ));
        assert!(!repository_matches(
            project,
            "https://git.example.org/eve/dark-factory"
        ));
        assert!(!repository_matches(
            project,
            "https://evil.example/peter/dark-factory"
        ));
        assert!(!repository_matches(
            project,
            "https://git.example.org:8443/peter/dark-factory"
        ));
        assert!(!repository_matches(project, "not a url"));
        assert!(!repository_matches(project, "https://git.example.org/"));
    }

    #[test]
    fn webhook_events_route_by_branch_pr_number_or_issue() {
        assert_eq!(
            review("factory/task/DF-1", "alice").target(),
            Some(Target::Branch("factory/task/DF-1".into()))
        );
        assert_eq!(
            closed(true, "bob").target(),
            Some(Target::Branch("factory/task/DF-1".into()))
        );
        assert_eq!(
            ci("factory/task/DF-3").target(),
            Some(Target::Branch("factory/task/DF-3".into()))
        );
        assert_eq!(
            review("renovate/serde-1.x", "alice").target(),
            Some(Target::Branch("renovate/serde-1.x".into()))
        );
        assert_eq!(
            comment(true, "alice", &[]).target(),
            Some(Target::PullRequest(7))
        );
        assert_eq!(
            comment(false, "alice", &[]).target(),
            Some(Target::Issue(7))
        );
    }

    #[test]
    fn webhook_only_reviews_and_comments_have_a_feedback_author() {
        assert_eq!(
            review("factory/task/DF-1", "alice").feedback_author(),
            Some("alice")
        );
        assert_eq!(comment(false, "bob", &[]).feedback_author(), Some("bob"));
        assert_eq!(closed(true, "bob").feedback_author(), None);
        assert_eq!(ci("factory/task/DF-1").feedback_author(), None);
    }

    #[test]
    fn webhook_comment_addresses_the_bot_only_when_it_mentions_it() {
        assert!(!comment(true, "alice", &["carol"]).addresses("df-bot"));
        assert!(comment(false, "alice", &["DF-Bot"]).addresses("df-bot"));
        assert!(review("factory/task/DF-1", "alice").addresses("df-bot"));
        assert!(closed(false, "bob").addresses("df-bot"));
    }

    #[test]
    fn webhook_wake_round_trips_through_its_stored_form() {
        for &wake in WebhookWake::ALL {
            assert_eq!(wake.as_str().parse::<WebhookWake>(), Ok(wake));
            assert_eq!(WebhookWake::try_from(wake.as_str().to_string()), Ok(wake));
            assert_eq!(serde_json::to_value(wake).unwrap(), wake.as_str());
        }
        assert_eq!(WebhookWake::default(), WebhookWake::Members);
        assert!("everyone".parse::<WebhookWake>().is_err());
    }

    #[test]
    fn webhook_message_names_the_event_and_points_to_the_feedback_tool_for_reviews() {
        let review = WebhookEvent::ReviewSubmitted {
            number: 12,
            head: "factory/task/DF-1".into(),
            actor: "alice".into(),
            verdict: ReviewVerdict::ChangesRequested,
        };
        assert_eq!(
            render_message(&review),
            "Forge event on your task: alice submitted a review on PR #12: changes requested.\n\
             Read reviews and comments with read_pull_request_feedback. \
             Pass the next_since of your last read as since.\n"
        );
        let comment = WebhookEvent::CommentCreated {
            number: 3,
            is_pull_request: false,
            actor: "carol".into(),
            mentions: vec!["df-bot".into()],
        };
        assert_eq!(
            render_message(&comment),
            "Forge event on your task: carol commented on issue #3.\n\
             Read reviews and comments with read_pull_request_feedback. \
             Pass the next_since of your last read as since.\n"
        );
    }

    #[test]
    fn webhook_message_of_ci_and_close_has_no_feedback_pointer() {
        let ci = WebhookEvent::CiFailed {
            head: "factory/task/DF-1".into(),
            check: "test".into(),
            url: "https://ci.example/1".into(),
        };
        assert_eq!(
            render_message(&ci),
            "Forge event on your task: CI check \"test\" failed on factory/task/DF-1: https://ci.example/1\n"
        );
        let lint = WebhookEvent::CiFailed {
            head: "main".into(),
            check: "lint".into(),
            url: String::new(),
        };
        assert_eq!(
            render_message(&lint),
            "Forge event on your task: CI check \"lint\" failed on main.\n"
        );
        assert_eq!(
            render_message(&closed(false, "bob")),
            "Forge event on your task: bob closed PR #12 without merging.\n"
        );
    }
}
