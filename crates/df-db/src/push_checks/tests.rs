//! Tests of the push policy storage, the exceptions and the reactions to push records
//! (container-backed).

use std::time::Duration;

use df_core::UserId;
use df_proto::transcript::v1::content_block;

use super::*;
use crate::git_pushes::{RefUpdate, record_push};
use crate::notify::Notifier;
use crate::requests::{
    ApprovalTypes, Decision, RequestState, close_pending_for_task, decide, list_for_task,
    raise_question,
};

const TASK_BRANCH: &str = "refs/heads/factory/task/DF-1";

struct Seed {
    project: Uuid,
    task: TaskId,
    run: df_core::RunId,
}

/// Project `DF` with task `DF-1` and its running run.
async fn seed(pool: &PgPool) -> Seed {
    let (project, persona, task, run) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image)
         VALUES ($1, 'DF', 'DF', 'https://example.test/r.git', 'img')",
    )
    .bind(project)
    .execute(pool)
    .await
    .unwrap();
    crate::model_providers::tests::seed_provider(pool).await;
    sqlx::query("INSERT INTO personas (id, name, system_prompt, model, model_provider_id) VALUES ($1, 'Linus', '', 'test-model', (SELECT id FROM model_providers LIMIT 1))")
        .bind(persona)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, project_id, number, key, title, title_source, persona_id, status, created_by)
         VALUES ($1, $2, 1, 'DF-1', 't', 'user', $3, 'running',
                 '{\"kind\":\"system\",\"reason\":\"test\"}')",
    )
    .bind(task)
    .bind(project)
    .bind(persona)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, 'running')")
        .bind(run)
        .bind(task)
        .execute(pool)
        .await
        .unwrap();
    Seed {
        project,
        task: TaskId::from_uuid(task),
        run: df_core::RunId::from_uuid(run),
    }
}

/// A pending `push-check-exception` request of `task` without findings.
async fn insert_request(pool: &PgPool, task: TaskId) -> RequestId {
    let request = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO agent_requests (id, task_id, kind, approval_type, payload, fingerprint)
         VALUES ($1, $2, 'approval', 'push-check-exception', '{}', 'f')",
    )
    .bind(request)
    .bind(task.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    RequestId::from_uuid(request)
}

#[test]
fn the_default_policy_asks_for_approval_and_protects_ci_workflows_and_codeowners() {
    let policy = PushPolicy::default();
    assert_eq!(
        policy.protected_globs,
        [
            ".forgejo/workflows/**",
            ".github/workflows/**",
            ".gitea/workflows/**",
            "CODEOWNERS"
        ]
    );
    assert_eq!(
        (policy.protected_mode, policy.secret_mode, policy.size_mode),
        (
            CheckMode::Approval,
            CheckMode::Approval,
            CheckMode::Approval
        )
    );
    assert_eq!(policy.max_blob_bytes, 5 * 1024 * 1024);
    assert!(!policy.linear_history);
}

#[test]
fn check_modes_round_trip_and_unknown_ones_are_invalid() {
    for mode in [CheckMode::Reject, CheckMode::Approval] {
        assert_eq!(mode.as_str().parse::<CheckMode>().unwrap(), mode);
    }
    assert!(matches!(
        "warn".parse::<CheckMode>(),
        Err(DbError::InvalidData(_))
    ));
}

#[tokio::test]
async fn only_stored_policies_of_the_asked_projects_are_returned() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let other = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image)
         VALUES ($1, 'WEB', 'WEB', 'https://example.test/w.git', 'img')",
    )
    .bind(other)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_push_policies
             (project_id, protected_globs, protected_mode, secret_mode, size_mode,
              max_blob_bytes, linear_history)
         VALUES ($1, ARRAY['deploy/**'], 'reject', 'approval', 'reject', 1024, true)",
    )
    .bind(seed.project)
    .execute(&pool)
    .await
    .unwrap();

    let policies = push_policies(&pool, &[seed.project, other]).await.unwrap();

    assert_eq!(
        policies,
        HashMap::from([(
            seed.project,
            PushPolicy {
                protected_globs: vec!["deploy/**".into()],
                protected_mode: CheckMode::Reject,
                secret_mode: CheckMode::Approval,
                size_mode: CheckMode::Reject,
                max_blob_bytes: 1024,
                linear_history: true,
            }
        )])
    );
    assert!(push_policies(&pool, &[other]).await.unwrap().is_empty());
}

#[tokio::test]
async fn added_exceptions_are_listed_once_per_task_and_announced() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let request = insert_request(&pool, seed.task).await;
    let notifier = Notifier::start(&pool).await.unwrap();
    let mut notifications = notifier.subscribe();
    let fingerprints = ["path:CODEOWNERS".to_string(), "size:a.bin:ab".to_string()];

    for attempt in [&fingerprints[..], &fingerprints[..1]] {
        let mut tx = pool.begin().await.unwrap();
        add_push_check_exceptions(&mut tx, seed.task, attempt, request)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }

    let exceptions = push_check_exceptions(&pool, &[seed.task.as_uuid(), Uuid::now_v7()])
        .await
        .unwrap();
    assert_eq!(
        exceptions,
        HashMap::from([(seed.task.as_uuid(), fingerprints.to_vec())])
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while notifications.recv().await.unwrap() != Notification::EgressPolicy {}
    })
    .await
    .unwrap();
}

fn finding(check: PushCheck, mode: CheckMode, path: &str, detail: &str) -> PushFinding {
    let (line, fingerprint, secret) = match check {
        PushCheck::SecretScan => (
            Some(3),
            Some("secret:github-pat:ab12:3".to_owned()),
            Some(MaskedSecret {
                rule: "github-pat".into(),
                masked: "ghp_********".into(),
            }),
        ),
        PushCheck::BlobTooLarge => (None, Some(format!("size:{path}:cd34")), None),
        PushCheck::ProtectedPath => (None, Some(format!("path:{path}")), None),
        PushCheck::Identity | PushCheck::MergeCommit => (None, None, None),
    };
    PushFinding {
        ref_name: TASK_BRANCH.into(),
        check,
        mode,
        fingerprint,
        path: path.into(),
        line,
        detail: detail.into(),
        secret,
    }
}

fn workflow(mode: CheckMode) -> PushFinding {
    finding(
        PushCheck::ProtectedPath,
        mode,
        ".forgejo/workflows/ci.yml",
        "matches .forgejo/workflows/**",
    )
}

fn secret(mode: CheckMode) -> PushFinding {
    finding(
        PushCheck::SecretScan,
        mode,
        "config.py",
        "github-pat ghp_********",
    )
}

fn identity() -> PushFinding {
    finding(
        PushCheck::Identity,
        CheckMode::Reject,
        "",
        "commit 0a1b author",
    )
}

fn push(seed: &Seed, verdict: PushVerdict, findings: Vec<PushFinding>) -> PushRecord {
    let approval_requested = verdict == PushVerdict::Rejected
        && !findings.is_empty()
        && findings.iter().all(|f| f.mode == CheckMode::Approval);
    PushRecord {
        push_id: Uuid::now_v7(),
        run_id: seed.run,
        gateway_pod: "gateway-0".into(),
        refs: vec![RefUpdate {
            name: TASK_BRANCH.into(),
            old_sha: "0".repeat(40),
            new_sha: "a".repeat(40),
            verdict,
            reason: String::new(),
        }],
        verdict,
        bytes: 1024,
        duration: Duration::from_millis(5),
        findings,
        approval_requested,
    }
}

async fn record(pool: &PgPool, record: &PushRecord) {
    record_push(pool, record).await.unwrap();
}

async fn exception_requests(pool: &PgPool, task: TaskId) -> Vec<RequestRow> {
    list_for_task(pool, task)
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.approval_type.as_deref() == Some(PUSH_CHECK_EXCEPTION))
        .collect()
}

/// The texts of the undelivered `push.rejected` messages of `task`.
async fn feedback_texts(pool: &PgPool, task: TaskId) -> Vec<String> {
    crate::tasks::list_undelivered_messages(pool, task)
        .await
        .unwrap()
        .into_iter()
        .filter(|m| m.author == Principal::system(PUSH_REJECTED))
        .flat_map(|m| m.content)
        .filter_map(|block| match block.kind {
            Some(content_block::Kind::Text(text)) => Some(text.text),
            _ => None,
        })
        .collect()
}

async fn deliver_all(pool: &PgPool, task: TaskId) {
    sqlx::query("UPDATE task_messages SET delivered_at = now() WHERE task_id = $1")
        .bind(task.as_uuid())
        .execute(pool)
        .await
        .unwrap();
}

fn peter() -> Principal {
    Principal::human(UserId::new(), "Peter")
}

#[tokio::test]
async fn a_push_rejected_only_by_approval_findings_raises_one_request_listing_them() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let findings = vec![workflow(CheckMode::Approval), secret(CheckMode::Approval)];

    record(&pool, &push(&seed, PushVerdict::Rejected, findings.clone())).await;
    let reversed = findings.into_iter().rev().collect();
    record(&pool, &push(&seed, PushVerdict::Rejected, reversed)).await;

    let [request] = &exception_requests(&pool, seed.task).await[..] else {
        panic!("not exactly one request");
    };
    assert_eq!(request.state, RequestState::Pending);
    assert_eq!(request.run_id, None);
    assert_eq!(request.reason, EXCEPTION_REASON);
    assert_eq!(
        request_findings(&request.payload),
        [
            ExceptionFinding {
                check: PushCheck::ProtectedPath,
                path: ".forgejo/workflows/ci.yml".into(),
                line: None,
                rule: None,
                preview: String::new(),
                fingerprint: "path:.forgejo/workflows/ci.yml".into(),
            },
            ExceptionFinding {
                check: PushCheck::SecretScan,
                path: "config.py".into(),
                line: Some(3),
                rule: Some("github-pat".into()),
                preview: "ghp_********".into(),
                fingerprint: "secret:github-pat:ab12:3".into(),
            },
        ]
    );
    assert_eq!(
        PushCheckException.describe(request),
        "Push needs approval: 2 findings"
    );
}

#[tokio::test]
async fn mixed_findings_and_protected_repository_configuration_raise_no_request() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let config = finding(
        PushCheck::ProtectedPath,
        CheckMode::Reject,
        ".dark-factory/config.toml",
        "matches .dark-factory/**",
    );

    record(
        &pool,
        &push(
            &seed,
            PushVerdict::Rejected,
            vec![identity(), workflow(CheckMode::Approval)],
        ),
    )
    .await;
    record(&pool, &push(&seed, PushVerdict::Rejected, vec![config])).await;

    assert!(exception_requests(&pool, seed.task).await.is_empty());
}

#[tokio::test]
async fn approving_creates_task_exceptions_for_exactly_the_listed_findings() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let findings = vec![workflow(CheckMode::Approval), secret(CheckMode::Approval)];
    record(&pool, &push(&seed, PushVerdict::Rejected, findings)).await;
    let request = exception_requests(&pool, seed.task).await[0].id;
    let types = ApprovalTypes::builtin();
    let roles = ["board-member".to_string()];

    let wider = decide(
        &pool,
        &types,
        request,
        &peter(),
        &roles,
        Decision::Approve(Scope::Project),
    )
    .await
    .unwrap_err();
    let approved = decide(
        &pool,
        &types,
        request,
        &peter(),
        &roles,
        Decision::Approve(Scope::Task),
    )
    .await
    .unwrap();

    assert!(matches!(
        wider,
        RequestError::ScopeNotOffered(Scope::Project)
    ));
    assert_eq!(approved.state, RequestState::Approved);
    let exceptions = push_check_exceptions(&pool, &[seed.task.as_uuid()])
        .await
        .unwrap();
    assert_eq!(
        exceptions[&seed.task.as_uuid()],
        ["path:.forgejo/workflows/ci.yml", "secret:github-pat:ab12:3"]
    );
}

#[tokio::test]
async fn an_agent_cannot_raise_an_exception_request() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let agent = Principal::agent(
        df_core::PersonaId::new(),
        "Linus",
        Some(seed.run),
        seed.task,
    );
    let payload =
        json!({ "findings": [ExceptionFinding::from_finding(&workflow(CheckMode::Approval))] });

    let error = crate::requests::raise_approval(
        &pool,
        &ApprovalTypes::builtin(),
        Raise {
            task_id: seed.task,
            run_id: Some(seed.run),
            actor: &agent,
            blocking: false,
        },
        PUSH_CHECK_EXCEPTION,
        payload,
        "please",
    )
    .await
    .unwrap_err();

    assert!(matches!(error, RequestError::Invalid(_)), "{error:?}");
    assert!(exception_requests(&pool, seed.task).await.is_empty());
}

#[tokio::test]
async fn an_accepted_task_branch_push_closes_the_pending_exception_requests_only() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let agent = Principal::agent(
        df_core::PersonaId::new(),
        "Linus",
        Some(seed.run),
        seed.task,
    );
    let from = Raise {
        task_id: seed.task,
        run_id: Some(seed.run),
        actor: &agent,
        blocking: false,
    };
    let (question, _) = raise_question(&pool, from, "Which base?", &[])
        .await
        .unwrap();
    record(
        &pool,
        &push(
            &seed,
            PushVerdict::Rejected,
            vec![secret(CheckMode::Approval)],
        ),
    )
    .await;

    let mut fixed = push(&seed, PushVerdict::Forwarding, Vec::new());
    record(&pool, &fixed).await;
    fixed.verdict = PushVerdict::Accepted;
    fixed.refs[0].verdict = PushVerdict::Accepted;
    record(&pool, &fixed).await;

    let [request] = &exception_requests(&pool, seed.task).await[..] else {
        panic!("not exactly one request");
    };
    assert_eq!(request.state, RequestState::Closed);
    assert_eq!(request.close_reason.as_deref(), Some("push-accepted"));
    let question = crate::requests::get_request(&pool, question.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(question.state, RequestState::Pending);
}

#[tokio::test]
async fn a_rejected_push_queues_feedback_with_the_masked_findings() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;

    record(
        &pool,
        &push(
            &seed,
            PushVerdict::Rejected,
            vec![secret(CheckMode::Reject)],
        ),
    )
    .await;

    let [text] = &feedback_texts(&pool, seed.task).await[..] else {
        panic!("not exactly one message");
    };
    assert!(
        text.contains("- df:secret-scan config.py:3 (github-pat ghp_********)"),
        "{text}"
    );
    assert!(text.contains("factory/task/DF-1"), "{text}");
    assert!(text.contains("cannot be approved"), "{text}");
}

#[tokio::test]
async fn feedback_says_when_an_approval_was_requested() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;

    record(
        &pool,
        &push(
            &seed,
            PushVerdict::Rejected,
            vec![workflow(CheckMode::Approval)],
        ),
    )
    .await;

    let [text] = &feedback_texts(&pool, seed.task).await[..] else {
        panic!("not exactly one message");
    };
    assert!(
        text.contains(
            "- df:protected-path .forgejo/workflows/ci.yml (matches .forgejo/workflows/**)"
        ),
        "{text}"
    );
    assert!(text.contains("A human was asked to approve"), "{text}");
}

#[tokio::test]
async fn feedback_stops_after_three_rejections_until_a_push_is_accepted() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let rejected = || {
        push(
            &seed,
            PushVerdict::Rejected,
            vec![secret(CheckMode::Reject)],
        )
    };
    let queued = |pool: PgPool| async move {
        let texts = feedback_texts(&pool, seed.task).await.len();
        deliver_all(&pool, seed.task).await;
        texts
    };

    let mut counts = Vec::new();
    for _ in 0..4 {
        record(&pool, &rejected()).await;
        counts.push(queued(pool.clone()).await);
    }
    let accepted = push(&seed, PushVerdict::Accepted, Vec::new());
    record(&pool, &accepted).await;
    record(&pool, &rejected()).await;
    counts.push(queued(pool.clone()).await);

    assert_eq!(counts, [1, 1, 1, 0, 1]);
}

#[tokio::test]
async fn no_second_feedback_while_one_is_undelivered_or_for_a_retried_record() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let first = push(
        &seed,
        PushVerdict::Rejected,
        vec![secret(CheckMode::Reject)],
    );

    record(&pool, &first).await;
    record(&pool, &first).await;
    record(&pool, &push(&seed, PushVerdict::Rejected, vec![identity()])).await;

    assert_eq!(feedback_texts(&pool, seed.task).await.len(), 1);
    let count: i32 =
        sqlx::query_scalar("SELECT push_rejections_since_accept FROM tasks WHERE id = $1")
            .bind(seed.task.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn a_too_large_task_branch_push_queues_its_reason_up_to_the_cap() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let too_large = || {
        let mut record = push(&seed, PushVerdict::Rejected, Vec::new());
        record.refs[0].reason = "df:push-too-large".into();
        record
    };

    let mut texts = Vec::new();
    for _ in 0..4 {
        record(&pool, &too_large()).await;
        texts.push(feedback_texts(&pool, seed.task).await);
        deliver_all(&pool, seed.task).await;
    }

    let counts: Vec<usize> = texts.iter().map(Vec::len).collect();
    assert_eq!(counts, [1, 1, 1, 0]);
    assert_eq!(
        texts[0],
        ["The Git proxy rejected your push to factory/task/DF-1: df:push-too-large"]
    );
}

#[tokio::test]
async fn a_failed_check_queues_no_feedback() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let mut failed = push(&seed, PushVerdict::Rejected, Vec::new());
    failed.refs[0].reason = "df:check-failed".into();

    record(&pool, &failed).await;

    assert!(feedback_texts(&pool, seed.task).await.is_empty());
}

#[tokio::test]
async fn feedback_says_when_too_many_pending_requests_prevent_an_approval_request() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    for i in 0..crate::requests::MAX_PENDING_PER_TASK {
        sqlx::query(
            "INSERT INTO agent_requests (id, task_id, kind, payload, fingerprint)
             VALUES ($1, $2, 'question', '{}', $3)",
        )
        .bind(Uuid::now_v7())
        .bind(seed.task.as_uuid())
        .bind(format!("q{i}"))
        .execute(&pool)
        .await
        .unwrap();
    }

    record(
        &pool,
        &push(
            &seed,
            PushVerdict::Rejected,
            vec![workflow(CheckMode::Approval)],
        ),
    )
    .await;

    assert!(exception_requests(&pool, seed.task).await.is_empty());
    let [text] = &feedback_texts(&pool, seed.task).await[..] else {
        panic!("not exactly one message");
    };
    assert!(
        text.contains("These findings need a human's approval, but no request was raised because the task has too many pending requests."),
        "{text}"
    );
    assert!(!text.contains("cannot be approved"), "{text}");
}

#[tokio::test]
async fn findings_of_the_wip_branch_and_of_completed_tasks_change_nothing() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let seed = seed(&pool).await;
    let mut wip = secret(CheckMode::Reject);
    wip.ref_name = "refs/heads/factory/wip/DF-1".into();

    record(&pool, &push(&seed, PushVerdict::Rejected, vec![wip])).await;
    let mut tx = pool.begin().await.unwrap();
    close_pending_for_task(
        &mut tx,
        seed.task,
        "task-completed",
        &Principal::system("t"),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("UPDATE tasks SET status = 'completed' WHERE id = $1")
        .bind(seed.task.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    record(
        &pool,
        &push(
            &seed,
            PushVerdict::Rejected,
            vec![workflow(CheckMode::Approval)],
        ),
    )
    .await;

    assert!(feedback_texts(&pool, seed.task).await.is_empty());
    assert!(exception_requests(&pool, seed.task).await.is_empty());
    let status: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = $1")
        .bind(seed.task.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "completed");
}
