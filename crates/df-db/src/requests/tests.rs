use super::*;
use crate::tasks::{NewTask, create_task};
use df_core::{PersonaId, ProjectId, UserId};

#[test]
fn every_request_state_parses_from_its_name() {
    for state in [
        RequestState::Pending,
        RequestState::Answered,
        RequestState::Approved,
        RequestState::Denied,
        RequestState::Closed,
    ] {
        assert_eq!(state.as_str().parse::<RequestState>().unwrap(), state);
    }
}

#[test]
fn only_the_pending_state_is_pending() {
    assert!(RequestState::Pending.is_pending());
    for state in [
        RequestState::Answered,
        RequestState::Approved,
        RequestState::Denied,
        RequestState::Closed,
    ] {
        assert!(!state.is_pending(), "{state:?}");
    }
}

#[test]
fn an_unknown_request_state_is_an_error_naming_it() {
    let error = "reopened"
        .parse::<RequestState>()
        .expect_err("unknown state");

    assert_eq!(
        error.to_string(),
        "invalid request: unknown request state `reopened`"
    );
}

fn raise_by(task_id: TaskId, actor: &Principal, blocking: bool) -> Raise<'_> {
    Raise {
        task_id,
        run_id: None,
        actor,
        blocking,
    }
}

struct FakeType;

#[async_trait::async_trait]
impl ApprovalType for FakeType {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn validate(&self, _task: &TaskRow, payload: &Value) -> Result<Value, RequestError> {
        payload
            .get("what")
            .and_then(Value::as_str)
            .map(|_| payload.clone())
            .ok_or_else(|| RequestError::Invalid("`what` is required".into()))
    }
    fn fingerprint(&self, payload: &Value) -> String {
        payload["what"].as_str().unwrap_or_default().to_string()
    }
    fn scopes(&self) -> &'static [Scope] {
        &[Scope::Task, Scope::Project]
    }
    async fn apply(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: &RequestRow,
        _scope: Scope,
        _decider: Actor<'_>,
    ) -> Result<(), RequestError> {
        if request.payload["what"] == "fail" {
            return Err(RequestError::ApplyFailed("nope".into()));
        }
        sqlx::query("INSERT INTO audit_log (actor, roles, action, resource_type, resource_id, details) VALUES ('{}', '{}', 'fake.applied', 'request', $1, '{}')")
            .bind(request.id.to_string())
            .execute(&mut **tx)
            .await?;
        Ok(())
    }
    fn describe(&self, request: &RequestRow) -> String {
        format!("fake {}", request.payload["what"])
    }
}

fn question(value: Value) -> QuestionSpec {
    QuestionSpec::parse(serde_json::from_value(value).unwrap()).unwrap()
}

fn single_choice(text: &str) -> QuestionSpec {
    question(json!({
        "text": text, "kind": "single_choice",
        "options": [
            {"id": "pg16", "label": "16"},
            {"id": "pg17", "label": "17", "description": "newest"},
        ],
    }))
}

fn multi_choice(text: &str) -> QuestionSpec {
    question(json!({
        "text": text, "kind": "multi_choice", "max_selected": 2,
        "options": [{"id": "api", "label": "API"}, {"id": "ui", "label": "UI"}, {"id": "cli", "label": "CLI"}],
    }))
}

fn choice(selected: &[&str], other: Option<&str>) -> QuestionAnswer {
    QuestionAnswer::Choice {
        selected: selected.iter().map(ToString::to_string).collect(),
        other: other.map(ToString::to_string),
    }
}

/// Answers `spec` as a board member and returns the decided request and its decision message.
async fn answered_message(spec: QuestionSpec, answer: QuestionAnswer) -> (RequestRow, String) {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let (q, _) = raise_question(&pool, raise_by(task.id, &agent(&task), true), spec)
        .await
        .unwrap();
    let decided = decide(
        &pool,
        &types(),
        q.id,
        &human(),
        &[],
        Decision::Answer(answer),
    )
    .await
    .unwrap();
    let message = decision_message(None, &decided);
    (decided, message)
}

/// The JSON block of a decision message, parsed.
fn json_block(message: &str) -> Value {
    let body = message.split("```json\n").nth(1).expect("a json block");
    serde_json::from_str(body.split("\n```").next().unwrap()).expect("valid json")
}

fn types() -> ApprovalTypes {
    ApprovalTypes::default().with(Arc::new(FakeType))
}

async fn task(pool: &PgPool) -> TaskRow {
    let (project_id, persona_id) = (ProjectId::new(), PersonaId::new());
    sqlx::query("INSERT INTO projects (id, key, name, repo_url, image, base_branch) VALUES ($1, 'DF', 'P', 'https://x.test/r.git', 'img', 'main')")
        .bind(project_id.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    crate::model_providers::tests::seed_provider(pool).await;
    sqlx::query("INSERT INTO personas (id, display_name, handle, commit_email, system_prompt, harness, model, model_provider_id) VALUES ($1, 'Linus', 'linus', 'linus@example.org', 's', 'claude-code', 'test-model', (SELECT id FROM model_providers LIMIT 1))")
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

fn agent(task: &TaskRow) -> Principal {
    Principal::agent(PersonaId::new(), "Linus", None, task.id)
}

fn human() -> Principal {
    Principal::human(UserId::new(), "Peter")
}

async fn audit_count(pool: &PgPool, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = $1")
        .bind(action)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The newest message of the task is the system's decision notice, made only of hidden blocks.
async fn assert_hidden_decision_message(pool: &PgPool, task_id: TaskId) {
    let (author, content): (Value, Value) = sqlx::query_as(
        "SELECT author, content FROM task_messages WHERE task_id = $1 ORDER BY queued_at DESC LIMIT 1",
    )
    .bind(task_id.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(author, json!(Principal::system(REQUEST_DECIDED)));
    let blocks = content.as_array().unwrap();
    assert!(!blocks.is_empty());
    assert!(blocks.iter().all(|block| block["hidden"] == true));
}

#[tokio::test]
async fn blocked_is_an_accepted_task_status() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    sqlx::query("UPDATE tasks SET status = 'blocked' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE tasks SET status = 'bogus' WHERE id = $1")
            .bind(task.id.as_uuid())
            .execute(&pool)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn question_is_stored_and_identical_ones_merge() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let (first, created) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), false),
        single_choice("Use Postgres 16 or 17?"),
    )
    .await
    .unwrap();
    assert!(created);
    assert_eq!(first.state, RequestState::Pending);
    assert_eq!(first.payload["kind"], "single_choice");
    assert_eq!(
        first.payload["options"],
        json!([{"id": "pg16", "label": "16"}, {"id": "pg17", "label": "17", "description": "newest"}])
    );
    assert_eq!(first.payload["allow_other"], true);
    assert_eq!(first.question(), single_choice("Use Postgres 16 or 17?"));
    let (second, created) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), true),
        QuestionSpec::text_question("  use postgres 16   or 17? "),
    )
    .await
    .unwrap();
    assert!(!created);
    assert_eq!(second.id, first.id);
    assert_eq!(second.question().kind, QuestionKind::SingleChoice);
    assert_eq!(pending_count(&pool).await.unwrap(), 1);
    assert_eq!(audit_count(&pool, "request.raise").await, 1);
}

#[tokio::test]
async fn unknown_type_and_invalid_payload_store_nothing() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let err = raise_approval(
        &pool,
        &types(),
        raise_by(task.id, &agent(&task), false),
        "delete-everything",
        json!({}),
        "r",
    )
    .await
    .unwrap_err();
    assert!(matches!(err, RequestError::UnknownType(_)));
    let err = raise_approval(
        &pool,
        &types(),
        raise_by(task.id, &agent(&task), false),
        "fake",
        json!({}),
        "r",
    )
    .await
    .unwrap_err();
    assert!(matches!(err, RequestError::Invalid(_)));
    assert_eq!(pending_count(&pool).await.unwrap(), 0);
}

#[tokio::test]
async fn request_flood_is_limited_but_merging_still_works() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let actor = agent(&task);
    for i in 0..MAX_PENDING_PER_TASK {
        raise_question(
            &pool,
            raise_by(task.id, &actor, false),
            QuestionSpec::text_question(format!("q{i}")),
        )
        .await
        .unwrap();
    }
    let err = raise_question(
        &pool,
        raise_by(task.id, &actor, false),
        QuestionSpec::text_question("one more"),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, RequestError::TooManyPending));
    let (_, created) = raise_question(
        &pool,
        raise_by(task.id, &actor, false),
        QuestionSpec::text_question("q0"),
    )
    .await
    .unwrap();
    assert!(!created);
    assert_eq!(pending_count(&pool).await.unwrap(), MAX_PENDING_PER_TASK);
}

#[tokio::test]
async fn answer_is_stored_audited_and_queued_as_a_hidden_message() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    sqlx::query("UPDATE tasks SET status = 'blocked' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let (q, _) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), true),
        single_choice("Which?"),
    )
    .await
    .unwrap();
    let peter = human();
    let decided = decide(
        &pool,
        &types(),
        q.id,
        &peter,
        &["board-member".into()],
        Decision::Answer(choice(&["pg17"], None)),
    )
    .await
    .unwrap();
    assert_eq!(decided.state, RequestState::Answered);
    assert_eq!(
        decided.decision,
        Some(json!({"answer": {"selected": ["pg17"], "other": null}}))
    );
    assert_eq!(decided.answer(), Some(choice(&["pg17"], None)));
    assert_eq!(decided.decided_by, Some(peter));
    assert_eq!(audit_count(&pool, "request.answer").await, 1);
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "queued");
    assert_hidden_decision_message(&pool, task.id).await;
    let again = decide(
        &pool,
        &types(),
        q.id,
        &human(),
        &[],
        Decision::Answer(choice(&["pg16"], None)),
    )
    .await
    .unwrap_err();
    assert!(matches!(again, RequestError::AlreadyDecided));
}

#[tokio::test]
async fn an_invalid_answer_is_rejected_and_stores_nothing() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let (q, _) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), true),
        single_choice("Which?"),
    )
    .await
    .unwrap();
    let messages_before = message_count(&pool, task.id).await;

    let err = decide(
        &pool,
        &types(),
        q.id,
        &human(),
        &[],
        Decision::Answer(choice(&["pg16", "pg17"], None)),
    )
    .await
    .unwrap_err();

    let RequestError::InvalidAnswer(error) = err else {
        panic!("expected InvalidAnswer, got {err:?}");
    };
    assert_eq!(error.message, "Choose one option.");
    let stored = get_request(&pool, q.id).await.unwrap().unwrap();
    assert_eq!(stored.state, RequestState::Pending);
    assert_eq!(stored.decision, None);
    assert_eq!(audit_count(&pool, "request.answer").await, 0);
    assert_eq!(message_count(&pool, task.id).await, messages_before);
}

async fn message_count(pool: &PgPool, task_id: TaskId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM task_messages WHERE task_id = $1")
        .bind(task_id.as_uuid())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_text_answer_is_stored_trimmed_and_read_back() {
    let (decided, _) = answered_message(
        QuestionSpec::text_question("Why?"),
        QuestionAnswer::Text("  Use the **old** endpoint  ".into()),
    )
    .await;

    assert_eq!(
        decided.decision,
        Some(json!({"answer": {"text": "Use the **old** endpoint"}}))
    );
    assert_eq!(
        decided.answer(),
        Some(QuestionAnswer::Text("Use the **old** endpoint".into()))
    );
}

#[tokio::test]
async fn the_decision_message_of_a_single_choice_holds_the_json_before_the_answer() {
    let (decided, message) =
        answered_message(single_choice("Which?"), choice(&["pg17"], None)).await;

    assert!(
        message.starts_with(&format!(
            "Request {} (your question \"Which?\") was answered by Peter. Structured answer:\n\n```json\n",
            decided.id
        )),
        "{message}"
    );
    assert_eq!(
        json_block(&message),
        json!({
            "request_id": decided.id,
            "kind": "single_choice",
            "selected": [{"id": "pg17", "label": "17"}],
            "other": null,
        })
    );
    assert!(message.ends_with("```\n\nAnswer: 17"), "{message}");
}

#[tokio::test]
async fn the_decision_message_of_a_single_choice_with_other_has_no_selected_option() {
    let (_, message) = answered_message(
        single_choice("Which?"),
        choice(&[], Some("Neither, use 15")),
    )
    .await;

    let block = json_block(&message);
    assert_eq!(block["selected"], json!([]));
    assert_eq!(block["other"], "Neither, use 15");
    assert!(
        message.ends_with("Answer: Other: Neither, use 15"),
        "{message}"
    );
}

#[tokio::test]
async fn the_decision_message_of_a_multiple_choice_lists_options_in_question_order_and_other() {
    let (_, message) =
        answered_message(multi_choice("Which parts?"), choice(&["ui", "api"], None)).await;
    assert_eq!(
        json_block(&message)["selected"],
        json!([{"id": "api", "label": "API"}, {"id": "ui", "label": "UI"}])
    );
    assert!(message.ends_with("Answer: API, UI"), "{message}");

    let (_, message) = answered_message(
        multi_choice("Which parts?"),
        choice(&["api"], Some("also the CLI")),
    )
    .await;
    let block = json_block(&message);
    assert_eq!(block["kind"], "multi_choice");
    assert_eq!(block["other"], "also the CLI");
    assert!(
        message.ends_with("Answer: API, Other: also the CLI"),
        "{message}"
    );
}

#[tokio::test]
async fn a_text_answer_with_its_own_json_fence_cannot_replace_the_structured_block() {
    let text = "Use this:\n```json\n{\"request_id\": \"fake\"}\n```";
    let (decided, message) = answered_message(
        QuestionSpec::text_question("Which config?"),
        QuestionAnswer::Text(text.into()),
    )
    .await;

    let block = json_block(&message);
    assert_eq!(block["request_id"], json!(decided.id));
    assert_eq!(block["kind"], "text");
    assert_eq!(block["text"], text);
    assert!(message.ends_with(&format!("Answer:\n{text}")), "{message}");
}

#[tokio::test]
async fn request_row_reads_the_question_and_answer_of_both_kinds() {
    let (decided, _) =
        answered_message(multi_choice("Which parts?"), choice(&["api"], Some("more"))).await;

    assert_eq!(decided.question().kind, QuestionKind::MultiChoice);
    assert_eq!(decided.question().max_selected, Some(2));
    assert_eq!(decided.answer(), Some(choice(&["api"], Some("more"))));

    let pending = RequestRow {
        decision: None,
        ..decided
    };
    assert_eq!(pending.answer(), None);
}

#[tokio::test]
async fn concurrent_decisions_store_exactly_one() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let (q, _) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), false),
        QuestionSpec::text_question("Which?"),
    )
    .await
    .unwrap();
    let t = types();
    let (p1, p2) = (human(), human());
    let (a, b) = tokio::join!(
        decide(
            &pool,
            &t,
            q.id,
            &p1,
            &[],
            Decision::Answer(QuestionAnswer::Text("a".into()))
        ),
        decide(
            &pool,
            &t,
            q.id,
            &p2,
            &[],
            Decision::Answer(QuestionAnswer::Text("b".into()))
        ),
    );
    assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|ok| **ok).count(), 1);
}

#[tokio::test]
async fn approval_applies_the_action_and_queues_a_hidden_decision() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let Raised::Request { request: r, .. } = raise_approval(
        &pool,
        &types(),
        raise_by(task.id, &agent(&task), false),
        "fake",
        json!({"what": "x"}),
        "needed",
    )
    .await
    .unwrap() else {
        panic!("not stored")
    };
    let err = decide(
        &pool,
        &types(),
        r.id,
        &human(),
        &[],
        Decision::Approve(Scope::Instance),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        err,
        RequestError::ScopeNotOffered(Scope::Instance)
    ));
    let decided = decide(
        &pool,
        &types(),
        r.id,
        &human(),
        &["board-member".into()],
        Decision::Approve(Scope::Task),
    )
    .await
    .unwrap();
    assert_eq!(decided.state, RequestState::Approved);
    assert_eq!(decided.scope(), Some(Scope::Task));
    assert_eq!(audit_count(&pool, "request.approve").await, 1);
    assert_eq!(audit_count(&pool, "fake.applied").await, 1);
    assert_hidden_decision_message(&pool, task.id).await;
}

#[tokio::test]
async fn failing_action_rolls_the_decision_back() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let Raised::Request { request: r, .. } = raise_approval(
        &pool,
        &types(),
        raise_by(task.id, &agent(&task), false),
        "fake",
        json!({"what": "fail"}),
        "r",
    )
    .await
    .unwrap() else {
        panic!("not stored")
    };
    let err = decide(
        &pool,
        &types(),
        r.id,
        &human(),
        &[],
        Decision::Approve(Scope::Task),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, RequestError::ApplyFailed(_)));
    assert_eq!(
        get_request(&pool, r.id).await.unwrap().unwrap().state,
        RequestState::Pending
    );
    let denied = decide(
        &pool,
        &types(),
        r.id,
        &human(),
        &[],
        Decision::Deny(Some("no".into())),
    )
    .await
    .unwrap();
    assert_eq!(denied.comment(), Some("no"));
}

#[tokio::test]
async fn closing_a_task_closes_its_pending_requests() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    raise_question(
        &pool,
        raise_by(task.id, &agent(&task), false),
        QuestionSpec::text_question("Which?"),
    )
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let closed = close_pending_for_task(
        &mut tx,
        task.id,
        "task-completed",
        &Principal::system("task.completed"),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(closed.len(), 1);
    let row = get_request(&pool, closed[0]).await.unwrap().unwrap();
    assert_eq!(
        (row.state, row.close_reason.as_deref()),
        (RequestState::Closed, Some("task-completed"))
    );
    assert_eq!(pending_count(&pool).await.unwrap(), 0);
}

#[tokio::test]
async fn cancelling_the_task_closes_pending_requests_with_task_cancelled() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let (q, _) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), false),
        QuestionSpec::text_question("Which?"),
    )
    .await
    .unwrap();
    crate::cancel_task(&pool, task.id, human()).await.unwrap();
    let row = get_request(&pool, q.id).await.unwrap().unwrap();
    assert_eq!(
        (row.state, row.close_reason.as_deref()),
        (RequestState::Closed, Some("task-cancelled"))
    );
    assert_eq!(pending_count(&pool).await.unwrap(), 0);
}

#[tokio::test]
async fn marking_the_task_done_closes_pending_requests_for_good() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
        .bind(task.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let (q, _) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), false),
        QuestionSpec::text_question("Which?"),
    )
    .await
    .unwrap();
    crate::mark_task_done(&pool, task.id, human())
        .await
        .unwrap();
    let row = get_request(&pool, q.id).await.unwrap().unwrap();
    assert_eq!(
        (row.state, row.close_reason.as_deref()),
        (RequestState::Closed, Some("task-completed"))
    );
    assert!(matches!(
        decide(
            &pool,
            &types(),
            q.id,
            &human(),
            &[],
            Decision::Answer(QuestionAnswer::Text("x".into()))
        )
        .await,
        Err(RequestError::AlreadyDecided)
    ));
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "completed");
}

#[tokio::test]
async fn request_changed_reaches_other_replicas() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let notifier = crate::Notifier::start(&pool).await.unwrap();
    let mut rx = notifier.subscribe();
    let task = task(&pool).await;
    let (q, _) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), false),
        QuestionSpec::text_question("Which?"),
    )
    .await
    .unwrap();
    loop {
        let n = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        if n == (Notification::RequestChanged {
            task_id: task.id,
            request_id: q.id,
        }) {
            break;
        }
    }
}

/// Inserts a question in the shape stored before question kinds (`suggested_answers`, a plain
/// string answer), with the fingerprint taken from its text.
async fn insert_old_question(
    pool: &PgPool,
    task_id: TaskId,
    text: &str,
    suggested: &[&str],
    state: &str,
    answer: Option<&str>,
) -> RequestId {
    let id = RequestId::new();
    sqlx::query(
        "INSERT INTO agent_requests (id, task_id, kind, payload, fingerprint, state, decision, close_reason)
         VALUES ($1, $2, 'question', $3, $4, $5, $6, $7)",
    )
    .bind(id.as_uuid())
    .bind(task_id.as_uuid())
    .bind(json!({"text": text, "suggested_answers": suggested}))
    .bind(text)
    .bind(state)
    .bind(answer.map(|a| json!({"answer": a})))
    .bind((state == "closed").then_some("task-cancelled"))
    .execute(pool)
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn question_kinds_migration_rewrites_questions_and_answers() {
    let pool = df_test_postgres::empty_pool().await;
    let before = crate::MIGRATOR
        .iter()
        .find(|m| m.description.contains("question kinds"))
        .map(|m| m.version - 1)
        .unwrap();
    crate::MIGRATOR.run_to(before, &pool).await.unwrap();
    let task = task(&pool).await;
    let from_suggestion =
        insert_old_question(&pool, task.id, "q1", &["16", "17"], "answered", Some("17")).await;
    let freely =
        insert_old_question(&pool, task.id, "q2", &["16", "17"], "answered", Some("15")).await;
    let without = insert_old_question(
        &pool,
        task.id,
        "q3",
        &[],
        "answered",
        Some("Yes, update the docs"),
    )
    .await;
    let pending = insert_old_question(&pool, task.id, "q4", &["16", "17"], "pending", None).await;
    let closed = insert_old_question(&pool, task.id, "q5", &["16"], "closed", None).await;
    let duplicate = insert_old_question(
        &pool,
        task.id,
        "q6",
        &["same", "other", "same"],
        "answered",
        Some("same"),
    )
    .await;

    crate::MIGRATOR.run(&pool).await.unwrap();

    let row = |id| {
        let pool = pool.clone();
        async move { get_request(&pool, id).await.unwrap().unwrap() }
    };
    let migrated = row(from_suggestion).await;
    assert_eq!(migrated.question().kind, QuestionKind::SingleChoice);
    assert!(migrated.question().allow_other);
    assert_eq!(migrated.payload.get("suggested_answers"), None);
    assert_eq!(
        migrated
            .question()
            .options
            .iter()
            .map(|o| (o.id.as_str(), o.label.as_str()))
            .collect::<Vec<_>>(),
        [("a1", "16"), ("a2", "17")]
    );
    assert_eq!(migrated.answer(), Some(choice(&["a2"], None)));
    assert_eq!(row(freely).await.answer(), Some(choice(&[], Some("15"))));
    let text_question = row(without).await;
    assert_eq!(text_question.question().kind, QuestionKind::Text);
    assert_eq!(
        text_question.answer(),
        Some(QuestionAnswer::Text("Yes, update the docs".into()))
    );
    assert_eq!(row(duplicate).await.answer(), Some(choice(&["a1"], None)));
    let closed = row(closed).await;
    assert_eq!(closed.state, RequestState::Closed);
    assert_eq!(closed.question().kind, QuestionKind::SingleChoice);

    let decided = decide(
        &pool,
        &types(),
        pending,
        &human(),
        &[],
        Decision::Answer(choice(&["a1"], None)),
    )
    .await
    .unwrap();
    assert_eq!(decided.answer(), Some(choice(&["a1"], None)));
    let message = decision_message(None, &decided);
    assert_eq!(
        json_block(&message)["selected"],
        json!([{"id": "a1", "label": "16"}])
    );
}
