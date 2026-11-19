//! `list_pull_requests`, `read_ci_status` and `read_ci_log`: read-only views of the project
//! repository's pull requests and CI (`specs/ci-reads`).

use std::borrow::Cow;
use std::sync::LazyLock;

use df_core::{CiState, IntegrationError, ListedPullRequest, PullRequestFilter};
use regex::Regex;
use serde_json::{Value, json};

use super::{Outcome, forge_error, text_arg};
use crate::ChannelContext;
use crate::error::ChannelError;
use df_orchestrator::run_context::RunContext;

/// Most pull requests `list_pull_requests` returns.
const MAX_PULL_REQUESTS: usize = 100;

/// Bytes of a log page.
const LOG_PAGE_BYTES: usize = 64 * 1024;

/// CSI (`ESC [ … final`), OSC (`ESC ] … BEL` or `ESC ] … ESC \`), any other two-character escape
/// and a lone ESC at the end of the text.
static ANSI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|.)?")
        .expect("the ANSI pattern is valid")
});

/// The open pull requests of the project repository that match the filters, at most
/// [`MAX_PULL_REQUESTS`] ordered by number.
pub(super) async fn list_pull_requests(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let filter = PullRequestFilter {
        head_prefix: text_arg(args, "head_prefix"),
        author: text_arg(args, "author"),
        label: text_arg(args, "label"),
        base: text_arg(args, "base"),
    };
    let mut prs = match ctx.forge.list_pull_requests(&rc.project.id, &filter).await {
        Ok(prs) => prs,
        Err(e) => return Ok(forge_error(&e)),
    };
    prs.sort_by_key(|p| p.pr.id.number);
    let truncated = prs.len() > MAX_PULL_REQUESTS;
    prs.truncate(MAX_PULL_REQUESTS);
    Ok(structured(&json!({
        "pull_requests": prs.iter().map(pull_request_json).collect::<Vec<_>>(),
        "truncated": truncated,
    })))
}

/// The checks of a pull request's current head or of a ref.
pub(super) async fn read_ci_status(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let (number, git_ref) = (args.get("pull_request"), text_arg(args, "ref"));
    let git_ref = match (number, git_ref) {
        (Some(_), Some(_)) | (None, None) => {
            return Ok(Outcome::error(
                "pass exactly one of `pull_request` and `ref`",
            ));
        }
        (Some(number), None) => {
            let Some(number) = number.as_u64() else {
                return Ok(Outcome::error(
                    "`pull_request` must be a pull request number",
                ));
            };
            match ctx.forge.get_pull_request(&rc.project.id, number).await {
                Ok(pr) if pr.head_sha.is_empty() => {
                    return Ok(Outcome::error(format!(
                        "the forge reports no head commit for pull request #{number}"
                    )));
                }
                Ok(pr) => pr.head_sha,
                Err(e) => return Ok(forge_error(&e)),
            }
        }
        (None, Some(git_ref)) => {
            if git_ref.trim().is_empty() || git_ref.contains("..") {
                return Ok(Outcome::error(
                    "`ref` must be a branch, tag or commit SHA, not empty and without `..`",
                ));
            }
            git_ref
        }
    };
    let status = match ctx.forge.read_ci_status(&rc.project.id, &git_ref).await {
        Ok(status) => status,
        Err(e) => return Ok(forge_error(&e)),
    };
    Ok(structured(&json!({
        "ref": git_ref,
        "state": state_name(status.state),
        "checks": status.checks.iter().map(|c| json!({
            "name": c.name,
            "state": state_name(c.state),
            "url": c.url,
            "job_id": c.job_id,
        })).collect::<Vec<_>>(),
    })))
}

/// One page of a job log without ANSI escape sequences.
pub(super) async fn read_ci_log(
    ctx: &ChannelContext,
    rc: &RunContext,
    args: &Value,
) -> Result<Outcome, ChannelError> {
    let Some(job) = args.get("job").and_then(Value::as_u64) else {
        return Ok(Outcome::error("`job` must be a job id from read_ci_status"));
    };
    let offset = args.get("offset").and_then(Value::as_u64);
    let log = match ctx.forge.read_ci_job_log(&rc.project.id, job).await {
        Ok(log) => log,
        Err(IntegrationError::NotFound(_)) => {
            return Ok(Outcome::error(format!(
                "the log of job {job} is not available: it expired or the job does not exist in \
                 this repository"
            )));
        }
        Err(e) => return Ok(forge_error(&e)),
    };
    let text = strip_ansi(&log.text);
    let page = match page(&text, offset) {
        Ok(page) => page,
        Err(problem) => return Ok(Outcome::error(problem)),
    };
    Ok(structured(&json!({
        "text": page.text,
        "offset": page.offset,
        "total_bytes": text.len(),
        "head_dropped": log.head_dropped,
    })))
}

fn pull_request_json(listed: &ListedPullRequest) -> Value {
    let pr = &listed.pr;
    json!({
        "number": pr.id.number,
        "title": pr.title,
        "head": pr.head,
        "base": pr.base,
        "author": listed.author,
        "labels": listed.labels,
        "draft": pr.draft,
        "head_sha": pr.head_sha,
        "url": pr.url,
    })
}

fn state_name(state: CiState) -> &'static str {
    match state {
        CiState::Success => "success",
        CiState::Pending => "pending",
        CiState::Failure => "failure",
        CiState::None => "none",
    }
}

fn structured(result: &Value) -> Outcome {
    Outcome {
        structured: Some(result.clone()),
        ..Outcome::ok(result.to_string())
    }
}

/// `text` without ANSI escape sequences.
fn strip_ansi(text: &str) -> Cow<'_, str> {
    ANSI.replace_all(text, "")
}

struct Page<'a> {
    text: &'a str,
    /// Byte position of `text` in the log.
    offset: usize,
}

/// The last [`LOG_PAGE_BYTES`] of `text`, or with `offset` the page from there. A page starts at
/// the first character boundary at or after its position and ends at the first one at or after
/// its size, so a character on the edge stays whole in one page and the next page, `offset` plus
/// the page size, doesn't repeat it.
fn page(text: &str, offset: Option<u64>) -> Result<Page<'_>, String> {
    let total = text.len();
    let wanted = match offset {
        None => total.saturating_sub(LOG_PAGE_BYTES),
        Some(offset) => {
            let offset = usize::try_from(offset).unwrap_or(usize::MAX);
            if offset > 0 && offset >= total {
                return Err(format!(
                    "offset {offset} is at or past the end of the log (total_bytes {total})"
                ));
            }
            offset
        }
    };
    let start = text.ceil_char_boundary(wanted);
    let end = text.ceil_char_boundary(wanted.saturating_add(LOG_PAGE_BYTES).min(total));
    Ok(Page {
        text: text.get(start..end).unwrap_or_default(),
        offset: start,
    })
}

#[cfg(test)]
mod tests;
