//! `merge` and `create-branch`.

use percent_encoding::utf8_percent_encode;
use serde::Deserialize;
use serde_json::json;

use crate::darkfactory::integrations::types::{Error, PullRequestRef, RepoRef};
use crate::exports::darkfactory::integrations::pull_requests::PullRequestState;
use forge_common::log::Log;

use super::{Clock, GitHub, Method, REF_PATH, Transport, repo_path};

impl<T: Transport, C: Clock, L: Log> GitHub<T, C, L> {
    /// Merges with the first merge method the repository allows (`merge`, `squash`, `rebase`) if
    /// the head is still `head_sha`. An already merged pull request is read, not merged again.
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
        let repo: ApiRepo = self.rest(Method::Get, &repo_path(&id.repo), None)?;
        let method = repo.merge_method().ok_or_else(|| {
            Error::InvalidInput("the repository allows no merge method".to_string())
        })?;
        let path = format!("{}/pulls/{}/merge", repo_path(&id.repo), id.number);
        let body = json!({"sha": head_sha, "merge_method": method});
        self.rest::<serde_json::Value>(Method::Put, &path, Some(body))
            .map(drop)
    }

    /// Creates `name` at the tip of `from`, which is resolved first.
    pub fn create_branch(&self, repo: &RepoRef, name: &str, from: &str) -> Result<(), Error> {
        let lookup = format!(
            "{}/git/ref/heads/{}",
            repo_path(repo),
            utf8_percent_encode(from, REF_PATH)
        );
        let origin: ApiRef = self.rest(Method::Get, &lookup, None)?;
        let body = json!({"ref": format!("refs/heads/{name}"), "sha": origin.object.sha});
        self.rest::<serde_json::Value>(
            Method::Post,
            &format!("{}/git/refs", repo_path(repo)),
            Some(body),
        )
        .map(drop)
    }
}

#[derive(Deserialize)]
struct ApiRepo {
    #[serde(default)]
    allow_merge_commit: bool,
    #[serde(default)]
    allow_squash_merge: bool,
    #[serde(default)]
    allow_rebase_merge: bool,
}

impl ApiRepo {
    fn merge_method(&self) -> Option<&'static str> {
        [
            (self.allow_merge_commit, "merge"),
            (self.allow_squash_merge, "squash"),
            (self.allow_rebase_merge, "rebase"),
        ]
        .into_iter()
        .find_map(|(allowed, method)| allowed.then_some(method))
    }
}

#[derive(Deserialize)]
struct ApiRef {
    object: ApiObject,
}

#[derive(Deserialize)]
struct ApiObject {
    sha: String,
}

#[cfg(test)]
mod tests;
