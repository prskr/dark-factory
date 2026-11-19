//! Scenarios of the `github-integration` spec for `ci.status` and `ci.job-log`.

use serde_json::{Value, json};

use super::super::Response;
use super::super::tests::{fake, github, pr_ref, reply};
use super::*;

const COMMIT: &str = "https://api.github.com/repos/o/r/commits/feature%2Fx";

fn check_runs(runs: &Value) -> Response {
    reply(200, json!({ "total_count": 1, "check_runs": runs }))
}

fn statuses(statuses: &Value) -> Response {
    reply(200, json!({ "state": "success", "statuses": statuses }))
}

fn run(id: u64, name: &str, status: &str, conclusion: Option<&str>, app: &str) -> Value {
    json!({
        "id": id, "name": name, "status": status, "conclusion": conclusion,
        "html_url": format!("https://github.com/o/r/runs/{id}"), "app": {"slug": app}
    })
}

// Scenario "Actions and an external status".
#[test]
fn actions_check_runs_carry_the_job_id_and_external_statuses_do_not() -> anyhow::Result<()> {
    let fake = fake(vec![
        check_runs(&json!([run(
            99,
            "test",
            "completed",
            Some("failure"),
            "github-actions"
        )])),
        statuses(&json!([
            {"context": "deploy-preview", "state": "success", "target_url": "https://preview"}
        ])),
    ]);

    let status = github(&fake, &[]).ci_status(&pr_ref().repo, "feature/x")?;

    assert_eq!(status.state, CiState::Failure);
    let checks: Vec<_> = status
        .checks
        .iter()
        .map(|c| (c.name.as_str(), c.state, c.job_id, c.url.as_deref()))
        .collect();
    assert_eq!(
        checks,
        [
            (
                "test",
                CiState::Failure,
                Some(99),
                Some("https://github.com/o/r/runs/99")
            ),
            (
                "deploy-preview",
                CiState::Success,
                None,
                Some("https://preview")
            ),
        ]
    );
    let urls: Vec<_> = fake
        .requests
        .borrow()
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(
        urls,
        [
            format!("{COMMIT}/check-runs?filter=latest&per_page=100&page=1"),
            format!("{COMMIT}/status?per_page=100&page=1"),
        ]
    );
    Ok(())
}

#[test]
fn a_run_that_is_not_completed_is_pending_and_other_conclusions_succeed() -> anyhow::Result<()> {
    let fake = fake(vec![
        check_runs(&json!([
            run(1, "build", "in_progress", None, "github-actions"),
            run(2, "lint", "completed", Some("skipped"), "github-actions"),
            run(3, "audit", "completed", Some("neutral"), "other-app"),
        ])),
        statuses(&json!([])),
    ]);

    let status = github(&fake, &[]).ci_status(&pr_ref().repo, "feature/x")?;

    assert_eq!(status.state, CiState::Pending);
    let states: Vec<_> = status.checks.iter().map(|c| c.state).collect();
    assert_eq!(
        states,
        [CiState::Pending, CiState::Success, CiState::Success]
    );
    // Only GitHub Actions runs have job ids.
    let jobs: Vec<_> = status.checks.iter().map(|c| c.job_id).collect();
    assert_eq!(jobs, [Some(1), Some(2), None]);
    Ok(())
}

#[test]
fn failure_like_conclusions_fail_and_a_ref_without_checks_is_none() -> anyhow::Result<()> {
    for conclusion in ["failure", "timed_out", "cancelled", "action_required"] {
        let fake = fake(vec![
            check_runs(&json!([run(
                1,
                "t",
                "completed",
                Some(conclusion),
                "github-actions"
            )])),
            statuses(&json!([])),
        ]);
        let status = github(&fake, &[]).ci_status(&pr_ref().repo, "main")?;
        assert_eq!(status.state, CiState::Failure, "{conclusion}");
    }
    let fake = fake(vec![check_runs(&json!([])), statuses(&json!([]))]);
    let status = github(&fake, &[]).ci_status(&pr_ref().repo, "main")?;
    assert_eq!((status.state, status.checks.len()), (CiState::None, 0));
    Ok(())
}

#[test]
fn an_unknown_ref_is_not_found() {
    let fake = fake(vec![reply(
        422,
        json!({"message": "Validation Failed", "errors": [{"message": "No commit found for SHA: nope"}]}),
    )]);

    let err = github(&fake, &[])
        .ci_status(&pr_ref().repo, "nope")
        .unwrap_err();

    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}

#[test]
fn full_pages_of_check_runs_are_followed() -> anyhow::Result<()> {
    let page: Vec<_> = (0..100)
        .map(|n| {
            run(
                n,
                &format!("c{n}"),
                "completed",
                Some("success"),
                "github-actions",
            )
        })
        .collect();
    let fake = fake(vec![
        check_runs(&json!(page)),
        check_runs(&json!([run(
            500,
            "last",
            "completed",
            Some("success"),
            "x"
        )])),
        statuses(&json!([])),
    ]);

    let status = github(&fake, &[]).ci_status(&pr_ref().repo, "main")?;

    assert_eq!(status.checks.len(), 101);
    assert!(fake.requests.borrow()[1].url.ends_with("page=2"));
    Ok(())
}

fn redirect(location: &str) -> Response {
    Response {
        status: 302,
        location: Some(location.to_string()),
        ..Response::default()
    }
}

fn body(status: u16, body: &[u8]) -> Response {
    Response {
        status,
        body: body.to_vec(),
        ..Response::default()
    }
}

// Scenario "Timestamps removed", and the redirect is followed without credentials.
#[test]
fn the_log_follows_the_redirect_and_loses_bom_and_timestamps() -> anyhow::Result<()> {
    let log = "\u{feff}2026-11-03T10:00:00.1234567Z error[E0425]: cannot find value\n\
               2026-11-03T10:00:01.0000000Z \n\
               no timestamp here\n";
    let fake = fake(vec![
        redirect("https://pipelines.actions.githubusercontent.com/x?sig=1"),
        body(200, log.as_bytes()),
    ]);

    let tail = github(&fake, &[]).job_log(&pr_ref().repo, 812)?;

    assert_eq!(
        tail.text,
        "error[E0425]: cannot find value\n\nno timestamp here\n"
    );
    assert!(!tail.head_dropped);
    let requests = fake.requests.borrow();
    assert_eq!(
        requests[0].url,
        "https://api.github.com/repos/o/r/actions/jobs/812/logs"
    );
    assert_eq!(
        requests[1].url,
        "https://pipelines.actions.githubusercontent.com/x?sig=1"
    );
    assert!(
        requests[1]
            .headers
            .iter()
            .all(|(name, _)| *name != "authorization")
    );
    Ok(())
}

#[test]
fn a_log_over_4_mib_keeps_its_end_and_says_so() -> anyhow::Result<()> {
    let mut big = vec![b'a'; 100];
    big.extend(vec![b'b'; log_tail::MAX_BYTES]);
    let fake = fake(vec![redirect("https://logs.example/x"), body(200, &big)]);

    let tail = github(&fake, &[]).job_log(&pr_ref().repo, 813)?;

    assert!(tail.head_dropped);
    assert_eq!(tail.text.len(), log_tail::MAX_BYTES);
    Ok(())
}

#[test]
fn a_job_that_does_not_exist_or_expired_is_not_found() {
    for status in [404, 410] {
        let fake = fake(vec![reply(status, json!({"message": "Not Found"}))]);
        let err = github(&fake, &[]).job_log(&pr_ref().repo, 1).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "{status}: {err:?}");
    }
}

#[test]
fn a_refusing_log_storage_is_an_upstream_error() {
    let fake = fake(vec![
        redirect("https://logs.example/x"),
        body(403, b"expired"),
    ]);
    let err = github(&fake, &[]).job_log(&pr_ref().repo, 1).unwrap_err();
    assert!(
        matches!(err, Error::Upstream(UpstreamError { status: 403, .. })),
        "{err:?}"
    );
}

#[test]
fn only_leading_rfc_3339_tokens_count_as_timestamps() {
    assert_eq!(strip_timestamp("2026-11-03T10:00:00Z x\n"), "x\n");
    assert_eq!(strip_timestamp("Z x\n"), "Z x\n");
    assert_eq!(strip_timestamp("12:00:00Z x\n"), "12:00:00Z x\n");
    assert_eq!(strip_timestamp("single\n"), "single\n");
}
