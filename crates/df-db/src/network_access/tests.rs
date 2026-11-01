use df_core::{PersonaId, Principal, ProjectId, RunId, TaskId, UserId};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use super::*;
use crate::egress::{EntryKind, HostScope, add_entry, http_host_entries};
use crate::requests::{
    ApprovalTypes, Decision, Raise, Raised, RequestError, RequestRow, RequestState, Scope, decide,
    get_request, list_for_task, raise_approval,
};
use crate::tasks::{NewTask, TaskRow, create_task, mark_task_done};

fn types() -> ApprovalTypes {
    ApprovalTypes::builtin()
}

fn human() -> Principal {
    Principal::human(UserId::new(), "Peter")
}

fn roles() -> Vec<String> {
    vec!["board-member".to_string()]
}

async fn project(pool: &PgPool, key: &str) -> ProjectId {
    let id = ProjectId::new();
    sqlx::query(
        "INSERT INTO projects (id, key, name, repo_url, image, base_branch)
         VALUES ($1, $2, $2, 'https://x.test/r.git', 'img', 'main')",
    )
    .bind(id.as_uuid())
    .bind(key)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn task(pool: &PgPool, project_id: ProjectId) -> TaskRow {
    let persona_id = PersonaId::new();
    sqlx::query(
        "INSERT INTO personas (id, name, system_prompt, harness) VALUES ($1, $2, 's', 'claude-code')",
    )
    .bind(persona_id.as_uuid())
    .bind(persona_id.to_string())
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

async fn try_request(pool: &PgPool, task: &TaskRow, payload: Value) -> Raised {
    let agent = Principal::agent(PersonaId::new(), "Linus", None, task.id);
    let from = Raise {
        task_id: task.id,
        run_id: None,
        actor: &agent,
        blocking: false,
    };
    raise_approval(pool, &types(), from, NETWORK_ACCESS, payload, "pip install")
        .await
        .unwrap()
}

async fn request(pool: &PgPool, task: &TaskRow, payload: Value) -> RequestRow {
    match try_request(pool, task, payload).await {
        Raised::Request { request, .. } => *request,
        Raised::AlreadyGranted => panic!("already granted"),
    }
}

async fn add_host_entry(pool: &PgPool, scope: HostScope, host: &str, port: u16) {
    let (principal, roles) = (human(), roles());
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let mut tx = pool.begin().await.unwrap();
    create_http_host_entry(&mut tx, scope, host, port, actor, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

async fn reload(pool: &PgPool, request: &RequestRow) -> RequestRow {
    get_request(pool, request.id).await.unwrap().unwrap()
}

async fn decided_messages(pool: &PgPool, task: TaskId) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM task_messages WHERE task_id = $1 AND author->>'reason' = 'request.decided'",
    )
    .bind(task.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn add_project_entry(pool: &PgPool, decider: &Principal, key: &str, host: &str, port: u16) {
    let roles = roles();
    let actor = Actor {
        principal: decider,
        roles: &roles,
    };
    let kind = EntryKind::HttpHost {
        host: host.into(),
        port,
    };
    add_entry(pool, actor, Some(key), &kind).await.unwrap();
}

#[test]
fn the_fingerprint_is_the_normalized_host_and_port() {
    assert_eq!(
        NetworkAccess.fingerprint(&json!({ "host": " PyPI.org. " })),
        "pypi.org:443"
    );
    assert_eq!(
        NetworkAccess.fingerprint(&json!({ "host": "index.crates.io", "port": 8443 })),
        "index.crates.io:8443"
    );
    assert_eq!(
        NetworkAccess.scopes(),
        [Scope::Task, Scope::Project, Scope::Instance]
    );
}

#[test]
fn the_builtin_types_include_network_access() {
    assert!(ApprovalTypes::builtin().get(NETWORK_ACCESS).is_some());
}

#[tokio::test]
async fn a_host_the_task_may_reach_already_is_not_stored() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    let (df7, df8) = (task(&pool, df).await, task(&pool, df).await);
    let other = task(&pool, project(&pool, "OPS").await).await;
    add_host_entry(&pool, HostScope::Instance, "*.crates.io", 443).await;
    add_host_entry(&pool, HostScope::Project(df.as_uuid()), "pypi.org", 443).await;
    let pythonhosted = "files.pythonhosted.org";
    add_host_entry(&pool, HostScope::Task(df8.id.as_uuid()), pythonhosted, 443).await;

    for (task, payload) in [
        (&df7, json!({ "host": "static.crates.io" })),
        (&df7, json!({ "host": "PyPI.org", "port": 443 })),
        (&df8, json!({ "host": pythonhosted })),
    ] {
        let raised = try_request(&pool, task, payload.clone()).await;
        assert_eq!(raised, Raised::AlreadyGranted, "{payload}");
    }
    for (task, payload) in [
        (&df7, json!({ "host": pythonhosted })),
        (&df7, json!({ "host": "pypi.org", "port": 8443 })),
        (&df7, json!({ "host": "crates.io" })),
        (&other, json!({ "host": "pypi.org" })),
    ] {
        let raised = try_request(&pool, task, payload.clone()).await;
        assert!(
            matches!(raised, Raised::Request { new: true, .. }),
            "{payload}"
        );
    }
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 4);
}

#[test]
fn targets_are_validated_like_http_host_entries() {
    assert_eq!(
        requested_target(&json!({ "host": "pypi.org", "port": 80 })).unwrap(),
        ("pypi.org".to_string(), 80)
    );
    for bad in [
        json!({ "host": "10.0.0.5" }),
        json!({ "host": "" }),
        json!({ "port": 443 }),
        json!({ "host": 7 }),
        json!({ "host": "pypi.org", "port": 0 }),
        json!({ "host": "pypi.org", "port": 65536 }),
        json!({ "host": "pypi.org", "port": "443" }),
        json!("pypi.org"),
    ] {
        assert!(
            matches!(requested_target(&bad), Err(RequestError::Invalid(_))),
            "{bad}"
        );
    }
}

#[tokio::test]
async fn an_invalid_host_is_rejected_and_nothing_is_stored() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let task = task(&pool, project(&pool, "DF").await).await;
    let agent = Principal::agent(PersonaId::new(), "Linus", None, task.id);
    let from = Raise {
        task_id: task.id,
        run_id: None,
        actor: &agent,
        blocking: false,
    };

    let error = raise_approval(
        &pool,
        &types(),
        from,
        NETWORK_ACCESS,
        json!({ "host": "10.0.0.5", "port": 443 }),
        "",
    )
    .await
    .unwrap_err();

    assert!(matches!(error, RequestError::Invalid(_)), "{error}");
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 0);
}

#[tokio::test]
async fn a_project_entry_added_by_hand_approves_the_requests_of_the_project() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    let (df7, df8) = (task(&pool, df).await, task(&pool, df).await);
    let other = task(&pool, project(&pool, "OPS").await).await;
    let pypi = json!({ "host": "pypi.org", "port": 443 });
    let r7 = request(&pool, &df7, pypi.clone()).await;
    let r8 = request(&pool, &df8, json!({ "host": "PyPI.org" })).await;
    let ro = request(&pool, &other, pypi).await;
    let peter = human();

    add_project_entry(&pool, &peter, "DF", "pypi.org", 443).await;

    for (request, task) in [(&r7, &df7), (&r8, &df8)] {
        let decided = reload(&pool, request).await;
        assert_eq!(decided.state, RequestState::Approved);
        assert_eq!(decided.scope(), Some(Scope::Project));
        assert_eq!(decided.decided_by.as_ref(), Some(&peter));
        assert_eq!(decided_messages(&pool, task.id).await, 1);
    }
    assert_eq!(reload(&pool, &ro).await.state, RequestState::Pending);
    assert_eq!(decided_messages(&pool, other.id).await, 0);
    let approvals: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'request.approve'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(approvals, 2);
}

#[tokio::test]
async fn a_wildcard_entry_approves_covered_hosts_on_its_port_only() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df7 = task(&pool, project(&pool, "DF").await).await;
    let static_crates = request(&pool, &df7, json!({ "host": "static.crates.io" })).await;
    let index_crates = request(
        &pool,
        &df7,
        json!({ "host": "index.crates.io", "port": 8443 }),
    )
    .await;
    let apex = request(&pool, &df7, json!({ "host": "crates.io" })).await;

    add_project_entry(&pool, &human(), "DF", "*.crates.io", 443).await;

    assert_eq!(
        reload(&pool, &static_crates).await.state,
        RequestState::Approved
    );
    assert_eq!(
        reload(&pool, &index_crates).await.state,
        RequestState::Pending
    );
    assert_eq!(reload(&pool, &apex).await.state, RequestState::Pending);
}

#[tokio::test]
async fn a_request_decided_before_settling_reaches_it_is_skipped() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df7 = task(&pool, project(&pool, "DF").await).await;
    let pypi = request(&pool, &df7, json!({ "host": "pypi.org" })).await;
    let (peter, roles) = (human(), roles());
    decide(
        &pool,
        &types(),
        pypi.id,
        &peter,
        &roles,
        Decision::Deny(None),
    )
    .await
    .unwrap();
    let actor = Actor {
        principal: &peter,
        roles: &roles,
    };

    let mut tx = pool.begin().await.unwrap();
    let settled = record_approval_in(&mut tx, &NetworkAccess, pypi.id, actor, Scope::Task)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(settled.is_none());
    assert_eq!(reload(&pool, &pypi).await.state, RequestState::Denied);
    assert_eq!(decided_messages(&pool, df7.id).await, 1);
}

#[tokio::test]
async fn approved_for_the_task_creates_a_task_entry_and_settles_only_that_task() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df = project(&pool, "DF").await;
    let (df7, df8) = (task(&pool, df).await, task(&pool, df).await);
    let r7 = request(&pool, &df7, json!({ "host": "pypi.org" })).await;
    let r8 = request(&pool, &df8, json!({ "host": "pypi.org" })).await;
    let peter = human();

    let decided = decide(
        &pool,
        &types(),
        r7.id,
        &peter,
        &roles(),
        Decision::Approve(Scope::Task),
    )
    .await
    .unwrap();

    assert_eq!(decided.state, RequestState::Approved);
    assert_eq!(decided.scope(), Some(Scope::Task));
    let entries = http_host_entries(&pool).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].scope, HostScope::Task(df7.id.as_uuid()));
    assert_eq!(
        (entries[0].host.as_str(), entries[0].port),
        ("pypi.org", 443)
    );
    let creator: Value =
        sqlx::query_scalar("SELECT actor FROM audit_log WHERE action = 'egress_allowlist.create'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(creator, serde_json::to_value(&peter).unwrap());
    // The decided request gets exactly one decision message, the other task none.
    assert_eq!(decided_messages(&pool, df7.id).await, 1);
    assert_eq!(reload(&pool, &r8).await.state, RequestState::Pending);
}

#[tokio::test]
async fn approving_an_entry_that_exists_already_succeeds() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df7 = task(&pool, project(&pool, "DF").await).await;
    let first = request(&pool, &df7, json!({ "host": "pypi.org" })).await;
    decide(
        &pool,
        &types(),
        first.id,
        &human(),
        &roles(),
        Decision::Approve(Scope::Task),
    )
    .await
    .unwrap();
    // Raising checks the allowlist; only a request that raced the entry is still pending.
    let second = RequestId::from_uuid(Uuid::now_v7());
    sqlx::query(
        "INSERT INTO agent_requests (id, task_id, kind, approval_type, payload, fingerprint)
         VALUES ($1, $2, 'approval', $3, '{\"host\": \"pypi.org\"}', 'pypi.org:443')",
    )
    .bind(second.as_uuid())
    .bind(df7.id.as_uuid())
    .bind(NETWORK_ACCESS)
    .execute(&pool)
    .await
    .unwrap();

    let decided = decide(
        &pool,
        &types(),
        second,
        &human(),
        &roles(),
        Decision::Approve(Scope::Task),
    )
    .await
    .unwrap();

    assert_eq!(decided.state, RequestState::Approved);
    assert_eq!(http_host_entries(&pool).await.unwrap().len(), 1);
}

async fn run(pool: &PgPool, task: &TaskRow, state: &str) -> RunId {
    let id = RunId::new();
    sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, 1, $3)")
        .bind(id.as_uuid())
        .bind(task.id.as_uuid())
        .bind(state)
        .execute(pool)
        .await
        .unwrap();
    id
}

async fn pending_for(pool: &PgPool, task: &TaskRow) -> Vec<RequestRow> {
    list_for_task(pool, task.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.state == RequestState::Pending)
        .collect()
}

#[tokio::test]
async fn denials_of_an_unknown_host_raise_one_request_from_the_control_plane() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df7 = task(&pool, project(&pool, "DF").await).await;
    let run_id = run(&pool, &df7, "running").await;

    let first = raise_from_denial(&pool, run_id, "statsig.anthropic.com", 443)
        .await
        .unwrap();
    let again = raise_from_denial(&pool, run_id, "statsig.anthropic.com", 443)
        .await
        .unwrap();

    let DenialOutcome::Raised(Raised::Request { request, new: true }) = first else {
        panic!("{first:?}");
    };
    assert!(
        matches!(
            again,
            DenialOutcome::Raised(Raised::Request { new: false, .. })
        ),
        "{again:?}"
    );
    assert_eq!(request.approval_type.as_deref(), Some(NETWORK_ACCESS));
    assert_eq!(
        request.payload,
        json!({ "host": "statsig.anthropic.com", "port": 443 })
    );
    assert_eq!(
        request.reason,
        "The egress proxy denied statsig.anthropic.com:443"
    );
    assert!(!request.blocking);
    assert_eq!(request.run_id, None, "raised by the control plane");
    assert_eq!(pending_for(&pool, &df7).await.len(), 1);
}

#[tokio::test]
async fn a_denial_reported_after_the_host_was_allowed_raises_nothing() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df7 = task(&pool, project(&pool, "DF").await).await;
    let run_id = run(&pool, &df7, "running").await;
    add_host_entry(&pool, HostScope::Task(df7.id.as_uuid()), "pypi.org", 443).await;

    let outcome = raise_from_denial(&pool, run_id, "pypi.org", 443)
        .await
        .unwrap();

    assert_eq!(outcome, DenialOutcome::Raised(Raised::AlreadyGranted));
    assert!(pending_for(&pool, &df7).await.is_empty());
}

#[tokio::test]
async fn a_denial_reported_after_the_run_ended_or_the_task_was_done_raises_nothing() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let project_id = project(&pool, "DF").await;
    let df7 = task(&pool, project_id).await;
    let cancelled = run(&pool, &df7, "cancelled").await;
    let df8 = task(&pool, project_id).await;
    let idle = run(&pool, &df8, "awaiting_input").await;
    sqlx::query("UPDATE tasks SET status = 'waiting' WHERE id = $1")
        .bind(df8.id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    mark_task_done(&pool, df8.id, human()).await.unwrap();

    let after_cancel = raise_from_denial(&pool, cancelled, "pypi.org", 443)
        .await
        .unwrap();
    let after_done = raise_from_denial(&pool, idle, "pypi.org", 443)
        .await
        .unwrap();

    assert_eq!(after_cancel, DenialOutcome::RunEnded);
    assert_eq!(after_done, DenialOutcome::TaskCompleted);
    assert!(pending_for(&pool, &df7).await.is_empty());
    assert!(pending_for(&pool, &df8).await.is_empty());
}

#[tokio::test]
async fn invalid_or_unknown_denials_raise_nothing() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let df7 = task(&pool, project(&pool, "DF").await).await;
    let run_id = run(&pool, &df7, "running").await;

    for (host, port) in [
        ("", 443),
        ("*.evil.com", 443),
        ("pypi.org", 0),
        ("pypi.org", 70_000),
    ] {
        let result = raise_from_denial(&pool, run_id, host, port).await;
        assert!(
            matches!(result, Err(RequestError::Invalid(_))),
            "{host}:{port}"
        );
    }
    let unknown = raise_from_denial(&pool, RunId::new(), "pypi.org", 443)
        .await
        .unwrap();
    assert_eq!(unknown, DenialOutcome::UnknownRun);
    assert!(pending_for(&pool, &df7).await.is_empty());
}

/// Waits until a session of `pool` waits for an advisory lock.
async fn advisory_lock_waiter(pool: &PgPool) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_locks
                 WHERE locktype = 'advisory' AND NOT granted
                   AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            if waiting > 0 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the denial never waited for the allowlist lock");
}

#[tokio::test]
async fn a_denial_handled_while_an_entry_is_created_leaves_no_pending_request() {
    let pool = df_test_postgres::migrated_pool(&crate::MIGRATOR).await;
    let project_id = project(&pool, "DF").await;
    let df7 = task(&pool, project_id).await;
    let run_id = run(&pool, &df7, "running").await;
    let (principal, roles) = (human(), roles());
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };

    // The entry's transaction holds the allowlist lock while the denial is handled.
    let mut entry = pool.begin().await.unwrap();
    create_http_host_entry(
        &mut entry,
        HostScope::Project(project_id.as_uuid()),
        "pypi.org",
        443,
        actor,
        None,
    )
    .await
    .unwrap();
    let denial = tokio::spawn({
        let pool = pool.clone();
        async move { raise_from_denial(&pool, run_id, "pypi.org", 443).await }
    });
    advisory_lock_waiter(&pool).await;
    entry.commit().await.unwrap();
    assert_eq!(
        denial.await.unwrap().unwrap(),
        DenialOutcome::Raised(Raised::AlreadyGranted)
    );

    // A denial handled before the entry is settled by it.
    let raised = raise_from_denial(&pool, run_id, "files.pythonhosted.org", 443)
        .await
        .unwrap();
    assert!(
        matches!(
            raised,
            DenialOutcome::Raised(Raised::Request { new: true, .. })
        ),
        "{raised:?}"
    );
    add_project_entry(&pool, &human(), "DF", "files.pythonhosted.org", 443).await;
    assert!(pending_for(&pool, &df7).await.is_empty());
}
