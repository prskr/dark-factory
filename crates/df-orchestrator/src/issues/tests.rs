//! Tests of the `issue.create` handler against the fake forge (spec `agent-requests`,
//! "Outcome of the issue job").

use df_core::testing::ForgeCall;
use df_core::{PersonaId, Principal, RateLimit, RequestId, UpstreamError, UserId};
use df_db::create_issue::{CreatedIssue, created_issue, issue_error};
use df_db::outbox::Dispatcher;
use df_db::requests::{ApprovalTypes, Decision, Raise, Raised, Scope, decide};
use serde_json::{Value, json};

use super::*;
use crate::testing::Env;

struct Fixture {
    env: Env,
}

impl Fixture {
    async fn new() -> Self {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        Self {
            env: Env::new(pool).await,
        }
    }

    fn handler(&self) -> IssueCreateHandler {
        IssueCreateHandler::new(
            self.env.ctx.pool.clone(),
            Arc::new(self.env.forge.clone()),
            Duration::from_secs(30),
        )
    }

    /// An approved `create-issue` request for "Flaky" with the label `bug`.
    async fn approved(&self) -> RequestId {
        let id = self.pending().await;
        self.decide(id, Decision::Approve(Scope::Task)).await;
        id
    }

    async fn pending(&self) -> RequestId {
        self.pending_with("Details").await
    }

    async fn pending_with(&self, body: &str) -> RequestId {
        let actor = Principal::agent(PersonaId::new(), "Linus", None, self.env.task.id);
        let raised = df_db::requests::raise_approval(
            &self.env.ctx.pool,
            &ApprovalTypes::builtin(),
            Raise {
                task_id: self.env.task.id,
                run_id: None,
                actor: &actor,
                blocking: false,
            },
            CREATE_ISSUE,
            json!({"title": "Flaky", "body": body, "labels": ["bug"]}),
            "the user asked",
        )
        .await
        .unwrap();
        let Raised::Request { request, .. } = raised else {
            panic!("not stored")
        };
        request.id
    }

    async fn decide(&self, id: RequestId, decision: Decision) {
        decide(
            &self.env.ctx.pool,
            &ApprovalTypes::builtin(),
            id,
            &Principal::human(UserId::new(), "Peter"),
            &[],
            decision,
        )
        .await
        .unwrap();
    }

    async fn run(&self, id: RequestId) -> Result<(), JobError> {
        self.handler()
            .handle(&json!(IssueJob { request_id: id }))
            .await
    }

    fn creates(&self) -> Vec<NewIssue> {
        self.env
            .forge
            .calls()
            .into_iter()
            .filter_map(|c| match c {
                ForgeCall::CreateIssue { issue, .. } => Some(issue),
                _ => None,
            })
            .collect()
    }

    async fn issue_messages(&self) -> Vec<Value> {
        sqlx::query_scalar(
            "SELECT content FROM task_messages WHERE task_id = $1 AND author = $2 ORDER BY queued_at",
        )
        .bind(self.env.task.id.as_uuid())
        .bind(serde_json::to_value(Principal::system(df_core::system_reason::ISSUE_CREATED)).unwrap())
        .fetch_all(&self.env.ctx.pool)
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn the_issue_is_created_from_the_stored_payload_and_the_task_is_told_number_and_url() {
    let f = Fixture::new().await;
    let id = f.approved().await;

    f.run(id).await.unwrap();

    assert_eq!(
        f.creates(),
        vec![NewIssue {
            title: "Flaky".into(),
            body: "Details".into(),
            labels: vec!["bug".into()],
        }]
    );
    let request = get_request(&f.env.ctx.pool, id).await.unwrap().unwrap();
    let issue = created_issue(&request).expect("the number is recorded");
    assert_eq!(issue.number, 1);
    let messages = f.issue_messages().await;
    assert_eq!(messages.len(), 1);
    let text = messages[0].to_string();
    assert!(
        text.contains(&format!("Issue #1 created: {}", issue.url)),
        "{text}"
    );
}

#[tokio::test]
async fn a_second_request_with_another_body_leaves_the_approved_issue_unchanged() {
    let f = Fixture::new().await;
    let first = f.approved().await;
    let second = f.pending_with("Another body").await;

    f.run(first).await.unwrap();

    assert_ne!(first, second, "the new text is a separate request");
    assert_eq!(f.creates().len(), 1);
    assert_eq!(f.creates()[0].body, "Details");
    assert_eq!(
        df_db::requests::list_for_task(&f.env.ctx.pool, f.env.task.id)
            .await
            .unwrap()
            .len(),
        2
    );
    let second = get_request(&f.env.ctx.pool, second).await.unwrap().unwrap();
    assert!(second.state.is_pending());
}

#[tokio::test]
async fn the_linked_issue_of_the_task_is_unchanged_after_the_issue_was_created() {
    let f = Fixture::new().await;
    let id = f.approved().await;
    let linked = |f: &Fixture| {
        let (pool, task) = (f.env.ctx.pool.clone(), f.env.task.id);
        async move {
            sqlx::query_scalar::<_, Option<i64>>("SELECT issue_number FROM tasks WHERE id = $1")
                .bind(task.as_uuid())
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    let before = linked(&f).await;

    f.run(id).await.unwrap();

    assert_eq!(f.creates().len(), 1);
    assert_eq!(linked(&f).await, before);
}

#[tokio::test]
async fn the_job_does_not_run_again_once_the_number_is_recorded() {
    let f = Fixture::new().await;
    let id = f.approved().await;

    f.run(id).await.unwrap();
    f.run(id).await.unwrap();

    assert_eq!(f.creates().len(), 1);
    assert_eq!(f.issue_messages().await.len(), 1);
}

#[tokio::test]
async fn rate_limited_and_transport_failures_are_retried_for_up_to_an_hour_until_the_forge_answers()
{
    for failure in [
        IntegrationError::RateLimited(RateLimit {
            retry_after_seconds: Some(1),
            message: "slow down".into(),
        }),
        IntegrationError::Transport("connection refused".into()),
    ] {
        let f = Fixture::new().await;
        let id = f.approved().await;
        f.env.forge.push_create_issue_result(Err(failure.clone()));

        let first = f.run(id).await;

        assert!(matches!(first, Err(JobError::Retry(_))), "{failure}");
        assert!(
            f.issue_messages().await.is_empty(),
            "nothing is reported yet"
        );
        f.run(id).await.unwrap();
        assert_eq!(f.creates().len(), 2, "one failed call and one retry");
        let request = get_request(&f.env.ctx.pool, id).await.unwrap().unwrap();
        assert!(matches!(created_issue(&request), Some(CreatedIssue { .. })));
    }
}

#[tokio::test]
async fn invalid_input_and_unauthorized_are_reported_to_the_task_and_not_retried() {
    for failure in [
        IntegrationError::InvalidInput("the repository has no label `bug`".into()),
        IntegrationError::Unauthorized("token lacks the issues scope".into()),
        IntegrationError::Upstream(UpstreamError {
            status: 422,
            message: "validation failed".into(),
        }),
    ] {
        let f = Fixture::new().await;
        let id = f.approved().await;
        f.env.forge.push_create_issue_result(Err(failure.clone()));

        f.run(id).await.unwrap();
        f.run(id).await.unwrap();

        assert_eq!(f.creates().len(), 1, "{failure}");
        let messages = f.issue_messages().await;
        assert_eq!(messages.len(), 1);
        assert!(messages[0].to_string().contains(&failure.to_string()));
        let request = get_request(&f.env.ctx.pool, id).await.unwrap().unwrap();
        assert_eq!(created_issue(&request), None);
    }
}

#[tokio::test]
async fn a_job_that_gives_up_after_retries_reports_the_last_forge_error_to_the_task() {
    let f = Fixture::new().await;
    let id = f.approved().await;
    f.env
        .forge
        .push_create_issue_result(Err(IntegrationError::Transport("forge down".into())));
    let dispatcher = Dispatcher::new(
        f.env.ctx.pool.clone(),
        f.env.ctx.notifier.clone(),
        vec![Arc::new(f.handler())],
        &opentelemetry::global::meter("test"),
    );

    // The first attempt fails and is retried; the job is then older than the give-up age.
    dispatcher.run_due().await.unwrap();
    assert!(f.issue_messages().await.is_empty(), "retried, not reported");
    sqlx::query(
        "UPDATE outbox SET created_at = now() - interval '2 hours', next_attempt_at = now()",
    )
    .execute(&f.env.ctx.pool)
    .await
    .unwrap();
    dispatcher.run_due().await.unwrap();
    dispatcher.run_due().await.unwrap();

    let messages = f.issue_messages().await;
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].to_string().contains("forge down"),
        "{messages:?}"
    );
    let request = get_request(&f.env.ctx.pool, id).await.unwrap().unwrap();
    assert_eq!(created_issue(&request), None);
    assert!(issue_error(&request).is_some_and(|e| e.contains("forge down")));
    assert_eq!(f.creates().len(), 1, "no attempt after the give-up age");
}

#[tokio::test]
async fn a_denied_or_pending_request_creates_nothing() {
    let f = Fixture::new().await;
    let pending = f.pending().await;
    f.run(pending).await.unwrap();
    f.decide(pending, Decision::Deny(None)).await;

    f.run(pending).await.unwrap();

    assert!(f.creates().is_empty());
}

#[tokio::test]
async fn a_malformed_job_is_given_up() {
    let f = Fixture::new().await;

    let result = f.handler().handle(&json!({"request": 1})).await;

    assert!(matches!(result, Err(JobError::GiveUp(_))));
}
