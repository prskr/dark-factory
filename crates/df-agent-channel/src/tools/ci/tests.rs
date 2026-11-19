use std::fmt::Write as _;

use df_core::testing::ForgeCall;
use df_core::{
    CiCheck, CiJobLog, CiStatus, PullRequest, PullRequestRef, PullRequestState, RepoRef,
};

use super::*;
use crate::tools::execute;
use crate::tools::tests::{call, message, setup};

fn structured(res: &df_proto::agent::v1::ToolCallResult) -> Value {
    serde_json::from_str(&res.structured_content_json).unwrap()
}

fn listed(number: u64, head: &str, author: &str, labels: &[&str]) -> ListedPullRequest {
    ListedPullRequest {
        pr: PullRequest {
            id: PullRequestRef {
                repo: RepoRef {
                    owner: "o".into(),
                    name: "r".into(),
                },
                number,
            },
            url: format!("https://forge.example.test/o/r/pulls/{number}"),
            head: head.into(),
            base: "main".into(),
            title: format!("PR {number}"),
            body: String::new(),
            draft: false,
            state: PullRequestState::Open,
            head_sha: format!("sha{number}"),
        },
        author: author.into(),
        labels: labels.iter().map(ToString::to_string).collect(),
    }
}

fn failing_status() -> CiStatus {
    CiStatus {
        state: CiState::Failure,
        checks: vec![
            CiCheck {
                name: "ci / test".into(),
                state: CiState::Failure,
                url: Some("https://forge.example.test/runs/1".into()),
                job_id: Some(812),
            },
            CiCheck {
                name: "ci / lint".into(),
                state: CiState::Success,
                url: None,
                job_id: None,
            },
        ],
    }
}

// Scenario "Renovate survey": filters reach the forge and the result is ordered by number.
#[tokio::test]
async fn list_pull_requests_passes_the_filters_and_orders_by_number() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    env.forge.push_list_pull_requests_result(Ok(vec![
        listed(41, "renovate/b", "renovate-bot", &[]),
        listed(40, "renovate/a", "renovate-bot", &["major"]),
    ]));

    let res = execute(
        &env.ctx,
        &rc,
        call(
            "list_pull_requests",
            json!({"head_prefix": "renovate/", "author": "renovate-bot"}),
        ),
    )
    .await;

    assert!(!res.is_error, "{}", message(&res));
    let result = structured(&res);
    let numbers: Vec<_> = result["pull_requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["number"].as_u64().unwrap())
        .collect();
    assert_eq!(numbers, [40, 41]);
    assert_eq!(result["truncated"], false);
    assert_eq!(
        result["pull_requests"][0],
        json!({
            "number": 40, "title": "PR 40", "head": "renovate/a", "base": "main",
            "author": "renovate-bot", "labels": ["major"], "draft": false,
            "head_sha": "sha40", "url": "https://forge.example.test/o/r/pulls/40"
        })
    );
    let calls = env.forge.calls();
    assert!(matches!(
        calls.as_slice(),
        [ForgeCall::ListPullRequests { filter, .. }]
            if filter.head_prefix.as_deref() == Some("renovate/")
                && filter.author.as_deref() == Some("renovate-bot")
                && filter.label.is_none() && filter.base.is_none()
    ));
}

// Scenario "No filter".
#[tokio::test]
async fn list_pull_requests_without_arguments_has_no_filter() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    env.forge
        .push_list_pull_requests_result(Ok(vec![listed(44, "fix/login", "alice", &[])]));

    let res = execute(&env.ctx, &rc, call("list_pull_requests", json!({}))).await;

    assert_eq!(
        structured(&res)["pull_requests"].as_array().unwrap().len(),
        1
    );
    assert!(matches!(
        env.forge.calls().as_slice(),
        [ForgeCall::ListPullRequests { filter, .. }] if *filter == PullRequestFilter::default()
    ));
}

// Scenario "Large backlog".
#[tokio::test]
async fn list_pull_requests_keeps_100_and_says_when_more_match() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    let many = (1..=130).rev().map(|n| listed(n, "h", "a", &[])).collect();
    env.forge.push_list_pull_requests_result(Ok(many));

    let res = execute(&env.ctx, &rc, call("list_pull_requests", json!({}))).await;

    let result = structured(&res);
    let prs = result["pull_requests"].as_array().unwrap();
    assert_eq!(prs.len(), 100);
    assert_eq!(prs[99]["number"], 100);
    assert_eq!(result["truncated"], true);
}

// Scenario "Failing PR": the status of the pull request's head SHA.
#[tokio::test]
async fn read_ci_status_of_a_pull_request_reads_its_head_commit() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    env.forge
        .insert_pull_request(rc.project.id, listed(40, "renovate/a", "bot", &[]).pr);
    env.forge.push_read_ci_status_result(Ok(failing_status()));

    let res = execute(
        &env.ctx,
        &rc,
        call("read_ci_status", json!({"pull_request": 40})),
    )
    .await;

    assert!(!res.is_error, "{}", message(&res));
    let result = structured(&res);
    assert_eq!(result["state"], "failure");
    assert_eq!(result["ref"], "sha40");
    assert_eq!(result["checks"][0]["name"], "ci / test");
    assert_eq!(result["checks"][0]["state"], "failure");
    assert_eq!(result["checks"][0]["job_id"], 812);
    assert_eq!(result["checks"][1]["state"], "success");
    assert!(result["checks"][1]["job_id"].is_null());
    assert!(matches!(
        env.forge.calls().last(),
        Some(ForgeCall::ReadCiStatus { git_ref, .. }) if git_ref == "sha40"
    ));
}

// Scenario "No CI".
#[tokio::test]
async fn read_ci_status_of_a_ref_without_checks_is_none() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;

    let res = execute(
        &env.ctx,
        &rc,
        call("read_ci_status", json!({"ref": "main"})),
    )
    .await;

    let result = structured(&res);
    assert_eq!(result["state"], "none");
    assert_eq!(result["checks"], json!([]));
    assert!(matches!(
        env.forge.calls().as_slice(),
        [ForgeCall::ReadCiStatus { git_ref, .. }] if git_ref == "main"
    ));
}

// Scenario "Both arguments", and the other invalid ones: the forge is not called.
#[tokio::test]
async fn read_ci_status_needs_exactly_one_valid_argument_before_it_calls_the_forge() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    for args in [
        json!({"pull_request": 40, "ref": "main"}),
        json!({}),
        json!({"ref": "a..b"}),
        json!({"ref": "  "}),
    ] {
        let res = execute(&env.ctx, &rc, call("read_ci_status", args.clone())).await;
        assert!(res.is_error, "{args}");
    }
    assert!(env.forge.calls().is_empty());
}

// Scenario "Unknown job"-like: a pull request that does not exist in the repository.
#[tokio::test]
async fn read_ci_status_of_an_unknown_pull_request_is_an_error() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;

    let res = execute(
        &env.ctx,
        &rc,
        call("read_ci_status", json!({"pull_request": 999})),
    )
    .await;

    assert!(res.is_error);
    assert!(message(&res).contains("not found"), "{}", message(&res));
}

fn log_of(text: &str, head_dropped: bool) -> CiJobLog {
    CiJobLog {
        text: text.into(),
        head_dropped,
    }
}

/// 300 KiB: 5 999 numbered lines of 51 bytes, the failure at the end, with colour codes.
fn long_log() -> String {
    let mut log = String::new();
    for n in 0..5_999 {
        writeln!(log, "\x1b[32mline {n:>41}\x1b[0m").unwrap();
    }
    log.push_str("\x1b[31merror: boom\x1b[0m\n");
    log
}

// Scenario "Tail of a failed job".
#[tokio::test]
async fn read_ci_log_returns_the_last_64_kib_without_escape_sequences() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    env.forge
        .push_read_ci_job_log_result(Ok(log_of(&long_log(), false)));

    let res = execute(&env.ctx, &rc, call("read_ci_log", json!({"job": 812}))).await;

    assert!(!res.is_error, "{}", message(&res));
    let result = structured(&res);
    let text = result["text"].as_str().unwrap();
    let total = result["total_bytes"].as_u64().unwrap();
    assert!(total > 250 * 1024, "{total}");
    assert!(text.len() <= LOG_PAGE_BYTES + 3 && text.len() > LOG_PAGE_BYTES - 100);
    assert!(text.ends_with("error: boom\n"));
    assert!(!text.contains('\x1b'));
    assert_eq!(
        result["offset"].as_u64().unwrap() + text.len() as u64,
        total
    );
    assert_eq!(result["head_dropped"], false);
    assert!(matches!(
        env.forge.calls().as_slice(),
        [ForgeCall::ReadCiJobLog { job_id: 812, .. }]
    ));
}

// Scenario "Earlier page".
#[tokio::test]
async fn read_ci_log_with_offset_zero_returns_the_first_page() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    env.forge
        .push_read_ci_job_log_result(Ok(log_of(&long_log(), false)));

    let res = execute(
        &env.ctx,
        &rc,
        call("read_ci_log", json!({"job": 812, "offset": 0})),
    )
    .await;

    let result = structured(&res);
    let text = result["text"].as_str().unwrap();
    assert_eq!(result["offset"], 0);
    assert!(text.starts_with(&format!("line {:>41}\n", 0)));
    assert_eq!(text.len(), LOG_PAGE_BYTES);
}

// Scenario "Log no longer available" and "Unknown job".
#[tokio::test]
async fn read_ci_log_of_an_expired_or_foreign_job_is_an_error_without_content() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    env.forge
        .push_read_ci_job_log_result(Err(IntegrationError::NotFound("job 812".into())));

    let res = execute(&env.ctx, &rc, call("read_ci_log", json!({"job": 812}))).await;

    assert!(res.is_error);
    assert!(message(&res).contains("not available"), "{}", message(&res));
    assert!(res.structured_content_json.is_empty());
}

// Scenarios "Huge log" and "Old Forgejo".
#[tokio::test]
async fn read_ci_log_reports_a_dropped_head_and_unsupported_forges() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    env.forge
        .push_read_ci_job_log_result(Ok(log_of("tail\n", true)));
    env.forge
        .push_read_ci_job_log_result(Err(IntegrationError::Unsupported(
            "job logs need Forgejo v16 or later".into(),
        )));

    let huge = execute(&env.ctx, &rc, call("read_ci_log", json!({"job": 813}))).await;
    let old = execute(&env.ctx, &rc, call("read_ci_log", json!({"job": 814}))).await;

    let result = structured(&huge);
    assert_eq!(
        (
            result["head_dropped"].clone(),
            result["total_bytes"].clone()
        ),
        (json!(true), json!(5))
    );
    assert!(old.is_error);
    assert!(
        message(&old).contains("Forgejo v16 or later"),
        "{}",
        message(&old)
    );
}

// Scenario "CI reads need forge support": a client without the capability fails the call only.
#[tokio::test]
async fn a_forge_client_without_ci_fails_the_call_and_not_the_run() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;
    env.forge
        .push_read_ci_status_result(Err(IntegrationError::Unsupported("no CI".to_string())));

    let unsupported = execute(
        &env.ctx,
        &rc,
        call("read_ci_status", json!({"ref": "main"})),
    )
    .await;
    let next = execute(
        &env.ctx,
        &rc,
        call("read_ci_status", json!({"ref": "main"})),
    )
    .await;

    assert!(unsupported.is_error);
    assert!(
        message(&unsupported).contains("not supported by this forge"),
        "{}",
        message(&unsupported)
    );
    assert!(!next.is_error, "{}", message(&next));
}

#[test]
fn ansi_csi_osc_and_a_lone_escape_are_removed() {
    assert_eq!(strip_ansi("\x1b[1;31mred\x1b[0m plain"), "red plain");
    assert_eq!(
        strip_ansi("see \x1b]8;;https://x.test\x1b\\link\x1b]8;;\x1b\\ and \x1b]0;title\x07done"),
        "see link and done"
    );
    assert_eq!(strip_ansi("a\x1b"), "a");
    assert_eq!(strip_ansi("no escapes é"), "no escapes é");
}

#[test]
fn a_character_on_a_page_edge_stays_whole_and_the_next_page_does_not_repeat_it() {
    // Edge at byte LOG_PAGE_BYTES splits the 2-byte `é` that starts one byte earlier.
    let text = format!("{}é{}", "a".repeat(LOG_PAGE_BYTES - 1), "b".repeat(10));

    let first = page(&text, Some(0)).unwrap();
    let second = page(&text, Some(LOG_PAGE_BYTES as u64)).unwrap();

    assert!(first.text.ends_with('é'));
    assert_eq!(first.text.len(), LOG_PAGE_BYTES + 1);
    assert_eq!(second.text, "b".repeat(10));
    assert_eq!(second.offset, LOG_PAGE_BYTES + 1);
}

#[test]
fn the_tail_page_starts_on_a_character_boundary() {
    let text = format!("é{}", "a".repeat(LOG_PAGE_BYTES - 1));
    // The last 64 KiB would start inside `é`.
    let tail = page(&text, None).unwrap();
    assert_eq!(tail.offset, 2);
    assert_eq!(tail.text, "a".repeat(LOG_PAGE_BYTES - 1));
}

#[test]
fn an_offset_past_the_end_is_an_error_and_an_empty_log_has_an_empty_page() {
    assert!(page("abc", Some(3)).is_err());
    assert!(page("abc", Some(100)).is_err());
    let empty = page("", Some(0)).unwrap();
    assert_eq!((empty.text, empty.offset), ("", 0));
}
