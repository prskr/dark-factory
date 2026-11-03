use super::*;
use crate::tasks::{NewTask, create_task};
use df_core::{PersonaId, ProjectId, UserId};

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
    sqlx::query("INSERT INTO personas (id, name, system_prompt, harness) VALUES ($1, 'Linus', 's', 'claude-code')")
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
            created_by: None,
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
    let answers = vec!["16".to_string(), "17".to_string()];
    let (first, created) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), false),
        "Use Postgres 16 or 17?",
        &answers,
    )
    .await
    .unwrap();
    assert!(created);
    assert_eq!(first.state, RequestState::Pending);
    assert_eq!(first.suggested_answers(), ["16", "17"]);
    let (second, created) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), true),
        "  use postgres 16   or 17? ",
        &[],
    )
    .await
    .unwrap();
    assert!(!created);
    assert_eq!(second.id, first.id);
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
            &format!("q{i}"),
            &[],
        )
        .await
        .unwrap();
    }
    let err = raise_question(&pool, raise_by(task.id, &actor, false), "one more", &[])
        .await
        .unwrap_err();
    assert!(matches!(err, RequestError::TooManyPending));
    let (_, created) = raise_question(&pool, raise_by(task.id, &actor, false), "q0", &[])
        .await
        .unwrap();
    assert!(!created);
    assert_eq!(pending_count(&pool).await.unwrap(), MAX_PENDING_PER_TASK);
}

#[tokio::test]
async fn answer_is_stored_audited_and_queued_as_a_message() {
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
        "Which?",
        &["17".into()],
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
        Decision::Answer("17".into()),
    )
    .await
    .unwrap();
    assert_eq!(decided.state, RequestState::Answered);
    assert_eq!(decided.answer(), Some("17"));
    assert_eq!(decided.decided_by, Some(peter));
    assert_eq!(audit_count(&pool, "request.answer").await, 1);
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(task.id.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "queued");
    let author: Value = sqlx::query_scalar(
        "SELECT author FROM task_messages WHERE task_id = $1 ORDER BY queued_at DESC LIMIT 1",
    )
    .bind(task.id.as_uuid())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(author["reason"], "request.decided");
    let again = decide(
        &pool,
        &types(),
        q.id,
        &human(),
        &[],
        Decision::Answer("16".into()),
    )
    .await
    .unwrap_err();
    assert!(matches!(again, RequestError::AlreadyDecided));
}

#[tokio::test]
async fn concurrent_decisions_store_exactly_one() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let (q, _) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), false),
        "Which?",
        &[],
    )
    .await
    .unwrap();
    let t = types();
    let (p1, p2) = (human(), human());
    let (a, b) = tokio::join!(
        decide(&pool, &t, q.id, &p1, &[], Decision::Answer("a".into())),
        decide(&pool, &t, q.id, &p2, &[], Decision::Answer("b".into())),
    );
    assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|ok| **ok).count(), 1);
}

#[tokio::test]
async fn approval_applies_the_action_and_is_audited() {
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
        "Which?",
        &[],
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
        "Which?",
        &[],
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
        "Which?",
        &[],
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
            Decision::Answer("x".into())
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
async fn suggested_answers_are_validated() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    for bad in [
        vec!["  ".to_string()],
        vec!["x".repeat(MAX_SUGGESTED_ANSWER_CHARS + 1)],
    ] {
        let err = raise_question(
            &pool,
            raise_by(task.id, &agent(&task), false),
            "Which?",
            &bad,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, RequestError::Invalid(_)), "{err:?}");
    }
    assert_eq!(pending_count(&pool).await.unwrap(), 0);
}

#[tokio::test]
async fn padded_suggested_answers_are_stored_trimmed() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool).await;
    let padded = format!("  {}  ", "x".repeat(MAX_SUGGESTED_ANSWER_CHARS));
    let (q, _) = raise_question(
        &pool,
        raise_by(task.id, &agent(&task), false),
        "Which?",
        &[padded],
    )
    .await
    .unwrap();
    assert_eq!(
        q.suggested_answers(),
        ["x".repeat(MAX_SUGGESTED_ANSWER_CHARS)]
    );
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
        "Which?",
        &[],
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
