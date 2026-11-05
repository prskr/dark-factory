//! Scenarios of the `github-integration` spec for `handle-webhook`, on GitHub's documented example
//! payloads (`tests/fixtures/webhooks/`, from `octokit/webhooks`), edited where an example only
//! shows another state.

use serde_json::{Value, json};

use super::*;

macro_rules! example {
    ($name:literal) => {
        serde_json::from_str::<Value>(include_str!(concat!(
            "../../tests/fixtures/webhooks/",
            $name,
            ".json"
        )))
        .expect("example is JSON")
    };
}

const REPO: &str = "https://github.com/Codertocat/Hello-World";

fn deliver(event: &str, payload: &Value) -> Vec<String> {
    let headers = vec![("X-GitHub-Event".to_string(), event.to_string())];
    describe(&parse(&headers, payload.to_string().as_bytes()).expect("parses"))
}

/// One line per event, so a test compares all fields at once.
fn describe(events: &[ParsedEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            let event = match &e.event {
                WebhookEvent::ReviewSubmitted(r) => format!(
                    "review {}/{}#{} head={} actor={} {:?}",
                    r.pr.repo.owner, r.pr.repo.name, r.pr.number, r.head, r.actor, r.verdict
                ),
                WebhookEvent::PullRequestClosed(c) => format!(
                    "closed {}/{}#{} head={} actor={} merged={}",
                    c.pr.repo.owner, c.pr.repo.name, c.pr.number, c.head, c.actor, c.merged
                ),
                WebhookEvent::CiFailed(f) => format!(
                    "ci-failed {}/{} head={} check={} url={}",
                    f.repo.owner, f.repo.name, f.head, f.check, f.url
                ),
                WebhookEvent::CommentCreated(c) => format!(
                    "comment {}/{}#{} pr={} actor={} mentions={}",
                    c.issue.repo.owner,
                    c.issue.repo.name,
                    c.issue.number,
                    c.is_pull_request,
                    c.actor,
                    c.mentions.join(",")
                ),
            };
            format!("{} {event}", e.repo_url)
        })
        .collect()
}

#[test]
fn submitted_reviews_map_to_their_verdict() {
    for (state, verdict) in [
        ("commented", "Commented"),
        ("approved", "Approved"),
        ("changes_requested", "ChangesRequested"),
    ] {
        let mut payload = example!("pull_request_review_submitted");
        payload["review"]["state"] = json!(state);
        assert_eq!(
            deliver("pull_request_review", &payload),
            vec![format!(
                "{REPO} review Codertocat/Hello-World#2 head=changes actor=Codertocat \
                 ReviewVerdict::{verdict}"
            )]
        );
    }
}

#[test]
fn a_dismissed_review_yields_none() {
    let mut payload = example!("pull_request_review_submitted");
    payload["action"] = json!("dismissed");
    payload["review"]["state"] = json!("dismissed");
    assert!(deliver("pull_request_review", &payload).is_empty());
}

#[test]
fn closed_pull_requests_carry_the_merged_flag() {
    let mut payload = example!("pull_request_closed");
    assert_eq!(
        deliver("pull_request", &payload),
        vec![format!(
            "{REPO} closed Codertocat/Hello-World#2 head=changes actor=Codertocat merged=false"
        )]
    );
    payload["pull_request"]["merged"] = json!(true);
    assert_eq!(
        deliver("pull_request", &payload),
        vec![format!(
            "{REPO} closed Codertocat/Hello-World#2 head=changes actor=Codertocat merged=true"
        )]
    );
}

#[test]
fn reviews_and_closes_of_a_fork_pull_request_yield_none() {
    for (event, example) in [
        (
            "pull_request_review",
            example!("pull_request_review_submitted"),
        ),
        ("pull_request", example!("pull_request_closed")),
    ] {
        // A deleted fork leaves `head.repo` null.
        for repo in [json!({"id": 99}), Value::Null] {
            let mut payload = example.clone();
            payload["pull_request"]["head"]["repo"] = repo;
            assert!(deliver(event, &payload).is_empty(), "{event}");
        }
    }
}

#[test]
fn failed_or_timed_out_check_suites_are_ci_failures() {
    for conclusion in ["failure", "timed_out"] {
        let mut payload = example!("check_suite_completed");
        payload["check_suite"]["conclusion"] = json!(conclusion);
        payload["check_suite"]["head_branch"] = json!("factory/task/DF-1");
        assert_eq!(
            deliver("check_suite", &payload),
            vec![format!(
                "{REPO} ci-failed Codertocat/Hello-World head=factory/task/DF-1 \
                 check=octocoders-linter url="
            )]
        );
    }
}

#[test]
fn successful_or_fork_check_suites_yield_none() {
    let mut payload = example!("check_suite_completed");
    assert!(deliver("check_suite", &payload).is_empty());
    payload["check_suite"]["conclusion"] = json!("failure");
    payload["check_suite"]["head_branch"] = Value::Null;
    assert!(deliver("check_suite", &payload).is_empty());
}

#[test]
fn failed_or_errored_statuses_fail_every_branch_at_the_commit() {
    for state in ["failure", "error"] {
        let mut payload = example!("status");
        payload["state"] = json!(state);
        payload["target_url"] = json!("https://ci.example/1");
        payload["branches"] = json!([{ "name": "factory/task/DF-1" }, { "name": "main" }]);
        assert_eq!(
            deliver("status", &payload),
            vec![
                format!(
                    "{REPO} ci-failed Codertocat/Hello-World head=factory/task/DF-1 \
                     check=default url=https://ci.example/1"
                ),
                format!(
                    "{REPO} ci-failed Codertocat/Hello-World head=main check=default \
                     url=https://ci.example/1"
                ),
            ]
        );
    }
    assert!(deliver("status", &example!("status")).is_empty());
}

#[test]
fn comments_on_issues_and_pull_requests_carry_mentions() {
    let mut payload = example!("issue_comment_created");
    payload["comment"]["body"] = json!("@factory-bot please rebase");
    assert_eq!(
        deliver("issue_comment", &payload),
        vec![format!(
            "{REPO} comment Codertocat/Hello-World#1 pr=false actor=Codertocat \
             mentions=factory-bot"
        )]
    );
    payload["issue"]["pull_request"] = json!({ "url": "https://api.github.com/x" });
    assert_eq!(
        deliver("issue_comment", &payload),
        vec![format!(
            "{REPO} comment Codertocat/Hello-World#1 pr=true actor=Codertocat \
             mentions=factory-bot"
        )]
    );
}

#[test]
fn ping_and_other_events_and_actions_yield_none() {
    assert!(deliver("ping", &example!("ping")).is_empty());
    assert!(deliver("pull_request", &example!("pull_request_opened")).is_empty());
    assert!(deliver("issue_comment", &example!("issue_comment_edited")).is_empty());
    assert!(deliver("push", &json!({})).is_empty());
}

#[test]
fn a_malformed_payload_or_missing_event_header_is_invalid_input() {
    let headers = vec![("X-GitHub-Event".to_string(), "pull_request".to_string())];
    assert!(matches!(
        parse(&headers, b"{}"),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        parse(&[], b"{}"),
        Err(Error::InvalidInput(m)) if m.contains("X-GitHub-Event")
    ));
}

#[test]
fn the_sha256_prefix_is_stripped_and_the_rest_hex_decoded() {
    let headers = vec![(
        "X-Hub-Signature-256".to_string(),
        format!("sha256={}", "0a".repeat(32)),
    )];
    assert_eq!(signature(&headers), Some(vec![0x0a; 32]));
}

#[test]
fn a_missing_header_prefix_or_bad_hex_is_none() {
    for value in [
        "",
        "0a0a",
        "sha256=",
        "sha256=zz",
        "sha256=abc",
        "sha1=0a0a",
    ] {
        let headers = vec![("X-Hub-Signature-256".to_string(), value.to_string())];
        assert_eq!(signature(&headers), None, "{value:?}");
    }
    // The legacy SHA-1 header alone is no signature.
    let legacy = vec![("X-Hub-Signature".to_string(), "sha1=0a0a".to_string())];
    assert_eq!(signature(&legacy), None);
}

#[test]
fn mentions_skip_emails_code_teams_and_duplicates() {
    assert_eq!(mentions("@factory-bot please"), vec!["factory-bot"]);
    assert_eq!(
        mentions("cc @alice, @bob. (@carol)"),
        vec!["alice", "bob", "carol"]
    );
    assert_eq!(mentions("mail alice@example.org"), Vec::<String>::new());
    assert_eq!(
        mentions("`@code` and ```\n@fenced\n```"),
        Vec::<String>::new()
    );
    assert_eq!(mentions("@octo-org/reviewers"), Vec::<String>::new());
    assert_eq!(mentions("@Alice @alice"), vec!["Alice"]);
}
