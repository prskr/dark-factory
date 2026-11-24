//! `issues.list` and `issues.create`.

use serde::Deserialize;

use crate::darkfactory::integrations::types::{Error, IssueRef, RepoRef};
use crate::exports::darkfactory::integrations::issues::{
    Issue, IssueFilter, IssuePage, IssueState, NewIssue, StateFilter,
};
use forge_common::log::Log;
use forge_common::url;

use super::{ApiIssue, Forgejo, Method, PAGE_SIZE, Transport, repo_path};

/// Forge pages read per `list` call.
const MAX_PAGES: usize = 5;
/// Pages of the label listing read to resolve names, so a forge that never ends one can't loop
/// the plugin.
const MAX_LABEL_PAGES: usize = 50;

/// Where a listing resumes: a forge page and the index of the next raw item in it (counted before
/// dropping pull requests), written `page:offset`.
struct Position {
    page: usize,
    offset: usize,
}

impl Position {
    fn parse(cursor: Option<&str>) -> Result<Self, Error> {
        let Some(cursor) = cursor else {
            return Ok(Self { page: 1, offset: 0 });
        };
        cursor
            .split_once(':')
            .and_then(|(page, offset)| {
                Some(Self {
                    page: page.parse().ok().filter(|p| *p >= 1)?,
                    offset: offset.parse().ok()?,
                })
            })
            .ok_or_else(|| Error::InvalidInput(format!("invalid cursor `{cursor}`")))
    }

    fn cursor(&self) -> String {
        format!("{}:{}", self.page, self.offset)
    }
}

impl<T: Transport, L: Log> Forgejo<T, L> {
    /// At most `limit` issues (never pull requests), newest first, from the position of `cursor`.
    pub fn list_issues(
        &self,
        repo: &RepoRef,
        filter: &IssueFilter,
        limit: u32,
        cursor: Option<&str>,
    ) -> Result<IssuePage, Error> {
        if limit == 0 {
            return Err(Error::InvalidInput("limit must be at least 1".into()));
        }
        let limit = limit as usize;
        let mut at = Position::parse(cursor)?;
        let query = list_query(filter);
        let mut issues = Vec::new();
        for _ in 0..MAX_PAGES {
            let raw: Vec<ApiIssue> = self.call(
                Method::Get,
                format!(
                    "{}/issues?{query}&page={}&limit={PAGE_SIZE}",
                    repo_path(repo),
                    at.page
                ),
                None,
            )?;
            let raw_len = raw.len();
            for item in raw.into_iter().skip(at.offset) {
                // Forgejo's `type=issues` already excludes pull requests; the field is a guard.
                let wanted = item.pull_request.is_none()
                    && filter
                        .labels
                        .iter()
                        .all(|l| item.labels.iter().any(|have| &have.name == l));
                if wanted && issues.len() == limit {
                    return Ok(page(issues, Some(&at), true));
                }
                if wanted {
                    issues.push(issue_of(repo, item));
                }
                at.offset += 1;
            }
            // Forgejo may cap a page below the requested size (`MAX_RESPONSE_ITEMS`), so only an
            // empty page is the end.
            if raw_len == 0 {
                return Ok(page(issues, None, false));
            }
            at = Position {
                page: at.page + 1,
                offset: 0,
            };
        }
        Ok(page(issues, Some(&at), true))
    }

    /// Creates the issue; label names are resolved to ids first, an unknown one is `invalid-input`
    /// and nothing is created.
    pub fn create_issue(&self, repo: &RepoRef, issue: &NewIssue) -> Result<Issue, Error> {
        let labels = self.label_ids(repo, &issue.labels)?;
        let created: ApiIssue = self.call(
            Method::Post,
            format!("{}/issues", repo_path(repo)),
            Some(serde_json::json!({
                "title": issue.title,
                "body": issue.body,
                "labels": labels,
            })),
        )?;
        Ok(issue_of(repo, created))
    }

    fn label_ids(&self, repo: &RepoRef, names: &[String]) -> Result<Vec<u64>, Error> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let mut ids: Vec<Option<u64>> = vec![None; names.len()];
        for page in 1..=MAX_LABEL_PAGES {
            let labels: Vec<ApiRepoLabel> = self.call(
                Method::Get,
                format!("{}/labels?page={page}&limit={PAGE_SIZE}", repo_path(repo)),
                None,
            )?;
            // An empty page is the end: Forgejo may cap a page below the requested size.
            if labels.is_empty() {
                break;
            }
            for (id, name) in ids.iter_mut().zip(names) {
                if id.is_none() {
                    *id = labels.iter().find(|l| &l.name == name).map(|l| l.id);
                }
            }
            if ids.iter().all(Option::is_some) {
                break;
            }
        }
        names
            .iter()
            .zip(ids)
            .map(|(name, id)| {
                id.ok_or_else(|| {
                    Error::InvalidInput(format!("the repository has no label `{name}`"))
                })
            })
            .collect()
    }
}

/// The issue record of an API issue of `repo`.
pub(super) fn issue_of(repo: &RepoRef, issue: ApiIssue) -> Issue {
    Issue {
        id: IssueRef {
            repo: repo.clone(),
            number: issue.number,
        },
        url: issue.html_url,
        title: issue.title,
        body: issue.body,
        state: if issue.state == "closed" {
            IssueState::Closed
        } else {
            IssueState::Open
        },
        labels: issue.labels.into_iter().map(|l| l.name).collect(),
        pull_request: issue.pull_request.is_some(),
    }
}

fn list_query(filter: &IssueFilter) -> String {
    let state = match filter.state {
        StateFilter::Open => "open",
        StateFilter::Closed => "closed",
        StateFilter::All => "all",
    };
    let mut parts = vec!["type=issues".to_string(), format!("state={state}")];
    if !filter.labels.is_empty() {
        let labels: Vec<String> = filter.labels.iter().map(|l| url::encode(l)).collect();
        parts.push(format!("labels={}", labels.join("%2C")));
    }
    if let Some(text) = &filter.query {
        parts.push(format!("q={}", url::encode(text)));
    }
    parts.join("&")
}

fn page(issues: Vec<Issue>, next: Option<&Position>, truncated: bool) -> IssuePage {
    IssuePage {
        issues,
        next_cursor: next.map(Position::cursor),
        truncated,
    }
}

#[derive(Deserialize)]
struct ApiRepoLabel {
    id: u64,
    name: String,
}

#[cfg(test)]
mod tests;
