//! Forgejo WASI integration plugin implementing `darkfactory:integrations/forge@0.1.0`.

wit_bindgen::generate!({
    path: "../../contract/wit/integrations",
    world: "forge",
    generate_all,
});

use darkfactory::integrations::types::{Error, IssueRef, PullRequestRef, RepoRef};
use exports::darkfactory::integrations::issues::{
    Comment, Guest as IssuesGuest, Issue, IssueState,
};
use exports::darkfactory::integrations::pull_requests::{
    Guest as PullRequestsGuest, NewPullRequest, PullRequest, PullRequestState, PullRequestUpdate,
};

pub struct ForgejoPlugin;

impl PullRequestsGuest for ForgejoPlugin {
    fn create(request: NewPullRequest) -> Result<PullRequest, Error> {
        Ok(PullRequest {
            id: PullRequestRef {
                repo: request.repo,
                number: 1,
            },
            url: String::from("https://forgejo.invalid/pr/1"),
            head: request.head,
            base: request.base,
            title: request.title,
            body: request.body,
            draft: request.draft,
            state: PullRequestState::Open,
            head_sha: String::from("0000000000000000000000000000000000000000"),
        })
    }

    fn find_open(
        _repo: RepoRef,
        _head: String,
        _base: String,
    ) -> Result<Option<PullRequest>, Error> {
        Ok(None)
    }

    fn get(id: PullRequestRef) -> Result<PullRequest, Error> {
        Err(Error::NotFound(format!(
            "pull request #{} not found",
            id.number
        )))
    }

    fn update(id: PullRequestRef, _changes: PullRequestUpdate) -> Result<PullRequest, Error> {
        Err(Error::NotFound(format!(
            "pull request #{} not found",
            id.number
        )))
    }
}

impl IssuesGuest for ForgejoPlugin {
    fn get(id: IssueRef) -> Result<Issue, Error> {
        Ok(Issue {
            id,
            url: String::from("https://forgejo.invalid/issues/1"),
            title: String::from("placeholder"),
            body: String::new(),
            state: IssueState::Open,
            labels: Vec::new(),
        })
    }

    fn add_comment(_id: IssueRef, _body: String) -> Result<Comment, Error> {
        Ok(Comment {
            id: String::from("1"),
            url: String::from("https://forgejo.invalid/issues/1#issuecomment-1"),
        })
    }
}

#[cfg(target_arch = "wasm32")]
export!(ForgejoPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_placeholder_pull_request() {
        let pr = ForgejoPlugin::create(NewPullRequest {
            repo: RepoRef {
                owner: "prskr".to_string(),
                name: "dark-factory".to_string(),
            },
            head: "factory/task/DF-1".to_string(),
            base: "main".to_string(),
            title: "Add feature".to_string(),
            body: "Body".to_string(),
            draft: true,
        })
        .expect("create should succeed");
        assert_eq!(pr.id.number, 1);
        assert!(pr.draft);
    }
}
