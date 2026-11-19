//! `ci.status` from check runs and commit statuses, `ci.job-log` from the Actions job log.

use serde::Deserialize;

use crate::darkfactory::integrations::types::{Error, RepoRef, UpstreamError};
use crate::error;
use crate::exports::darkfactory::integrations::ci::{Check, CiState, CiStatus, LogTail};
use forge_common::http;
use forge_common::log::Log;
use forge_common::log_tail;
use forge_common::url;

use super::{Clock, GitHub, LIST_PAGE_SIZE, Method, Request, Transport, USER_AGENT, repo_path};

/// The app of check runs created by GitHub Actions; their id is the job id.
const ACTIONS_APP: &str = "github-actions";

impl<T: Transport, C: Clock, L: Log> GitHub<T, C, L> {
    /// The latest check run per name and the legacy commit statuses of `git_ref`, as one list.
    pub fn ci_status(&self, repo: &RepoRef, git_ref: &str) -> Result<CiStatus, Error> {
        let commit = format!("{}/commits/{}", repo_path(repo), url::encode(git_ref));
        let mut checks = Vec::new();
        for page in 1.. {
            let path =
                format!("{commit}/check-runs?filter=latest&per_page={LIST_PAGE_SIZE}&page={page}");
            let runs: ApiCheckRuns = self
                .rest(Method::Get, &path, None)
                .map_err(unknown_ref_not_found)?;
            let last_page = runs.check_runs.len() < LIST_PAGE_SIZE;
            checks.extend(runs.check_runs.into_iter().map(check_of_run));
            if last_page {
                break;
            }
        }
        for page in 1.. {
            let path = format!("{commit}/status?per_page={LIST_PAGE_SIZE}&page={page}");
            let combined: ApiCombinedStatus = self
                .rest(Method::Get, &path, None)
                .map_err(unknown_ref_not_found)?;
            let last_page = combined.statuses.len() < LIST_PAGE_SIZE;
            checks.extend(combined.statuses.into_iter().map(check_of_status));
            if last_page {
                break;
            }
        }
        Ok(CiStatus {
            state: combined_state(&checks),
            checks,
        })
    }

    /// At most the last 4 MiB of the log of Actions job `job_id`, without the byte order mark and
    /// the timestamp GitHub puts in front of every line.
    ///
    /// GitHub answers with a redirect to a short-lived storage URL. The plugin follows it once;
    /// the host sends no credential to that host (`extraHosts` of the plugin entry).
    pub fn job_log(&self, repo: &RepoRef, job_id: u64) -> Result<LogTail, Error> {
        let url = format!(
            "{}{}/actions/jobs/{job_id}/logs",
            self.settings.rest_url,
            repo_path(repo)
        );
        let request = http::request_line(Method::Get, &url);
        let first = self.send(Method::Get, url, None)?;
        let (response, cut) = match first.status {
            302 | 307 => {
                let location = first.location.ok_or_else(|| {
                    Error::Internal("GitHub redirected the log without a location".to_string())
                })?;
                let target = Request {
                    method: Method::Get,
                    url: location,
                    headers: vec![("user-agent", USER_AGENT.to_string())],
                    body: None,
                };
                self.transport.send_tail(target, log_tail::MAX_BYTES)?
            }
            200..=299 => {
                let mut response = first;
                let cut = log_tail::keep_tail(&mut response.body, log_tail::MAX_BYTES);
                (response, cut)
            }
            410 => {
                return Err(Error::NotFound(format!(
                    "the log of job {job_id} has expired ({request})"
                )));
            }
            _ => {
                return Err(error::with_request(
                    error::from_response(&first, self.clock.now()),
                    &request,
                ));
            }
        };
        if !(200..300).contains(&response.status) {
            return Err(Error::Upstream(UpstreamError {
                status: response.status,
                message: "the log storage refused the request".to_string(),
            }));
        }
        Ok(LogTail {
            text: clean_log(&log_tail::text(&response.body, cut)),
            head_dropped: cut,
        })
    }
}

/// GitHub answers an unknown ref with `422 No commit found for SHA: <ref>`, which is a missing
/// object, not a rejected input.
// shortcut: matches GitHub's "No commit found" wording of the 422, there is no error code to
// match; change it if the recorded response in `an_unknown_ref_is_not_found` changes.
fn unknown_ref_not_found(error: Error) -> Error {
    match error {
        Error::InvalidInput(message) if message.contains("No commit found") => {
            Error::NotFound(message)
        }
        other => other,
    }
}

fn check_of_run(run: ApiCheckRun) -> Check {
    let state = if run.status == "completed" {
        match run.conclusion.as_deref() {
            Some("failure" | "timed_out" | "cancelled" | "action_required") => CiState::Failure,
            _ => CiState::Success,
        }
    } else {
        CiState::Pending
    };
    let is_actions = run.app.is_some_and(|app| app.slug == ACTIONS_APP);
    Check {
        name: run.name,
        state,
        url: run.html_url,
        job_id: is_actions.then_some(run.id),
    }
}

fn check_of_status(status: ApiStatus) -> Check {
    Check {
        name: status.context,
        state: match status.state.as_str() {
            "success" => CiState::Success,
            "failure" | "error" => CiState::Failure,
            _ => CiState::Pending,
        },
        url: status.target_url.filter(|u| !u.is_empty()),
        job_id: None,
    }
}

/// `failure` if any check failed, else `pending` if any is, else `success`; `none` without checks.
fn combined_state(checks: &[Check]) -> CiState {
    let any = |state| checks.iter().any(|c| c.state == state);
    if checks.is_empty() {
        CiState::None
    } else if any(CiState::Failure) {
        CiState::Failure
    } else if any(CiState::Pending) {
        CiState::Pending
    } else {
        CiState::Success
    }
}

/// Removes one leading byte order mark and, of every line, the leading RFC 3339 timestamp
/// (`2026-11-03T10:00:00.1234567Z `).
fn clean_log(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.split_inclusive('\n').map(strip_timestamp).collect()
}

fn strip_timestamp(line: &str) -> &str {
    let Some((first, rest)) = line.split_once(' ') else {
        return line;
    };
    let digits = |range: std::ops::Range<usize>| {
        first
            .get(range)
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    };
    let timestamp = first.ends_with('Z')
        && digits(0..4)
        && first.get(4..5) == Some("-")
        && first.get(10..11) == Some("T");
    if timestamp { rest } else { line }
}

#[derive(Deserialize)]
struct ApiCheckRuns {
    #[serde(default)]
    check_runs: Vec<ApiCheckRun>,
}

#[derive(Deserialize)]
struct ApiCheckRun {
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
    html_url: Option<String>,
    app: Option<ApiApp>,
}

#[derive(Deserialize)]
struct ApiApp {
    slug: String,
}

#[derive(Deserialize)]
struct ApiCombinedStatus {
    #[serde(default)]
    statuses: Vec<ApiStatus>,
}

#[derive(Deserialize)]
struct ApiStatus {
    context: String,
    state: String,
    target_url: Option<String>,
}

#[cfg(test)]
mod tests;
