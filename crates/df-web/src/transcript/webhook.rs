//! The one-line text of a received webhook event: the transcript row and the queued message
//! stack show the same words.

use df_proto::transcript::v1::{WebhookReceived, WebhookReviewVerdict, webhook_received::Kind};

/// The sentence for an event: `lead`, the linked `label` and `tail`. The link (`label`, `url`) is
/// the pull request or the CI check; the URL may be empty or unsafe, the renderer decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Summary {
    pub lead: String,
    pub link: Option<(String, String)>,
    pub tail: String,
}

impl Summary {
    /// The sentence as plain text.
    pub(crate) fn text(&self) -> String {
        let label = self.link.as_ref().map_or("", |(label, _)| label);
        format!("{}{label}{}", self.lead, self.tail)
    }
}

/// The summary of `event`; `None` for an event without a kind (written by a newer control plane).
pub(crate) fn summary(event: &WebhookReceived) -> Option<Summary> {
    let pull_request = |lead: String, number: u64, url: &str, tail: &str| Summary {
        lead,
        link: Some((format!("#{number}"), url.to_owned())),
        tail: tail.to_owned(),
    };
    Some(match event.kind.as_ref()? {
        Kind::ReviewSubmitted(r) => {
            let verb = match r.verdict() {
                WebhookReviewVerdict::Approved => "approved",
                WebhookReviewVerdict::ChangesRequested => "requested changes on",
                WebhookReviewVerdict::Commented | WebhookReviewVerdict::Unspecified => "reviewed",
            };
            pull_request(format!("{} {verb} PR ", r.actor), r.number, &r.url, "")
        }
        Kind::CommentCreated(c) => {
            let target = if c.is_pull_request { "PR" } else { "issue" };
            pull_request(
                format!("{} commented on {target} ", c.actor),
                c.number,
                &c.url,
                "",
            )
        }
        Kind::PullRequestClosed(c) => pull_request(
            format!("{} closed PR ", c.actor),
            c.number,
            &c.url,
            " without merging",
        ),
        Kind::CiFailed(c) => Summary {
            lead: "CI check ".to_owned(),
            link: Some((c.check.clone(), c.url.clone())),
            tail: format!(
                " failed on {}",
                c.head.strip_prefix("refs/heads/").unwrap_or(&c.head)
            ),
        },
    })
}
