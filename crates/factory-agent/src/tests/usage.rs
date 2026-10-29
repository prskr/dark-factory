//! Session log usage, rate limits and budgets.

#[test]
fn test_cwd_to_slug() {
    use crate::usage::cwd_to_slug;
    use std::path::Path;

    assert_eq!(cwd_to_slug(Path::new("/data/repo")), "-data-repo");
    assert_eq!(
        cwd_to_slug(Path::new("/Users/baez/sources/code.icb4dc0.de/repo")),
        "-Users-baez-sources-code-icb4dc0-de-repo"
    );
}

#[test]
fn test_parse_session_log_fixture() {
    use crate::usage::parse_session_log;
    use std::io::Cursor;

    let fixture = r#"
{"type":"user","message":{"role":"user","content":"Do the work"}}
{"type":"assistant","message":{"model":"claude-3-5-sonnet","role":"assistant","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}}}
{"type":"tool_use","message":{"role":"assistant","content":"running bash"}}
{"type":"assistant","message":{"model":"claude-3-5-sonnet","role":"assistant","usage":{"input_tokens":80,"output_tokens":40,"cache_read_input_tokens":20,"cache_creation_input_tokens":0}}}
{"type":"assistant","message":{"model":"claude-3-haiku","role":"assistant","usage":{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}
"#;

    let cursor = Cursor::new(fixture);
    let usage = parse_session_log(cursor).expect("parse_session_log");

    assert_eq!(usage.len(), 2);

    let sonnet = usage
        .iter()
        .find(|u| u.model_id == "claude-3-5-sonnet")
        .expect("sonnet");
    assert_eq!(sonnet.input_tokens, 180);
    assert_eq!(sonnet.output_tokens, 90);
    assert_eq!(sonnet.cache_read_tokens, 30);
    assert_eq!(sonnet.cache_write_tokens, 5);

    let haiku = usage
        .iter()
        .find(|u| u.model_id == "claude-3-haiku")
        .expect("haiku");
    assert_eq!(haiku.input_tokens, 200);
    assert_eq!(haiku.output_tokens, 30);
    assert_eq!(haiku.cache_read_tokens, 0);
    assert_eq!(haiku.cache_write_tokens, 0);
}

#[test]
fn test_scenario_adapter_without_usage_session_log_tracker() {
    use crate::usage::SessionLogTracker;
    use df_proto::transcript::v1::UsageSource;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::Path;

    let temp_home = tempfile::tempdir().expect("tempdir");
    let cwd = Path::new("/data/repo");
    let session_id = "sess-12345";

    let mut tracker = SessionLogTracker::new(temp_home.path(), cwd, session_id);
    let session_file = tracker.session_file_path();
    std::fs::create_dir_all(session_file.parent().unwrap()).expect("create_dir_all");

    // Before turn 1: mark turn start
    tracker.mark_turn_start();

    // Turn 1 produces log entry
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&session_file)
        .expect("open session file");
    writeln!(
        file,
        r#"{{"type":"assistant","message":{{"model":"claude-3-5-sonnet","role":"assistant","usage":{{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}}}}}}"#
    )
    .expect("writeln");
    file.flush().expect("flush");

    // Turn 1 ends: parse usage
    let (usage_turn_1, source_1) = tracker.parse_turn_usage();
    assert_eq!(source_1, UsageSource::SessionLog);
    assert_eq!(usage_turn_1.len(), 1);
    assert_eq!(usage_turn_1[0].input_tokens, 100);
    assert_eq!(usage_turn_1[0].output_tokens, 50);

    // Turn 2 begins: mark turn start
    tracker.mark_turn_start();

    // Turn 2 produces another log entry
    writeln!(
        file,
        r#"{{"type":"assistant","message":{{"model":"claude-3-5-sonnet","role":"assistant","usage":{{"input_tokens":150,"output_tokens":70,"cache_read_input_tokens":5,"cache_creation_input_tokens":0}}}}}}"#
    )
    .expect("writeln");
    file.flush().expect("flush");

    // Turn 2 ends: parse usage (must only contain turn 2 metrics!)
    let (usage_turn_2, source_2) = tracker.parse_turn_usage();
    assert_eq!(source_2, UsageSource::SessionLog);
    assert_eq!(usage_turn_2.len(), 1);
    assert_eq!(usage_turn_2[0].input_tokens, 150);
    assert_eq!(usage_turn_2[0].output_tokens, 70);

    // Turn 3 begins: mark turn start
    tracker.mark_turn_start();

    // No logs produced in turn 3
    let (usage_turn_3, source_3) = tracker.parse_turn_usage();
    assert_eq!(source_3, UsageSource::Unspecified);
    assert!(usage_turn_3.is_empty());
}

#[test]
fn test_rate_limit_parsing_and_retry_after() {
    use crate::harness::{RateLimitInfo, extract_retry_after, parse_rate_limit};
    use agent_client_protocol::Error;

    // 1. Extract retry after from text
    assert_eq!(
        extract_retry_after("Rate limit exceeded. Retry after 120 seconds."),
        Some(120)
    );
    assert_eq!(extract_retry_after("retry after 45s"), Some(45));
    assert_eq!(extract_retry_after("Please wait a moment"), None);

    // 2. Parse rate limit from error data JSON
    let err_json = Error::internal_error().data(serde_json::json!({
        "error": "rate_limited",
        "retry_after_seconds": 3600,
        "message": "Quota exhausted"
    }));
    let info = parse_rate_limit(&err_json).expect("should parse rate limit");
    assert_eq!(
        info,
        RateLimitInfo {
            retry_after_seconds: Some(3600),
            detail: "Quota exhausted".to_string(),
        }
    );

    // 3. Parse rate limit from error message
    let err_msg = Error::new(-32603, "Rate limit reached. Retry after 300 seconds.");
    let info_msg = parse_rate_limit(&err_msg).expect("should parse from message");
    assert_eq!(info_msg.retry_after_seconds, Some(300));
    assert!(info_msg.detail.contains("Rate limit reached"));

    // 4. Non-rate-limit error returns None
    let normal_err = Error::invalid_params().data("syntax error");
    assert!(parse_rate_limit(&normal_err).is_none());
}

#[test]
fn test_budget_tracker_constraints() {
    use crate::harness::BudgetTracker;
    use df_proto::agent::v1::Budgets;
    use df_proto::pbjson_types::Duration;

    let budgets = Budgets {
        max_wall_clock: Some(Duration {
            seconds: 1,
            nanos: 0,
        }),
        max_output_tokens: Some(1000),
        max_turns: Some(3),
    };

    let mut tracker = BudgetTracker::new(Some(&budgets));

    assert_eq!(tracker.turns_completed(), 0);
    assert_eq!(tracker.total_output_tokens(), 0);
    assert!(!tracker.is_turn_limit_reached());
    assert!(!tracker.is_output_tokens_exceeded());

    // Record turn 1 with 400 tokens
    tracker.record_turn(400);
    assert_eq!(tracker.turns_completed(), 1);
    assert_eq!(tracker.total_output_tokens(), 400);
    assert!(!tracker.is_turn_limit_reached());
    assert!(!tracker.is_output_tokens_exceeded());

    // Record turn 2 with 700 tokens (cumulative 1100 >= 1000)
    tracker.record_turn(700);
    assert_eq!(tracker.turns_completed(), 2);
    assert_eq!(tracker.total_output_tokens(), 1100);
    assert!(!tracker.is_turn_limit_reached());
    assert!(tracker.is_output_tokens_exceeded());

    // Record turn 3 (cumulative turns 3 >= 3)
    tracker.record_turn(100);
    assert_eq!(tracker.turns_completed(), 3);
    assert!(tracker.is_turn_limit_reached());
}

#[test]
fn test_parse_rate_limit_matches_precise_429() {
    use crate::harness::parse_rate_limit;
    use agent_client_protocol::schema::v1::Error as AcpError;

    // Line 1429 error must NOT be parsed as rate limit
    let err_line = AcpError::internal_error();
    let mut err1 = err_line;
    err1.message = "Syntax error at line 1429: unexpected token".to_string();
    assert!(parse_rate_limit(&err1).is_none());

    // Request ID containing 429 must NOT be parsed as rate limit
    let mut err2 = AcpError::internal_error();
    err2.message = "Failed to fetch resource for request-42981-xyz".to_string();
    assert!(parse_rate_limit(&err2).is_none());

    // Real HTTP 429 must be parsed as rate limit
    let mut err3 = AcpError::internal_error();
    err3.message = "API returned HTTP 429 Too Many Requests. retry-after: 60".to_string();
    let parsed = parse_rate_limit(&err3);
    assert!(parsed.is_some());
    assert_eq!(parsed.unwrap().retry_after_seconds, Some(60));
}

#[test]
fn test_parse_session_log_deduplicates_records() {
    use crate::usage::parse_session_log;

    // Multiple JSONL lines sharing the same message id should only count usage once
    let jsonl = r#"
{"type":"assistant","message":{"id":"msg_1","model":"claude-3-7-sonnet","role":"assistant","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":0}}}
{"type":"assistant","message":{"id":"msg_1","model":"claude-3-7-sonnet","role":"assistant","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":10,"cache_creation_input_tokens":0}}}
{"type":"assistant","message":{"id":"msg_2","model":"claude-3-7-sonnet","role":"assistant","usage":{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":5}}}
"#;

    let usages = parse_session_log(jsonl.as_bytes()).unwrap();
    assert_eq!(usages.len(), 1);
    let u = &usages[0];
    assert_eq!(u.model_id, "claude-3-7-sonnet");
    // input_tokens should be 100 + 200 = 300 (msg_1 counted only once, not twice!)
    assert_eq!(u.input_tokens, 300);
    // output_tokens should be 50 + 30 = 80
    assert_eq!(u.output_tokens, 80);
}

#[test]
fn test_partial_session_log_line_is_read_later() {
    use crate::usage::{SessionLogTracker, cwd_to_slug};
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let cwd = std::path::Path::new("/data/repo");
    let dir = home.join(".claude").join("projects").join(cwd_to_slug(cwd));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("sess.jsonl");
    std::fs::write(&file, "").unwrap();

    let mut tracker = SessionLogTracker::new(&home, cwd, "sess");
    tracker.mark_turn_start();

    let line1 = r#"{"type":"assistant","message":{"id":"m1","model":"m","role":"assistant","usage":{"output_tokens":50}}}"#;
    let line2 = r#"{"type":"assistant","message":{"id":"m2","model":"m","role":"assistant","usage":{"output_tokens":30}}}"#;
    let (first, rest) = line2.split_at(40);

    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap();
    write!(f, "{line1}\n{first}").unwrap();
    let (usage, _) = tracker.parse_turn_usage();
    assert_eq!(usage.iter().map(|u| u.output_tokens).sum::<u64>(), 50);

    writeln!(f, "{rest}").unwrap();
    let (usage, _) = tracker.parse_turn_usage();
    assert_eq!(usage.iter().map(|u| u.output_tokens).sum::<u64>(), 30);
}

#[test]
fn test_rate_limit_text_is_anchored() {
    use crate::harness::parse_rate_limit;
    use agent_client_protocol::schema::v1::Error as AcpError;

    let mut err = AcpError::internal_error();
    err.message = "tool failed: API rate limit doc not found".to_string();
    assert!(parse_rate_limit(&err).is_none());

    err.message = "Rate limit exceeded. Retry after 30 seconds.".to_string();
    assert_eq!(
        parse_rate_limit(&err).unwrap().retry_after_seconds,
        Some(30)
    );
}

#[test]
fn test_late_usage_record_is_counted_after_mark_turn_start() {
    use crate::usage::{SessionLogTracker, cwd_to_slug};
    use std::io::Write;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let cwd = std::path::Path::new("/data/repo");
    let dir = home.join(".claude").join("projects").join(cwd_to_slug(cwd));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("sess.jsonl");
    std::fs::write(&file, "").unwrap();

    let mut tracker = SessionLogTracker::new(&home, cwd, "sess");
    tracker.mark_turn_start();

    let line = r#"{"type":"assistant","message":{"id":"m1","model":"m","role":"assistant","usage":{"output_tokens":40}}}"#;
    let (first, rest) = line.split_at(30);
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap();
    write!(f, "{first}").unwrap();
    assert!(tracker.parse_turn_usage().0.is_empty());

    // The harness finishes the line after the prompt response; the next turn starts
    writeln!(f, "{rest}").unwrap();
    tracker.mark_turn_start();
    let (usage, _) = tracker.parse_turn_usage();
    assert_eq!(usage.iter().map(|u| u.output_tokens).sum::<u64>(), 40);
}

#[test]
fn negative_wall_clock_budget_counts_as_exhausted() {
    use crate::harness::BudgetTracker;
    use df_proto::agent::v1::Budgets;
    use df_proto::pbjson_types::Duration;

    let budgets = Budgets {
        max_wall_clock: Some(Duration {
            seconds: -5,
            nanos: -1,
        }),
        max_output_tokens: None,
        max_turns: None,
    };

    let tracker = BudgetTracker::new(Some(&budgets));

    assert_eq!(
        tracker.remaining_wall_clock(),
        Some(std::time::Duration::ZERO)
    );
}
