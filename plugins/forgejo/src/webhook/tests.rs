//! Scenarios of the `forgejo-integration` spec for `handle-webhook`, on deliveries recorded from
//! Forgejo 15 (`tests/fixtures/webhooks/`).

use super::*;

/// Headers and body of a delivery recorded from Forgejo 15 in `tests/fixtures/webhooks/`.
macro_rules! fixture {
    ($name:literal) => {
        (
            headers(include_str!(concat!(
                "../../tests/fixtures/webhooks/",
                $name,
                ".headers"
            ))),
            include_bytes!(concat!("../../tests/fixtures/webhooks/", $name, ".json")).as_slice(),
        )
    };
}

fn headers(raw: &str) -> Vec<(String, String)> {
    raw.lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.to_string(), value.trim().to_string()))
        .collect()
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

fn parse_fixture((headers, body): (Vec<(String, String)>, &[u8])) -> Vec<String> {
    describe(&parse(&headers, body).expect("parses"))
}

#[test]
fn reviews_map_to_review_submitted_with_their_verdict() {
    for (fixture, actor, verdict) in [
        (fixture!("review_approved"), "alice", "Approved"),
        (
            fixture!("review_request_changes"),
            "alice",
            "ChangesRequested",
        ),
        (fixture!("review_comment"), "alice", "Commented"),
        (fixture!("single_review_comment"), "peter", "Commented"),
    ] {
        assert_eq!(
            parse_fixture(fixture),
            vec![format!(
                "http://localhost:13000/peter/demo review peter/demo#1 head=factory/task/DF-1 \
                 actor={actor} ReviewVerdict::{verdict}"
            )]
        );
    }
}

#[test]
fn closed_pull_requests_carry_the_merged_flag() {
    assert_eq!(
        parse_fixture(fixture!("pull_request_merged")),
        vec![
            "http://localhost:13000/peter/demo closed peter/demo#1 head=factory/task/DF-1 actor=peter merged=true"
        ]
    );
    assert_eq!(
        parse_fixture(fixture!("pull_request_closed")),
        vec![
            "http://localhost:13000/peter/demo closed peter/demo#2 head=factory/task/DF-2 actor=peter merged=false"
        ]
    );
}

#[test]
fn comments_carry_mentions_and_whether_they_are_on_a_pull_request() {
    assert_eq!(
        parse_fixture(fixture!("pull_request_comment_mention")),
        vec![
            "http://localhost:13000/peter/demo comment peter/demo#1 pr=true actor=alice mentions=factory-bot"
        ]
    );
    assert_eq!(
        parse_fixture(fixture!("issue_comment_mention")),
        vec![
            "http://localhost:13000/peter/demo comment peter/demo#3 pr=false actor=alice mentions=factory-bot,peter"
        ]
    );
}

#[test]
fn failed_runs_name_the_branch_workflow_and_run() {
    assert_eq!(
        parse_fixture(fixture!("action_run_failure_push")),
        vec![
            "http://localhost:13000/peter/demo ci-failed peter/demo head=factory/task/DF-3 check=ci.yml url=http://localhost:13000/peter/demo/actions/runs/1"
        ]
    );
    // The run's ref is `#4`; the branch comes from the pull request in the event payload.
    assert_eq!(
        parse_fixture(fixture!("action_run_failure_pull_request")),
        vec![
            "http://localhost:13000/peter/demo ci-failed peter/demo head=factory/task/DF-3 check=ci.yml url=http://localhost:13000/peter/demo/actions/runs/2"
        ]
    );
}

#[test]
fn other_events_and_actions_yield_none() {
    assert!(parse_fixture(fixture!("push")).is_empty());
    assert!(parse_fixture(fixture!("pull_request_opened")).is_empty());
}

/// `body` with the pull request's head repository replaced by `repo`, inside the run's
/// `event_payload` for a run.
fn with_head_repo(body: &[u8], repo: &serde_json::Value) -> Vec<u8> {
    let mut json: serde_json::Value = serde_json::from_slice(body).unwrap();
    if let Some(trigger) = json.pointer("/run/event_payload").and_then(|p| p.as_str()) {
        let trigger = with_head_repo(trigger.as_bytes(), repo);
        json["run"]["event_payload"] = String::from_utf8(trigger).unwrap().into();
    } else {
        json["pull_request"]["head"]["repo"] = repo.clone();
    }
    serde_json::to_vec(&json).unwrap()
}

#[test]
fn pull_request_events_from_a_fork_yield_none() {
    for (headers, body) in [
        fixture!("review_request_changes"),
        fixture!("single_review_comment"),
        fixture!("pull_request_closed"),
        fixture!("pull_request_merged"),
        fixture!("action_run_failure_pull_request"),
    ] {
        // A deleted fork leaves `head.repo` null.
        for repo in [serde_json::json!({"id": 99}), serde_json::Value::Null] {
            let events = parse(&headers, &with_head_repo(body, &repo)).expect("parses");
            assert!(events.is_empty(), "{repo}: {:?}", describe(&events));
        }
    }
}

#[test]
fn a_malformed_payload_or_missing_event_header_is_invalid_input() {
    let (headers, _) = fixture!("pull_request_merged");
    assert!(matches!(
        parse(&headers, b"{}"),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        parse(&[], b"{}"),
        Err(Error::InvalidInput(m)) if m.contains("X-Forgejo-Event")
    ));
}

#[test]
fn the_signature_header_is_hex_decoded() {
    let (headers, _) = fixture!("push");
    let decoded = signature(&headers).expect("signature");
    assert_eq!(decoded.len(), 32);
    assert_eq!(decoded.first(), Some(&0x14));
}

#[test]
fn a_missing_or_malformed_signature_header_is_none() {
    for value in ["", "abc", "zz", "+f", "sha256=00"] {
        let headers = vec![("X-Forgejo-Signature".to_string(), value.to_string())];
        assert_eq!(signature(&headers), None, "{value:?}");
    }
    assert_eq!(signature(&[]), None);
}

#[test]
fn mentions_skip_emails_code_and_duplicates() {
    assert_eq!(mentions("@factory-bot please rebase"), vec!["factory-bot"]);
    assert_eq!(
        mentions("cc @alice, @bob. (@carol)"),
        vec!["alice", "bob", "carol"]
    );
    assert_eq!(mentions("mail alice@example.org"), Vec::<String>::new());
    assert_eq!(
        mentions("`@notamention` and ```\n@fenced\n```"),
        Vec::<String>::new()
    );
    assert_eq!(mentions("@Alice @alice"), vec!["Alice"]);
    assert_eq!(mentions("unclosed ` @bob"), vec!["bob"]);
    assert_eq!(mentions("@ alone"), Vec::<String>::new());
}
