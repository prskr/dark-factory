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
        .push_list_pull_request_feedback_result(Err(IntegrationError::Unauthorized("no".into())));
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

    assert!(read.is_error && message(&read).contains("credentials"));
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

fn encoded_len(feedback: &Feedback) -> usize {
    serde_json::to_string(feedback).unwrap().len()
}

/// `feedback` as the budget returns it after dropping items.
fn truncated(feedback: Feedback) -> Feedback {
    Feedback {
        truncated: true,
        ..feedback
    }
}

#[test]
fn feedback_within_the_budget_is_returned_whole() {
    let feedback = Feedback {
        comments: vec![pr_comment("a", AuthorRole::Member, "2026-11-03T08:00:00Z")],
        ..Feedback::default()
    };

    let result = fit_to_budget(feedback.clone(), FEEDBACK_BUDGET_BYTES).unwrap();

    assert_eq!(result, serde_json::to_value(&feedback).unwrap());
    assert_eq!(result["truncated"], false);
}

#[test]
fn truncation_by_the_plugin_is_reported_within_the_budget() {
    let feedback = Feedback {
        truncated: true,
        ..Feedback::default()
    };

    let result = fit_to_budget(feedback, FEEDBACK_BUDGET_BYTES).unwrap();

    assert_eq!(result["truncated"], true);
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

    let result = fit_to_budget(feedback, encoded_len(&expected)).unwrap();

    assert_eq!(result, serde_json::to_value(&expected).unwrap());
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

    let result = fit_to_budget(feedback, encoded_len(&expected)).unwrap();

    assert_eq!(result, serde_json::to_value(&expected).unwrap());
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

    let result = fit_to_budget(feedback, encoded_len(&expected)).unwrap();

    assert_eq!(result, serde_json::to_value(&expected).unwrap());
}

#[test]
fn the_budget_is_filled_exactly_with_the_newest_items_in_plugin_order() {
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
    // Items newer than minute 5 survive, in the plugin's order.
    let pick = |items: &[PrComment], keep: &[&str]| -> Vec<PrComment> {
        items
            .iter()
            .filter(|c| keep.contains(&c.id.as_str()))
            .cloned()
            .collect()
    };
    let expected = truncated(Feedback {
        comments: pick(&comments, &["c5", "c9"]),
        reviews: reviews
            .iter()
            .filter(|r| ["r8", "r6"].contains(&r.id.as_str()))
            .cloned()
            .collect(),
        threads,
        truncated: true,
    });
    let budget = encoded_len(&expected);

    let fits = fit_to_budget(feedback.clone(), budget).unwrap();
    let one_byte_less = fit_to_budget(feedback, budget - 1).unwrap();

    assert_eq!(fits, serde_json::to_value(&expected).unwrap());
    assert_eq!(fits.to_string().len(), budget);
    assert!(one_byte_less.to_string().len() < budget);
    assert_eq!(one_byte_less["comments"].as_array().unwrap().len(), 1);
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

    let result = fit_to_budget(feedback, FEEDBACK_BUDGET_BYTES).unwrap();

    assert!(result.to_string().len() <= 48 * 1024);
    assert_eq!(result["truncated"], true);
    let ids: Vec<&str> = result["comments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.last(), Some(&"c99"));
    assert!(!ids.contains(&"c0"));
    assert!(ids.len() > 40, "only as many items as needed are dropped");
}
