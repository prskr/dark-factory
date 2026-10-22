//! Mapping between the `df-core` seam types and the generated WIT types.

use crate::host::{issues, pull_requests as prs, wit};

pub fn repo(r: &df_core::RepoRef) -> wit::RepoRef {
    wit::RepoRef {
        owner: r.owner.clone(),
        name: r.name.clone(),
    }
}

fn from_repo(r: wit::RepoRef) -> df_core::RepoRef {
    df_core::RepoRef {
        owner: r.owner,
        name: r.name,
    }
}

pub fn pull_request_ref(repo_ref: &df_core::RepoRef, number: u64) -> wit::PullRequestRef {
    wit::PullRequestRef {
        repo: repo(repo_ref),
        number,
    }
}

pub fn issue_ref(repo_ref: &df_core::RepoRef, number: u64) -> wit::IssueRef {
    wit::IssueRef {
        repo: repo(repo_ref),
        number,
    }
}

pub fn new_pull_request(r: df_core::NewPullRequest) -> prs::NewPullRequest {
    prs::NewPullRequest {
        repo: repo(&r.repo),
        head: r.head,
        base: r.base,
        title: r.title,
        body: r.body,
        draft: r.draft,
    }
}

pub fn pull_request_update(c: df_core::PullRequestUpdate) -> prs::PullRequestUpdate {
    prs::PullRequestUpdate {
        title: c.title,
        body: c.body,
        draft: c.draft,
    }
}

pub fn from_pull_request(p: prs::PullRequest) -> df_core::PullRequest {
    df_core::PullRequest {
        id: df_core::PullRequestRef {
            repo: from_repo(p.id.repo),
            number: p.id.number,
        },
        url: p.url,
        head: p.head,
        base: p.base,
        title: p.title,
        body: p.body,
        draft: p.draft,
        state: match p.state {
            prs::PullRequestState::Open => df_core::PullRequestState::Open,
            prs::PullRequestState::Closed => df_core::PullRequestState::Closed,
            prs::PullRequestState::Merged => df_core::PullRequestState::Merged,
        },
        head_sha: p.head_sha,
    }
}

pub fn from_issue(i: issues::Issue) -> df_core::Issue {
    df_core::Issue {
        id: df_core::IssueRef {
            repo: from_repo(i.id.repo),
            number: i.id.number,
        },
        url: i.url,
        title: i.title,
        body: i.body,
        state: match i.state {
            issues::IssueState::Open => df_core::IssueState::Open,
            issues::IssueState::Closed => df_core::IssueState::Closed,
        },
        labels: i.labels,
    }
}

pub fn from_comment(c: issues::Comment) -> df_core::Comment {
    df_core::Comment {
        id: c.id,
        url: c.url,
    }
}

pub fn from_error(e: wit::Error) -> df_core::IntegrationError {
    use df_core::IntegrationError as E;
    match e {
        wit::Error::NotFound(m) => E::NotFound(m),
        wit::Error::Unauthorized(m) => E::Unauthorized(m),
        wit::Error::Conflict(m) => E::Conflict(m),
        wit::Error::InvalidInput(m) => E::InvalidInput(m),
        wit::Error::RateLimited(r) => E::RateLimited(df_core::RateLimit {
            retry_after_seconds: r.retry_after_seconds,
            message: r.message,
        }),
        wit::Error::Upstream(u) => E::Upstream(df_core::UpstreamError {
            status: u.status,
            message: u.message,
        }),
        wit::Error::Config(m) => E::Config(m),
        wit::Error::Transport(m) => E::Transport(m),
        wit::Error::Internal(m) => E::Internal(m),
    }
}
