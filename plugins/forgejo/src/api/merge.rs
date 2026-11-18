//! `merge` and `create-branch`.

use serde::Deserialize;

use crate::darkfactory::integrations::types::{Error, PullRequestRef, RepoRef};
use crate::exports::darkfactory::integrations::pull_requests::PullRequestState;
use forge_common::log::Log;

use super::{Failure, Forgejo, Method, Transport, repo_path};

/// Said when Forgejo refuses a merge without a reason.
const NOT_MERGEABLE: &str = "the pull request is not mergeable";

impl<T: Transport, L: Log> Forgejo<T, L> {
    /// Merges with the repository's default merge style if the head is still `head_sha`.
    ///
    /// The state is read first: Forgejo answers an already merged pull request with an empty 405,
    /// which could not be told from a conflict.
    pub fn merge(&self, id: &PullRequestRef, head_sha: &str) -> Result<(), Error> {
        let pr = self.get(id)?;
        match pr.state {
            PullRequestState::Merged if pr.head_sha == head_sha => return Ok(()),
            PullRequestState::Closed => {
                return Err(Error::InvalidInput(
                    "the pull request is closed without being merged".into(),
                ));
            }
            _ => {}
        }
        if pr.head_sha != head_sha {
            return Err(Error::Conflict(format!(
                "the head of the pull request is {}, not {head_sha}",
                pr.head_sha
            )));
        }
        let repo: ApiRepo = self.call(Method::Get, repo_path(&id.repo), None)?;
        let body = serde_json::json!({
            "Do": repo.default_merge_style,
            "head_commit_id": head_sha,
        });
        let path = format!("{}/pulls/{}/merge", repo_path(&id.repo), id.number);
        self.send_checked(Method::Post, path, Some(body))
            .map(drop)
            .map_err(|failure| match failure.error {
                Error::Conflict(m) if m.is_empty() => Error::Conflict(NOT_MERGEABLE.into()),
                other => other,
            })
    }

    /// Creates `name` at the tip of `from`.
    pub fn create_branch(&self, repo: &RepoRef, name: &str, from: &str) -> Result<(), Error> {
        let body = serde_json::json!({"new_branch_name": name, "old_ref_name": from});
        let path = format!("{}/branches", repo_path(repo));
        self.send_checked(Method::Post, path, Some(body))
            .map(drop)
            .map_err(|failure| unknown_origin(failure, from))
    }
}

/// Forgejo answers a branch created from an unknown ref with a 500 "object does not exist".
fn unknown_origin(failure: Failure, from: &str) -> Error {
    match failure.error {
        Error::Upstream(e) if e.status == 500 && e.message.contains("does not exist") => {
            Error::NotFound(format!("branch or ref `{from}`: {}", e.message))
        }
        other => other,
    }
}

#[derive(Deserialize)]
struct ApiRepo {
    default_merge_style: String,
}

#[cfg(test)]
mod tests;
