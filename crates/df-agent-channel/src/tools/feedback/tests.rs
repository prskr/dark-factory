use df_core::testing::ForgeCall;
use df_core::{
    Author, AuthorRole, IntegrationError, PrComment, RateLimit, Review, ReviewComment,
    ReviewThread, ReviewVerdict,
};
use serde_json::json;
use sqlx::PgPool;

use super::*;
use crate::tools::execute;
use crate::tools::tests::{call, message, setup};

const PR: u64 = 7;

fn author(login: &str, role: AuthorRole) -> Author {
    Author {
        login: login.into(),
        role,
    }
}

fn pr_comment(id: &str, role: AuthorRole, at: &str) -> PrComment {
    PrComment {
        id: id.into(),
        author: author("someone", role),
        body: format!("comment {id}"),
        url: format!("https://forge.example.test/pulls/7#{id}"),
        created_at: at.into(),
        updated_at: at.into(),
    }
}

fn review(id: &str, verdict: ReviewVerdict, at: &str) -> Review {
    Review {
        id: id.into(),
        author: author("ada", AuthorRole::Member),
        verdict,
        body: String::new(),
        url: format!("https://forge.example.test/pulls/7#{id}"),
        submitted_at: at.into(),
        updated_at: at.into(),
    }
}

fn review_comment(id: &str, at: &str) -> ReviewComment {
    ReviewComment {
        id: id.into(),
        author: author("ada", AuthorRole::Member),
        body: format!("review comment {id}"),
        url: format!("https://forge.example.test/pulls/7#{id}"),
        created_at: at.into(),
        updated_at: at.into(),
    }
}

fn thread(id: &str, line: u32, resolved: bool, comments: Vec<ReviewComment>) -> ReviewThread {
    ReviewThread {
        id: id.into(),
        path: "src/lib.rs".into(),
        line: Some(line),
        resolved,
        comments,
    }
}

/// Records pull request [`PR`] on the task, as opening the draft PR does.
async fn open_pr(pool: &PgPool, rc: &RunContext) {
    sqlx::query("UPDATE tasks SET pr_number = $2 WHERE id = $1")
        .bind(rc.task.id.as_uuid())
        .bind(i64::try_from(PR).unwrap())
        .execute(pool)
        .await
        .unwrap();
}

fn structured(res: &df_proto::agent::v1::ToolCallResult) -> Value {
    serde_json::from_str(&res.structured_content_json).unwrap()
}

fn read() -> df_proto::agent::v1::ToolCallRequest {
    call("read_pull_request_feedback", json!({}))
}

#[tokio::test]
async fn feedback_tools_before_the_pull_request_is_opened_are_errors() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool).await;

    let read = execute(&env.ctx, &rc, read()).await;
    let comment = execute(
        &env.ctx,
        &rc,
        call("comment_on_pull_request", json!({"body": "x"})),
    )
    .await;

    for res in [read, comment] {
        assert!(res.is_error);
        assert!(
            message(&res).contains("no pull request opened yet"),
            "{}",
            message(&res)
        );
    }
    assert!(env.forge.calls().is_empty());
}

#[tokio::test]
async fn review_with_line_comments_is_returned_with_verdict_threads_and_roles() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;
    let at = "2026-11-03T08:00:00Z";
    env.forge.insert_feedback(
        rc.project.id,
        PR,
        Feedback {
            reviews: vec![review("r1", ReviewVerdict::ChangesRequested, at)],
            threads: vec![
                thread("t1", 3, false, vec![review_comment("c1", at)]),
                thread("t2", 9, false, vec![review_comment("c2", at)]),
            ],
            ..Feedback::default()
        },
    );

    let res = execute(&env.ctx, &rc, read()).await;

    assert!(!res.is_error, "{}", message(&res));
    let result = structured(&res);
    assert_eq!(result["reviews"][0]["verdict"], "changes_requested");
    let threads = result["threads"].as_array().unwrap();
    assert_eq!(threads.len(), 2);
    for t in threads {
        assert_eq!(t["path"], "src/lib.rs");
        assert!(!t["id"].as_str().unwrap().is_empty());
        assert_eq!(t["comments"][0]["author"]["role"], "member");
    }
    assert_eq!(result["truncated"], false);
    let text: Value = serde_json::from_str(&message(&res)).unwrap();
    assert_eq!(text, result, "the text block carries the same JSON");
}

#[tokio::test]
async fn own_comments_and_resolved_threads_are_included() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;
    let at = "2026-11-03T08:00:00Z";
    env.forge.insert_feedback(
        rc.project.id,
        PR,
        Feedback {
            comments: vec![pr_comment("own", AuthorRole::Bot, at)],
            threads: vec![thread("t1", 3, true, vec![review_comment("c1", at)])],
            ..Feedback::default()
        },
    );

    let result = structured(&execute(&env.ctx, &rc, read()).await);

    assert_eq!(result["comments"][0]["author"]["role"], "bot");
    assert_eq!(result["threads"][0]["resolved"], true);
}

#[tokio::test]
async fn outside_comments_are_labelled_outside() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;
    env.forge.insert_feedback(
        rc.project.id,
        PR,
        Feedback {
            comments: vec![pr_comment("x", AuthorRole::Outside, "2026-11-03T08:00:00Z")],
            ..Feedback::default()
        },
    );

    let result = structured(&execute(&env.ctx, &rc, read()).await);

    assert_eq!(result["comments"][0]["author"]["role"], "outside");
}

#[tokio::test]
async fn since_is_passed_to_the_forge_for_the_task_pull_request() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;

    let res = execute(
        &env.ctx,
        &rc,
        call(
            "read_pull_request_feedback",
            json!({"since": "2026-11-03T08:00:00Z"}),
        ),
    )
    .await;

    assert!(!res.is_error, "{}", message(&res));
    assert_eq!(
        env.forge.calls(),
        [ForgeCall::ListPullRequestFeedback {
            project_id: rc.project.id,
            number: PR,
            since: Some("2026-11-03T08:00:00Z".into()),
        }]
    );
}

#[tokio::test]
async fn since_that_is_not_a_timestamp_is_an_error_without_a_forge_call() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;

    let res = execute(
        &env.ctx,
        &rc,
        call("read_pull_request_feedback", json!({"since": "yesterday"})),
    )
    .await;

    assert!(res.is_error);
    assert!(message(&res).contains("since"), "{}", message(&res));
    assert!(env.forge.calls().is_empty());
}

#[tokio::test]
async fn forge_errors_of_the_feedback_tools_are_tool_errors() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;
    env.forge
        .push_list_pull_request_feedback_result(Err(IntegrationError::Unauthorized(
            "GET /pulls: no".into(),
        )));
    env.forge
        .push_add_issue_comment_result(Err(IntegrationError::RateLimited(RateLimit {
            retry_after_seconds: Some(30),
            message: "slow down".into(),
        })));
    env.forge
        .push_reply_to_review_thread_result(Err(IntegrationError::NotFound("thread".into())));

    let read = execute(&env.ctx, &rc, read()).await;
    let comment = execute(
        &env.ctx,
        &rc,
        call("comment_on_pull_request", json!({"body": "x"})),
    )
    .await;
    let reply = execute(
        &env.ctx,
        &rc,
        call(
            "comment_on_pull_request",
            json!({"body": "x", "thread": "t1"}),
        ),
    )
    .await;

    assert!(read.is_error && message(&read).contains("GET /pulls: no"));
    assert!(comment.is_error && message(&comment).contains("retry in 30 seconds"));
    assert!(reply.is_error && message(&reply).contains("not found"));
}

#[tokio::test]
async fn pr_comment_is_an_issue_comment_on_the_pull_request_number() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;

    let res = execute(
        &env.ctx,
        &rc,
        call(
            "comment_on_pull_request",
            json!({"body": "Rebased onto main"}),
        ),
    )
    .await;

    assert!(!res.is_error, "{}", message(&res));
    assert_eq!(
        env.forge.calls(),
        [ForgeCall::AddIssueComment {
            project_id: rc.project.id,
            number: PR,
            body: "Rebased onto main".into(),
        }]
    );
    let result = structured(&res);
    assert_eq!(result["id"], "1");
    assert!(result["url"].as_str().unwrap().starts_with("https://"));
}

#[tokio::test]
async fn thread_reply_goes_to_the_review_thread_of_the_task_pull_request() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;
    env.forge.insert_feedback(
        rc.project.id,
        PR,
        Feedback {
            threads: vec![thread(
                "t1",
                3,
                false,
                vec![review_comment("c1", "2026-11-03T08:00:00Z")],
            )],
            ..Feedback::default()
        },
    );

    let res = execute(
        &env.ctx,
        &rc,
        call(
            "comment_on_pull_request",
            json!({"body": "Done", "thread": "t1"}),
        ),
    )
    .await;

    assert!(!res.is_error, "{}", message(&res));
    assert_eq!(
        env.forge.calls(),
        [ForgeCall::ReplyToReviewThread {
            project_id: rc.project.id,
            number: PR,
            thread: "t1".into(),
            body: "Done".into(),
        }]
    );
    assert!(structured(&res)["url"].is_string());
}

#[tokio::test]
async fn unknown_thread_is_an_error_and_posts_nothing() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;

    let res = execute(
        &env.ctx,
        &rc,
        call(
            "comment_on_pull_request",
            json!({"body": "Done", "thread": "of-another-pr"}),
        ),
    )
    .await;

    assert!(res.is_error);
    assert!(message(&res).contains("of-another-pr"), "{}", message(&res));
    assert!(
        !env.forge
            .calls()
            .iter()
            .any(|c| matches!(c, ForgeCall::AddIssueComment { .. }))
    );
}

#[tokio::test]
async fn repeated_call_id_posts_the_pull_request_comment_once() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;
    let request = call("comment_on_pull_request", json!({"body": "x"}));

    let first = execute(&env.ctx, &rc, request.clone()).await;
    let second = execute(&env.ctx, &rc, request).await;

    assert!(!first.is_error, "{}", message(&first));
    assert_eq!(first, second);
    assert_eq!(env.forge.calls().len(), 1);
}

#[tokio::test]
async fn repeated_call_id_posts_the_thread_reply_once() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;
    env.forge.insert_feedback(
        rc.project.id,
        PR,
        Feedback {
            threads: vec![thread(
                "t1",
                3,
                false,
                vec![review_comment("c1", "2026-11-03T08:00:00Z")],
            )],
            ..Feedback::default()
        },
    );
    let request = call(
        "comment_on_pull_request",
        json!({"body": "Done", "thread": "t1"}),
    );

    let first = execute(&env.ctx, &rc, request.clone()).await;
    let second = execute(&env.ctx, &rc, request).await;

    assert!(!first.is_error, "{}", message(&first));
    assert_eq!(first, second);
    let replies = env
        .forge
        .calls()
        .into_iter()
        .filter(|c| matches!(c, ForgeCall::ReplyToReviewThread { .. }))
        .count();
    assert_eq!(replies, 1);
}

/// `feedback` as the tool returns it: the plugin's result plus the cursor.
fn result_json(feedback: &Feedback, next_since: Option<&str>) -> Value {
    let mut value = serde_json::to_value(feedback).unwrap();
    value["next_since"] = json!(next_since);
    value
}

fn encoded_len(feedback: &Feedback, next_since: Option<&str>) -> usize {
    result_json(feedback, next_since).to_string().len()
}

/// `feedback` as the budget returns it after dropping items.
fn truncated(feedback: Feedback) -> Feedback {
    Feedback {
        truncated: true,
        ..feedback
    }
}

fn ids(result: &Value, list: &str) -> Vec<String> {
    result[list]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn feedback_within_the_budget_is_returned_whole() {
    let feedback = Feedback {
        comments: vec![pr_comment("a", AuthorRole::Member, "2026-11-03T08:00:00Z")],
        ..Feedback::default()
    };

    let result = fit_to_budget(feedback.clone(), None, FEEDBACK_BUDGET_BYTES).unwrap();

    assert_eq!(result, result_json(&feedback, Some("2026-11-03T07:55:00Z")));
    assert_eq!(result["truncated"], false);
}

#[test]
fn truncation_by_the_plugin_is_reported_within_the_budget() {
    let feedback = Feedback {
        truncated: true,
        ..Feedback::default()
    };

    let result = fit_to_budget(feedback, None, FEEDBACK_BUDGET_BYTES).unwrap();

    assert_eq!(result["truncated"], true);
}

#[test]
fn each_list_is_ordered_by_last_activity_newest_first() {
    let mut edited = pr_comment("edited", AuthorRole::Member, "2026-11-01T08:00:00Z");
    edited.updated_at = "2026-11-03T09:00:00Z".into();
    let unedited = pr_comment("unedited", AuthorRole::Member, "2026-11-02T08:00:00Z");
    let mut edited_review = review("edited", ReviewVerdict::Commented, "2026-11-01T08:00:00Z");
    edited_review.updated_at = "2026-11-03T09:00:00Z".into();
    let newer_review = review("newer", ReviewVerdict::Approved, "2026-11-02T08:00:00Z");
    let quiet = thread(
        "quiet",
        1,
        false,
        vec![review_comment("q", "2026-11-02T08:00:00Z")],
    );
    let replied = thread(
        "replied",
        2,
        false,
        vec![
            review_comment("first", "2026-11-01T08:00:00Z"),
            review_comment("reply", "2026-11-03T09:00:00Z"),
        ],
    );
    let feedback = Feedback {
        comments: vec![unedited, edited],
        reviews: vec![newer_review, edited_review],
        threads: vec![quiet, replied],
        truncated: false,
    };

    let result = fit_to_budget(feedback, None, FEEDBACK_BUDGET_BYTES).unwrap();

    assert_eq!(ids(&result, "comments"), ["edited", "unedited"]);
    assert_eq!(ids(&result, "reviews"), ["edited", "newer"]);
    assert_eq!(ids(&result, "threads"), ["replied", "quiet"]);
}

#[test]
fn items_with_the_same_activity_keep_the_plugins_order_and_unparsable_ones_come_last() {
    let at = "2026-11-03T08:00:00Z";
    let feedback = Feedback {
        comments: vec![
            pr_comment("broken", AuthorRole::Member, "yesterday"),
            pr_comment("b", AuthorRole::Member, at),
            pr_comment("a", AuthorRole::Member, at),
        ],
        ..Feedback::default()
    };

    let result = fit_to_budget(feedback, None, FEEDBACK_BUDGET_BYTES).unwrap();

    assert_eq!(ids(&result, "comments"), ["b", "a", "broken"]);
}

#[test]
fn next_since_is_the_newest_activity_minus_five_minutes_in_utc() {
    // 10:00+02:00 is 08:00Z; the later review comment is at 08:20Z.
    let feedback = Feedback {
        comments: vec![pr_comment(
            "c",
            AuthorRole::Member,
            "2026-11-03T10:00:00+02:00",
        )],
        threads: vec![thread(
            "t",
            1,
            false,
            vec![review_comment("r", "2026-11-03T08:20:30Z")],
        )],
        ..Feedback::default()
    };

    let result = fit_to_budget(feedback, None, FEEDBACK_BUDGET_BYTES).unwrap();

    assert_eq!(result["next_since"], "2026-11-03T08:15:30Z");
}

#[test]
fn next_since_never_moves_before_the_given_since() {
    let feedback = Feedback {
        comments: vec![pr_comment("c", AuthorRole::Member, "2026-11-03T09:02:00Z")],
        ..Feedback::default()
    };

    let result = fit_to_budget(
        feedback,
        Some("2026-11-03T09:00:00Z"),
        FEEDBACK_BUDGET_BYTES,
    )
    .unwrap();

    assert_eq!(result["next_since"], "2026-11-03T09:00:00Z");
}

#[test]
fn next_since_keeps_the_fractional_seconds_of_the_given_since() {
    let result = fit_to_budget(
        Feedback::default(),
        Some("2026-11-05T10:15:00.900Z"),
        FEEDBACK_BUDGET_BYTES,
    )
    .unwrap();

    assert_eq!(result["next_since"], "2026-11-05T10:15:00.900Z");
}

#[test]
fn an_empty_result_returns_the_given_since_or_null() {
    let with = fit_to_budget(
        Feedback::default(),
        Some("2026-11-03T09:00:00+01:00"),
        FEEDBACK_BUDGET_BYTES,
    )
    .unwrap();
    let without = fit_to_budget(Feedback::default(), None, FEEDBACK_BUDGET_BYTES).unwrap();

    assert_eq!(with["next_since"], "2026-11-03T08:00:00Z");
    assert_eq!(without["next_since"], Value::Null);
}

#[test]
fn cursor_overlap_covers_the_longest_plugin_call() {
    let max_call = i64::try_from(df_core::MAX_PLUGIN_CALL_TIMEOUT_SECONDS).unwrap();
    assert!(CURSOR_OVERLAP.num_seconds() >= max_call);
}

#[tokio::test]
async fn an_item_created_between_two_plugin_listings_is_returned_by_the_next_read() {
    let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
    let (env, rc) = setup(pool.clone()).await;
    open_pr(&pool, &rc).await;
    // The newest returned review is from 10:15:00Z; a PR comment from 10:14:58Z was missed.
    let newest = review("r", ReviewVerdict::Commented, "2026-11-05T10:15:00Z");
    env.forge
        .push_list_pull_request_feedback_result(Ok(Feedback {
            reviews: vec![newest.clone()],
            ..Feedback::default()
        }));
    env.forge
        .push_list_pull_request_feedback_result(Ok(Feedback {
            comments: vec![pr_comment(
                "late",
                AuthorRole::Member,
                "2026-11-05T10:14:58Z",
            )],
            reviews: vec![newest],
            ..Feedback::default()
        }));

    let first = structured(&execute(&env.ctx, &rc, read()).await);
    assert_eq!(first["next_since"], "2026-11-05T10:10:00Z");
    assert!(ids(&first, "comments").is_empty());
    let since = first["next_since"].as_str().unwrap();
    let second = structured(
        &execute(
            &env.ctx,
            &rc,
            call("read_pull_request_feedback", json!({ "since": since })),
        )
        .await,
    );

    let sent: Vec<_> = env
        .forge
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            ForgeCall::ListPullRequestFeedback { since, .. } => Some(since),
            _ => None,
        })
        .collect();
    assert_eq!(sent, [None, Some("2026-11-05T10:10:00Z".to_string())]);
    assert_eq!(ids(&second, "comments"), ["late"]);
}

#[test]
fn without_a_parsable_timestamp_next_since_is_the_given_since_or_null() {
    let feedback = Feedback {
        comments: vec![pr_comment("c", AuthorRole::Member, "")],
        ..Feedback::default()
    };

    let without = fit_to_budget(feedback.clone(), None, FEEDBACK_BUDGET_BYTES).unwrap();
    let with = fit_to_budget(
        feedback,
        Some("2026-11-03T09:00:00Z"),
        FEEDBACK_BUDGET_BYTES,
    )
    .unwrap();

    assert_eq!(without["next_since"], Value::Null);
    assert_eq!(with["next_since"], "2026-11-03T09:00:00Z");
}

#[test]
fn oversized_feedback_drops_the_oldest_item_first() {
    let comment = pr_comment("old", AuthorRole::Member, "2026-11-03T01:00:00Z");
    let review = review("mid", ReviewVerdict::Commented, "2026-11-03T02:00:00Z");
    let thread = thread(
        "new",
        3,
        false,
        vec![review_comment("c", "2026-11-03T03:00:00Z")],
    );
    let expected = truncated(Feedback {
        reviews: vec![review.clone()],
        threads: vec![thread.clone()],
        ..Feedback::default()
    });
    let feedback = Feedback {
        comments: vec![comment],
        reviews: vec![review],
        threads: vec![thread],
        truncated: false,
    };
    let next_since = Some("2026-11-03T02:55:00Z");

    let result = fit_to_budget(feedback, None, encoded_len(&expected, next_since)).unwrap();

    assert_eq!(result, result_json(&expected, next_since));
}

#[test]
fn threads_are_dropped_whole_by_their_last_activity() {
    let replied_today = thread(
        "replied",
        3,
        false,
        vec![
            review_comment("first", "2026-10-27T08:00:00Z"),
            review_comment("reply", "2026-11-03T08:00:00Z"),
        ],
    );
    let quiet = thread(
        "quiet",
        9,
        false,
        vec![review_comment("only", "2026-11-01T08:00:00Z")],
    );
    let comment = pr_comment("c", AuthorRole::Member, "2026-11-02T08:00:00Z");
    let expected = truncated(Feedback {
        comments: vec![comment.clone()],
        threads: vec![replied_today.clone()],
        ..Feedback::default()
    });
    let feedback = Feedback {
        comments: vec![comment],
        threads: vec![replied_today, quiet],
        ..Feedback::default()
    };
    let next_since = Some("2026-11-03T07:55:00Z");

    let result = fit_to_budget(feedback, None, encoded_len(&expected, next_since)).unwrap();

    assert_eq!(result, result_json(&expected, next_since));
}

#[test]
fn last_activity_compares_instants_not_strings() {
    // 10:00+02:00 is 08:00Z, older than 09:00Z although it sorts after it as a string.
    let older = pr_comment("older", AuthorRole::Member, "2026-11-03T10:00:00+02:00");
    let newer = pr_comment("newer", AuthorRole::Member, "2026-11-03T09:00:00Z");
    let expected = truncated(Feedback {
        comments: vec![newer.clone()],
        ..Feedback::default()
    });
    let feedback = Feedback {
        comments: vec![older, newer],
        ..Feedback::default()
    };
    let next_since = Some("2026-11-03T08:55:00Z");

    let result = fit_to_budget(feedback, None, encoded_len(&expected, next_since)).unwrap();

    assert_eq!(result, result_json(&expected, next_since));
}

#[test]
fn the_budget_is_filled_exactly_with_the_newest_items_newest_first() {
    // Plugin order differs from activity order; mixed kinds exercise the per-list commas.
    let at = |minute: u32| format!("2026-11-03T08:{minute:02}:00Z");
    let comments: Vec<PrComment> = [5, 1, 9, 3]
        .iter()
        .map(|m| pr_comment(&format!("c{m}"), AuthorRole::Member, &at(*m)))
        .collect();
    let reviews: Vec<Review> = [8, 2, 6]
        .iter()
        .map(|m| review(&format!("r{m}"), ReviewVerdict::Commented, &at(*m)))
        .collect();
    let threads = vec![thread("t7", 1, false, vec![review_comment("t7c", &at(7))])];
    let feedback = Feedback {
        comments: comments.clone(),
        reviews: reviews.clone(),
        threads: threads.clone(),
        truncated: false,
    };
    // Items newer than minute 5 survive.
    let pick = |items: &[PrComment], keep: &[&str]| -> Vec<PrComment> {
        keep.iter()
            .filter_map(|id| items.iter().find(|c| c.id == *id).cloned())
            .collect()
    };
    let expected = truncated(Feedback {
        comments: pick(&comments, &["c9", "c5"]),
        reviews: ["r8", "r6"]
            .iter()
            .filter_map(|id| reviews.iter().find(|r| r.id == *id).cloned())
            .collect(),
        threads,
        truncated: true,
    });
    let next_since = Some("2026-11-03T08:04:00Z");
    let budget = encoded_len(&expected, next_since);

    let fits = fit_to_budget(feedback.clone(), None, budget).unwrap();
    let one_byte_less = fit_to_budget(feedback, None, budget - 1).unwrap();

    assert_eq!(fits, result_json(&expected, next_since));
    assert_eq!(fits.to_string().len(), budget);
    assert!(one_byte_less.to_string().len() < budget);
    assert_eq!(one_byte_less["comments"].as_array().unwrap().len(), 1);
}

#[test]
fn old_review_edited_today_survives_the_budget_while_newer_pr_comments_are_dropped() {
    let mut edited = review(
        "edited",
        ReviewVerdict::ChangesRequested,
        "2026-10-27T08:00:00Z",
    );
    edited.updated_at = "2026-11-03T08:00:00Z".into();
    let comments: Vec<PrComment> = ["a", "b"]
        .iter()
        .map(|id| pr_comment(id, AuthorRole::Member, "2026-11-02T08:00:00Z"))
        .collect();
    let expected = truncated(Feedback {
        reviews: vec![edited.clone()],
        ..Feedback::default()
    });
    let feedback = Feedback {
        comments,
        reviews: vec![edited],
        ..Feedback::default()
    };

    let result = fit_to_budget(
        feedback,
        None,
        encoded_len(&expected, Some("2026-11-03T07:55:00Z")),
    )
    .unwrap();

    assert_eq!(ids(&result, "reviews"), ["edited"]);
    assert!(ids(&result, "comments").is_empty());
    assert_eq!(result["truncated"], true);
}

#[test]
fn large_feedback_keeps_the_newest_items_within_48_kib() {
    let comments: Vec<PrComment> = (0..100)
        .map(|i| PrComment {
            body: "x".repeat(1000),
            ..pr_comment(
                &format!("c{i}"),
                AuthorRole::Member,
                &format!("2026-11-03T{:02}:{:02}:00Z", i / 60, i % 60),
            )
        })
        .collect();
    let feedback = Feedback {
        comments,
        ..Feedback::default()
    };

    let result = fit_to_budget(feedback, None, FEEDBACK_BUDGET_BYTES).unwrap();

    assert!(result.to_string().len() <= 48 * 1024);
    assert_eq!(result["truncated"], true);
    let ids = ids(&result, "comments");
    assert_eq!(ids.first().map(String::as_str), Some("c99"));
    assert!(!ids.contains(&"c0".to_string()));
    assert!(ids.len() > 40, "only as many items as needed are dropped");
}
