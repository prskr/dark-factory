//! `issues.list` and `issues.create`.

use serde::Deserialize;
use serde_json::json;

use crate::darkfactory::integrations::types::{Error, IssueRef, RepoRef};
use crate::exports::darkfactory::integrations::issues::{
    Issue, IssueFilter, IssuePage, IssueState, NewIssue, StateFilter,
};
use forge_common::log::Log;
use forge_common::url;

use super::{ApiIssue, Clock, GitHub, LIST_PAGE_SIZE, Method, Transport, repo_path};

/// Forge pages read per `list` call.
const MAX_PAGES: usize = 5;
/// Pages of the label listing read to resolve names, so a forge that never ends one can't loop
/// the plugin.
const MAX_LABEL_PAGES: usize = 50;
/// GitHub's search returns at most this many results of a query.
const SEARCH_CAP: usize = 1000;

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

/// One forge page of a listing.
struct Fetched {
    items: Vec<ApiIssue>,
    /// GitHub says the search was cut short (`incomplete_results`).
    incomplete: bool,
    /// A page after this one can be requested (false at the search cap).
    next_readable: bool,
    /// Matching issues exist beyond what can be requested (search cap).
    more_beyond: bool,
}

impl<T: Transport, C: Clock, L: Log> GitHub<T, C, L> {
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
        let search = search_query(repo, filter)?;
        let mut issues = Vec::new();
        for _ in 0..MAX_PAGES {
            let fetched = match &search {
                Some(q) => self.search_page(q, at.page)?,
                None => self.repo_page(repo, filter, at.page)?,
            };
            let raw_len = fetched.items.len();
            for item in fetched.items.into_iter().skip(at.offset) {
                let wanted = item.pull_request.is_none();
                if wanted && issues.len() == limit {
                    return Ok(page(issues, Some(&at), true));
                }
                if wanted {
                    issues.push(issue_of(repo, item));
                }
                at.offset += 1;
            }
            let next = Position {
                page: at.page + 1,
                offset: 0,
            };
            if fetched.incomplete {
                return Ok(page(issues, fetched.next_readable.then_some(&next), true));
            }
            if raw_len < LIST_PAGE_SIZE || !fetched.next_readable {
                return Ok(page(issues, None, fetched.more_beyond));
            }
            at = next;
        }
        Ok(page(issues, Some(&at), true))
    }

    /// Creates the issue; an unknown label would be created by GitHub, so the names are checked
    /// first: an unknown one is `invalid-input` and nothing is created.
    pub fn create_issue(&self, repo: &RepoRef, issue: &NewIssue) -> Result<Issue, Error> {
        let labels = self.existing_labels(repo, &issue.labels)?;
        let created: ApiIssue = self.rest(
            Method::Post,
            &format!("{}/issues", repo_path(repo)),
            Some(json!({"title": issue.title, "body": issue.body, "labels": labels})),
        )?;
        Ok(issue_of(repo, created))
    }

    /// The repository's own spelling of `names`.
    fn existing_labels(&self, repo: &RepoRef, names: &[String]) -> Result<Vec<String>, Error> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let mut resolved: Vec<Option<String>> = vec![None; names.len()];
        for page in 1..=MAX_LABEL_PAGES {
            let path = format!(
                "{}/labels?per_page={LIST_PAGE_SIZE}&page={page}",
                repo_path(repo)
            );
            let labels: Vec<ApiRepoLabel> = self.rest(Method::Get, &path, None)?;
            let last = labels.len() < LIST_PAGE_SIZE;
            for (found, name) in resolved.iter_mut().zip(names) {
                if found.is_none() {
                    *found = labels
                        .iter()
                        .find(|l| l.name.eq_ignore_ascii_case(name))
                        .map(|l| l.name.clone());
                }
            }
            if last || resolved.iter().all(Option::is_some) {
                break;
            }
        }
        names
            .iter()
            .zip(resolved)
            .map(|(name, found)| {
                found.ok_or_else(|| {
                    Error::InvalidInput(format!("the repository has no label `{name}`"))
                })
            })
            .collect()
    }

    /// One page of the repository's issue listing, which also holds pull requests.
    fn repo_page(
        &self,
        repo: &RepoRef,
        filter: &IssueFilter,
        page: usize,
    ) -> Result<Fetched, Error> {
        let labels = if filter.labels.is_empty() {
            String::new()
        } else {
            let labels: Vec<String> = filter.labels.iter().map(|l| url::encode(l)).collect();
            format!("&labels={}", labels.join("%2C"))
        };
        let path = format!(
            "{}/issues?state={}{labels}&sort=created&direction=desc&per_page={LIST_PAGE_SIZE}&page={page}",
            repo_path(repo),
            state_name(filter.state)
        );
        Ok(Fetched {
            items: self.rest(Method::Get, &path, None)?,
            incomplete: false,
            next_readable: true,
            more_beyond: false,
        })
    }

    /// One page of the search for `q`, at most [`SEARCH_CAP`] results in all.
    fn search_page(&self, q: &str, page: usize) -> Result<Fetched, Error> {
        let path = format!(
            "/search/issues?q={}&sort=created&order=desc&per_page={LIST_PAGE_SIZE}&page={page}",
            url::encode(q)
        );
        let result: ApiSearch = self.rest(Method::Get, &path, None)?;
        let at_cap = page * LIST_PAGE_SIZE >= SEARCH_CAP;
        Ok(Fetched {
            items: result.items,
            incomplete: result.incomplete_results,
            next_readable: !at_cap,
            more_beyond: at_cap && result.total_count > SEARCH_CAP,
        })
    }
}

/// The search query for a filter with free text, `None` for a plain listing. The text is one
/// quoted phrase without double quotes, so it can't carry qualifiers.
fn search_query(repo: &RepoRef, filter: &IssueFilter) -> Result<Option<String>, Error> {
    let Some(text) = &filter.query else {
        return Ok(None);
    };
    let phrase = text.replace('"', " ");
    let phrase = phrase.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut parts = vec![
        "is:issue".to_string(),
        format!("repo:{}/{}", repo.owner, repo.name),
        "in:title,body".to_string(),
    ];
    match filter.state {
        StateFilter::Open => parts.push("is:open".into()),
        StateFilter::Closed => parts.push("is:closed".into()),
        StateFilter::All => {}
    }
    for label in &filter.labels {
        if label.contains('"') {
            return Err(Error::InvalidInput(format!(
                "label `{label}` can't be combined with a text query"
            )));
        }
        parts.push(format!("label:\"{label}\""));
    }
    if !phrase.is_empty() {
        parts.push(format!("\"{phrase}\""));
    }
    Ok(Some(parts.join(" ")))
}

fn state_name(state: StateFilter) -> &'static str {
    match state {
        StateFilter::Open => "open",
        StateFilter::Closed => "closed",
        StateFilter::All => "all",
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
        body: issue.body.unwrap_or_default(),
        state: if issue.state == "closed" {
            IssueState::Closed
        } else {
            IssueState::Open
        },
        labels: issue.labels.into_iter().map(|l| l.name).collect(),
        pull_request: issue.pull_request.is_some(),
    }
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
    name: String,
}

#[derive(Deserialize)]
struct ApiSearch {
    #[serde(default)]
    total_count: usize,
    #[serde(default)]
    incomplete_results: bool,
    items: Vec<ApiIssue>,
}

#[cfg(test)]
mod tests;
