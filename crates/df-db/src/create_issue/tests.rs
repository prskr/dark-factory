use df_core::{PersonaId, ProjectId, TaskId, UserId};
use serde_json::{Value, json};

use super::*;
use crate::requests::{
    ApprovalTypes, Decision, Raise, Raised, RequestState, decide, get_request, raise_approval,
};
use crate::tasks::{NewTask, create_task};

fn validate(payload: &Value) -> Result<Value, RequestError> {
    validate_payload(payload).map(|issue| json!(issue))
}

fn issue(title: &str, body: &str, labels: &[&str]) -> Value {
    json!({"title": title, "body": body, "labels": labels})
}

#[test]
fn validate_stores_the_trimmed_title_and_the_body_and_labels() {
    let stored = validate(&issue(
        "  Flaky login test ",
        "Steps",
        &["bug", " ui ", "bug"],
    ));

    assert_eq!(
        stored.unwrap(),
        issue("Flaky login test", "Steps", &["bug", "ui"])
    );
}

#[test]
fn validate_rejects_a_payload_that_does_not_fit() {
    let long = "x".repeat(MAX_TITLE_CHARS + 1);
    let long_body = "x".repeat(MAX_BODY_CHARS + 1);
    let long_label = "x".repeat(MAX_LABEL_CHARS + 1);
    let many: Vec<String> = (0..=MAX_LABELS).map(|n| format!("l{n}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    for payload in [
        json!({"body": "b"}),
        issue("", "b", &[]),
        issue("   ", "b", &[]),
        issue(&long, "b", &[]),
        issue("two\nlines", "b", &[]),
        issue("t", &long_body, &[]),
        issue("t", "b", &[""]),
        issue("t", "b", &[&long_label]),
        issue("t", "b", &many),
        json!({"title": "t", "body": "b", "labels": "bug"}),
        json!({"title": "t", "body": "b", "labels": [1]}),
        json!({"title": 3, "body": "b"}),
    ] {
        assert!(
            matches!(validate(&payload), Err(RequestError::Invalid(_))),
            "{payload}"
        );
    }
}

#[test]
fn validate_accepts_the_limits() {
    let title = "x".repeat(MAX_TITLE_CHARS);
    let body = "x".repeat(MAX_BODY_CHARS);
    let label = "x".repeat(MAX_LABEL_CHARS);
    let labels: Vec<String> = (0..MAX_LABELS).map(|n| format!("l{n}")).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();

    assert!(validate(&issue(&title, &body, &labels)).is_ok());
    assert!(validate(&issue("t", "", &[&label])).is_ok());
}

#[test]
fn the_fingerprint_ignores_label_order_and_tells_texts_apart() {
    let f = |title, body, labels: &[&str]| CreateIssue.fingerprint(&issue(title, body, labels));

    assert_eq!(f("t", "b", &["a", "b"]), f("t", "b", &["b", "a"]));
    assert_ne!(f("t", "b", &["a"]), f("t", "c", &["a"]));
    assert_ne!(f("ab", "c", &[]), f("a", "bc", &[]));
    assert_ne!(f("t", "b", &["a"]), f("t", "b", &[]));
}

#[test]
fn one_scope_is_offered_and_the_type_is_builtin() {
    assert_eq!(CreateIssue.scopes(), &[Scope::Task]);
    assert!(ApprovalTypes::builtin().get(CREATE_ISSUE).is_some());
}

async fn task(pool: &PgPool) -> TaskRow {
    let (project_id, persona_id) = (ProjectId::new(), PersonaId::new());
    sqlx::query("INSERT INTO projects (id, key, name, repo_url, image, base_branch) VALUES ($1, 'DF', 'P', 'https://x.test/r.git', 'img', 'main')")
        .bind(project_id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    crate::model_providers::tests::seed_provider(pool).await;
    sqlx::query("INSERT INTO personas (id, name, system_prompt, harness, model, model_provider_id) VALUES ($1, 'Linus', 's', 'claude-code', 'test-model', (SELECT id FROM model_providers LIMIT 1))")
        .bind(persona_id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    create_task(
        pool,
        NewTask {
            project_id,
            persona_id,
            title: Some("T".into()),
            description: "d".into(),
            issue_number: None,
            created_by: Principal::system("test"),
            parent_id: None,
            depends_on: Vec::new(),
        },
    )
    .await
    .unwrap()
    .0
}

async fn raise(pool: &PgPool, task: &TaskRow, payload: Value) -> (RequestRow, bool) {
    let actor = Principal::agent(PersonaId::new(), "Linus", None, task.id);
    let raised = raise_approval(
        pool,
        &ApprovalTypes::builtin(),
        Raise {
            task_id: task.id,
            run_id: None,
            actor: &actor,
            blocking: false,
        },
        CREATE_ISSUE,
        payload,
        "the user asked",
    )
    .await
    .unwrap();
    let Raised::Request { request, new } = raised else {
        panic!("not stored")
    };
    (*request, new)
}

async fn decide_as_human(pool: &PgPool, request: &RequestRow, decision: Decision) -> RequestRow {
    decide(
        pool,
        &ApprovalTypes::builtin(),
        request.id,
        &Principal::human(UserId::new(), "Peter"),
        &["board-member".to_string()],
        decision,
    )
    .await
    .unwrap()
}

async fn jobs(pool: &PgPool) -> Vec<Value> {
    sqlx::query_scalar("SELECT payload FROM outbox WHERE kind = $1 ORDER BY id")
        .bind(ISSUE_CREATE_KIND)
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn issue_messages(pool: &PgPool, task_id: TaskId) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT content FROM task_messages WHERE task_id = $1 AND author = $2 ORDER BY queued_at",
    )
    .bind(task_id.as_uuid())
    .bind(serde_json::to_value(Principal::system(ISSUE_CREATED)).unwrap())
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn identical_pending_requests_merge_into_one() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;

    let (first, first_new) = raise(&pool, &task, issue("Flaky", "Body", &["bug", "ui"])).await;
    let (second, second_new) = raise(&pool, &task, issue("Flaky", "Body", &["ui", "bug"])).await;

    assert!(first_new && !second_new);
    assert_eq!(first.id, second.id);
}

#[tokio::test]
async fn the_stored_payload_is_what_was_raised_and_describe_names_the_title() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let (request, _) = raise(&pool, &task, issue(" Flaky ", "Body", &["bug"])).await;

    let stored = get_request(&pool, request.id).await.unwrap().unwrap();

    assert_eq!(
        issue_payload(&stored.payload).unwrap(),
        IssuePayload {
            title: "Flaky".into(),
            body: "Body".into(),
            labels: vec!["bug".into()],
        }
    );
    assert_eq!(CreateIssue.describe(&stored), "Create issue \"Flaky\"");
}

#[tokio::test]
async fn approving_queues_one_job_for_the_request_and_denying_queues_none() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let (approved, _) = raise(&pool, &task, issue("A", "a", &[])).await;
    let (denied, _) = raise(&pool, &task, issue("B", "b", &[])).await;

    decide_as_human(&pool, &denied, Decision::Deny(None)).await;
    assert!(jobs(&pool).await.is_empty());
    let decided = decide_as_human(&pool, &approved, Decision::Approve(Scope::Task)).await;

    assert_eq!(decided.state, RequestState::Approved);
    assert_eq!(jobs(&pool).await, vec![json!({"request_id": approved.id})]);
}

#[tokio::test]
async fn a_scope_other_than_this_request_is_not_offered() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let (request, _) = raise(&pool, &task, issue("A", "a", &[])).await;

    let err = decide(
        &pool,
        &ApprovalTypes::builtin(),
        request.id,
        &Principal::human(UserId::new(), "Peter"),
        &[],
        Decision::Approve(Scope::Project),
    )
    .await
    .unwrap_err();

    assert!(matches!(err, RequestError::ScopeNotOffered(Scope::Project)));
    assert!(jobs(&pool).await.is_empty());
}

#[tokio::test]
async fn the_outcome_is_recorded_once_and_the_task_gets_a_hidden_message() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let (request, _) = raise(&pool, &task, issue("A", "a", &[])).await;
    decide_as_human(&pool, &request, Decision::Approve(Scope::Task)).await;
    let created = IssueOutcome::Created {
        number: 42,
        url: "https://forge.test/o/r/issues/42".into(),
    };

    let first = record_issue_outcome(&pool, request.id, &created)
        .await
        .unwrap();
    let again = record_issue_outcome(&pool, request.id, &created)
        .await
        .unwrap();

    assert!(first && !again);
    let stored = get_request(&pool, request.id).await.unwrap().unwrap();
    assert_eq!(
        created_issue(&stored),
        Some(CreatedIssue {
            number: 42,
            url: "https://forge.test/o/r/issues/42".into(),
        })
    );
    assert_eq!(
        stored.scope(),
        Some(Scope::Task),
        "the decision keeps its scope"
    );
    let messages = issue_messages(&pool, task.id).await;
    assert_eq!(messages.len(), 1, "{messages:?}");
    let text = messages[0].to_string();
    assert!(
        text.contains("Issue #42 created: https://forge.test/o/r/issues/42")
            && text.contains("true"),
        "{text}"
    );
}

#[tokio::test]
async fn a_failure_is_recorded_once_with_the_forge_error() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let (request, _) = raise(&pool, &task, issue("A", "a", &["nope"])).await;
    decide_as_human(&pool, &request, Decision::Approve(Scope::Task)).await;
    let failed = IssueOutcome::Failed {
        error: "the repository has no label `nope`".into(),
    };

    assert!(
        record_issue_outcome(&pool, request.id, &failed)
            .await
            .unwrap()
    );
    assert!(
        !record_issue_outcome(&pool, request.id, &failed)
            .await
            .unwrap()
    );

    let stored = get_request(&pool, request.id).await.unwrap().unwrap();
    assert_eq!(created_issue(&stored), None);
    let messages = issue_messages(&pool, task.id).await;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].to_string().contains("no label `nope`"));
}
