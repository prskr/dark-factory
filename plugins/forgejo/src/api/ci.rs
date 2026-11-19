//! `ci.status` from commit statuses and `ci.job-log` from the Actions API.

use serde::Deserialize;

use crate::darkfactory::integrations::types::{Error, RepoRef};
use crate::error;
use crate::exports::darkfactory::integrations::ci::{Check, CiState, CiStatus, LogTail};
use forge_common::http;
use forge_common::log::Log;
use forge_common::log_tail;
use forge_common::url;

use super::{Forgejo, Method, PAGE_SIZE, Request, Transport, null_as_empty, repo_path};

/// The Actions API (job ids and logs) exists from this Forgejo version.
const ACTIONS_API_VERSION: u32 = 16;

impl<T: Transport, L: Log> Forgejo<T, L> {
    /// The checks of `git_ref`, one per status context. On Forgejo v16 or later, a check of a
    /// Forgejo Actions job carries the job id.
    pub fn ci_status(&self, repo: &RepoRef, git_ref: &str) -> Result<CiStatus, Error> {
        // Forgejo routes `commits/{ref}/status` by one path segment: a `/` of the ref is `%2F`.
        let path = format!(
            "{}/commits/{}/status",
            repo_path(repo),
            url::encode(git_ref)
        );
        let mut sha = String::new();
        let mut statuses = Vec::new();
        for page in 1.. {
            let combined: ApiCombinedStatus = self.call(
                Method::Get,
                format!("{path}?page={page}&limit={PAGE_SIZE}"),
                None,
            )?;
            if combined.statuses.is_empty() {
                break;
            }
            sha = combined.sha;
            statuses.extend(combined.statuses);
        }
        let jobs = if statuses.is_empty() || self.major_version()? < ACTIONS_API_VERSION {
            Vec::new()
        } else {
            match self.jobs_of(repo, &sha) {
                // Actions are disabled for the repository: the statuses come from elsewhere.
                Err(Error::NotFound(_)) => Vec::new(),
                other => other?,
            }
        };
        let checks: Vec<Check> = statuses
            .into_iter()
            .map(|s| Check {
                state: check_state(&s.status),
                job_id: jobs.iter().find(|j| j.context == s.context).map(|j| j.id),
                url: Some(s.target_url).filter(|u| !u.is_empty()),
                name: s.context,
            })
            .collect();
        Ok(CiStatus {
            state: combined_state(&checks),
            checks,
        })
    }

    /// At most the last 4 MiB of the log of job `job_id`. Needs Forgejo v16 or later.
    pub fn job_log(&self, repo: &RepoRef, job_id: u64) -> Result<LogTail, Error> {
        self.require_version(ACTIONS_API_VERSION, "job logs need")?;
        let path = format!("{}/actions/jobs/{job_id}/logs", repo_path(repo));
        let request_line = http::request_line(Method::Get, &path);
        let request = Request {
            method: Method::Get,
            path,
            body: None,
            headers: vec![("range", format!("bytes=-{}", log_tail::MAX_BYTES))],
        };
        let (response, cut) = self.transport.send_tail(request, log_tail::MAX_BYTES)?;
        if !(200..300).contains(&response.status) {
            return Err(error::with_request(
                error::from_status(response.status, response.retry_after, &response.body),
                &request_line,
            ));
        }
        Ok(LogTail {
            text: log_tail::text(&response.body, cut),
            head_dropped: cut,
        })
    }

    /// The jobs of the Actions runs of commit `sha`, by the status context they report.
    // shortcut: the first page of 50 runs, when a commit has more runs than that, page.
    fn jobs_of(&self, repo: &RepoRef, sha: &str) -> Result<Vec<Job>, Error> {
        let runs: ApiRuns = self.call(
            Method::Get,
            format!(
                "{}/actions/runs?head_sha={}&limit={PAGE_SIZE}",
                repo_path(repo),
                url::encode(sha)
            ),
            None,
        )?;
        let mut jobs = Vec::new();
        for run in runs.workflow_runs {
            let listed: ApiJobs = self.call(
                Method::Get,
                format!("{}/actions/runs/{}/jobs", repo_path(repo), run.id),
                None,
            )?;
            for job in listed.jobs {
                for workflow in run.workflow_names() {
                    jobs.push(Job {
                        context: format!("{workflow} / {} ({})", job.name, run.event),
                        id: job.id,
                    });
                }
            }
        }
        Ok(jobs)
    }
}

struct Job {
    /// `<workflow> / <job> (<event>)`
    context: String,
    id: u64,
}

fn check_state(status: &str) -> CiState {
    match status {
        "success" | "warning" => CiState::Success,
        "failure" | "error" => CiState::Failure,
        _ => CiState::Pending,
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

#[derive(Deserialize)]
struct ApiCombinedStatus {
    #[serde(default)]
    sha: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    statuses: Vec<ApiStatus>,
}

#[derive(Deserialize)]
struct ApiStatus {
    context: String,
    status: String,
    #[serde(default)]
    target_url: String,
}

#[derive(Deserialize)]
struct ApiRuns {
    #[serde(default, deserialize_with = "null_as_empty")]
    workflow_runs: Vec<ApiRun>,
}

#[derive(Deserialize)]
struct ApiRun {
    id: u64,
    #[serde(default)]
    name: String,
    /// Workflow file, e.g. `.forgejo/workflows/ci.yml`.
    #[serde(default)]
    path: String,
    #[serde(default)]
    event: String,
}

impl ApiRun {
    /// What the status context may call the workflow: its `name`, else its file's stem.
    fn workflow_names(&self) -> Vec<String> {
        let file = self.path.rsplit('/').next().unwrap_or_default();
        let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
        [self.name.as_str(), stem]
            .into_iter()
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .collect()
    }
}

#[derive(Deserialize)]
struct ApiJobs {
    #[serde(default, deserialize_with = "null_as_empty")]
    jobs: Vec<ApiJob>,
}

#[derive(Deserialize)]
struct ApiJob {
    id: u64,
    name: String,
}

#[cfg(test)]
mod tests;
