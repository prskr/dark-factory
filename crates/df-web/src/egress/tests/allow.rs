//! The "allow" action of the traffic page: dialog and entry creation.

use std::time::Duration;

use axum::http::StatusCode;
use df_core::{PersonaId, Principal, RequestId, TaskId};
use df_db::network_access::NETWORK_ACCESS;
use df_db::requests::{
    ApprovalTypes, Raise, Raised, RequestState, Scope, get_request, raise_approval,
};
use df_db::{EntryKind, Notification};
use df_proto::gateway::v1::FlowEvent;
use serde_json::json;

use super::{app_with_egress, denied};
use crate::testkit::{TestApp, form, header_str, new_task, seed, text};

/// A `not-allowlisted` denial of a run of `task`.
fn denied_for(task: &str, host: &str) -> FlowEvent {
    FlowEvent {
        run_id: format!("run-{task}"),
        task_key: task.into(),
        ..denied(host, "DF", 1)
    }
}

/// The dialog URL of the first allow action on the page.
fn allow_href(page: &str) -> String {
    let start = page.find("hx-get=\"/egress/log/allow?").unwrap() + "hx-get=\"".len();
    page[start..start + page[start..].find('"').unwrap()].replace("&amp;", "&")
}

/// The dialog of the first allow action of the log page.
async fn open_dialog(app: &TestApp, cookie: &str) -> String {
    let page = text(app.get("/egress/log", cookie).await).await;
    let response = app.get_htmx(&allow_href(&page), cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    text(response).await
}

async fn allow(app: &TestApp, cookie: &str, fields: &[(&str, &str)]) -> axum::response::Response {
    app.post("/egress/log/allow", &form(fields), cookie).await
}

/// A pending `network-access` request of the task's agent for `host:port`.
async fn network_access(app: &TestApp, task: TaskId, host: &str, port: u16) -> RequestId {
    let agent = Principal::agent(PersonaId::new(), "Linus", None, task);
    let from = Raise {
        task_id: task,
        run_id: None,
        actor: &agent,
        blocking: false,
    };
    let raised = raise_approval(
        &app.pool,
        &ApprovalTypes::builtin(),
        from,
        NETWORK_ACCESS,
        json!({ "host": host, "port": port }),
        "pip install",
    )
    .await
    .unwrap();
    let Raised::Request { request, .. } = raised else {
        panic!("already granted")
    };
    request.id
}

#[tokio::test]
async fn allowing_a_row_for_its_project_creates_an_announced_and_audited_entry() {
    // Scenarios "Allow for the project" and "Action on the table row".
    // The first task of a fresh database is DF-1.
    let app = app_with_egress(vec![denied_for("DF-1", "files.pythonhosted.org")]).await;
    let seed = seed(&app.pool).await;
    let task_row = new_task(&app.pool, &seed, "Fix CI").await;
    let task = task_row.key.to_string();
    assert_eq!(task, "DF-1");
    let cookie = app.login().await;
    let dialog = open_dialog(&app, &cookie).await;
    assert!(
        dialog.contains("<dialog") && dialog.contains("Allow egress"),
        "{dialog}"
    );
    assert!(
        dialog.contains("value=\"files.pythonhosted.org:443\" readonly"),
        "{dialog}"
    );
    assert!(
        dialog.contains("value=\"task\" checked>Task DF-1<"),
        "{dialog}"
    );
    assert!(
        dialog.contains(">Project DF<") && dialog.contains(">Everywhere<"),
        "{dialog}"
    );
    assert!(dialog.contains(">Create rule<"));

    let notifier = df_db::Notifier::start(&app.pool).await.unwrap();
    let mut notifications = notifier.subscribe();
    let response = allow(
        &app,
        &cookie,
        &[
            ("host", "files.pythonhosted.org"),
            ("port", "443"),
            ("task", "DF-1"),
            ("project", "DF"),
            ("scope", "project"),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-trigger"), "modal-close");
    assert_eq!(header_str(&response, "hx-retarget"), "#allow-toast");
    let toast = text(response).await;
    assert!(
        toast.contains("Rule created") && toast.contains(" is allowed for project DF."),
        "{toast}"
    );
    assert!(toast.contains("href=\"/egress/allowlist?highlight=files.pythonhosted.org%3A443\""));

    let entries = df_db::list_entries(&app.pool).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        (
            entries[0].project_key.as_deref(),
            entries[0].task_key.as_deref()
        ),
        (Some("DF"), None)
    );
    assert_eq!(
        entries[0].kind,
        EntryKind::HttpHost {
            host: "files.pythonhosted.org".into(),
            port: 443
        }
    );
    let announced = tokio::time::timeout(Duration::from_secs(5), notifications.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(announced, Notification::EgressPolicy);
    let actions: Vec<String> =
        sqlx::query_scalar("SELECT action FROM audit_log WHERE resource_type = 'egress_allowlist'")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(actions, ["egress_allowlist.create"]);
}

#[tokio::test]
async fn allowing_a_row_settles_the_pending_request_and_covers_the_task() {
    let app = app_with_egress(vec![denied_for("DF-1", "files.pythonhosted.org")]).await;
    let seed = seed(&app.pool).await;
    let task_row = new_task(&app.pool, &seed, "Fix CI").await;
    let request = network_access(&app, task_row.id, "files.pythonhosted.org", 443).await;
    let cookie = app.login().await;
    let response = allow(
        &app,
        &cookie,
        &[
            ("host", "files.pythonhosted.org"),
            ("port", "443"),
            ("task", "DF-1"),
            ("project", "DF"),
            ("scope", "project"),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    // The entry settles the task's pending request for the host with its scope.
    let request = get_request(&app.pool, request).await.unwrap().unwrap();
    assert_eq!(
        (request.state, request.scope()),
        (RequestState::Approved, Some(Scope::Project))
    );
    // The next attempt of the task's run succeeds: the rule is in its effective allowlist.
    let mut tx = app.pool.begin().await.unwrap();
    let covered = df_db::egress::allowlist_covers(
        &mut tx,
        task_row.id.as_uuid(),
        "files.pythonhosted.org",
        443,
    )
    .await
    .unwrap();
    assert!(covered);
}

#[tokio::test]
async fn the_action_allows_exactly_the_denied_host_never_a_wildcard() {
    // Scenario "Wildcard not offered".
    let app = app_with_egress(vec![denied_for("DF-7", "static.crates.io")]).await;
    let cookie = app.login().await;
    let dialog = open_dialog(&app, &cookie).await;
    assert!(
        dialog.contains("name=\"host\" value=\"static.crates.io\""),
        "{dialog}"
    );
    assert!(!dialog.contains("*.crates.io"), "{dialog}");

    let wildcard = allow(
        &app,
        &cookie,
        &[
            ("host", "*.crates.io"),
            ("port", "443"),
            ("scope", "instance"),
        ],
    )
    .await;
    assert_eq!(wildcard.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(wildcard).await.contains("not a wildcard"));
    assert!(df_db::list_entries(&app.pool).await.unwrap().is_empty());

    let exact = allow(
        &app,
        &cookie,
        &[
            ("host", "static.crates.io"),
            ("port", "443"),
            ("scope", "instance"),
        ],
    )
    .await;
    assert_eq!(exact.status(), StatusCode::OK);
    assert!(text(exact).await.contains(" is allowed everywhere."));
    let entries = df_db::list_entries(&app.pool).await.unwrap();
    assert_eq!(
        (&entries[0].kind, entries[0].project_key.as_deref()),
        (
            &EntryKind::HttpHost {
                host: "static.crates.io".into(),
                port: 443
            },
            None
        )
    );
}

#[tokio::test]
async fn the_server_checks_the_scope_and_creates_task_entries() {
    let app = app_with_egress(vec![]).await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "Fix CI").await.key.to_string();
    let cookie = app.login().await;
    for (scope, task_key, project, message) in [
        ("task", "DF-99", "DF", "Unknown task."),
        ("project", "", "OPS", "Unknown project."),
        ("run", task.as_str(), "DF", "Choose a scope."),
    ] {
        let response = allow(
            &app,
            &cookie,
            &[
                ("host", "pypi.org"),
                ("port", "443"),
                ("task", task_key),
                ("project", project),
                ("scope", scope),
            ],
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{scope}"
        );
        assert!(text(response).await.contains(message), "{scope}");
    }
    assert!(df_db::list_entries(&app.pool).await.unwrap().is_empty());

    let response = allow(
        &app,
        &cookie,
        &[
            ("host", "pypi.org"),
            ("port", "443"),
            ("task", &task),
            ("project", "DF"),
            ("scope", "task"),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let entries = df_db::list_entries(&app.pool).await.unwrap();
    assert_eq!(entries[0].task_key.as_deref(), Some(task.as_str()));
}

#[tokio::test]
async fn invalid_ports_are_form_errors() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    for port in ["0", "65536", "https"] {
        let response = allow(
            &app,
            &cookie,
            &[("host", "pypi.org"), ("port", port), ("scope", "instance")],
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{port}"
        );
        let body = text(response).await;
        assert!(
            body.contains("id=\"allow-form\"")
                && body.contains("Must be a port between 1 and 65535."),
            "{port}: {body}"
        );
    }
    assert!(df_db::list_entries(&app.pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn an_existing_entry_is_a_form_error() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let fields = [("host", "pypi.org"), ("port", "443"), ("scope", "instance")];
    assert_eq!(allow(&app, &cookie, &fields).await.status(), StatusCode::OK);
    let again = allow(&app, &cookie, &fields).await;
    assert_eq!(again.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = text(again).await;
    assert!(
        body.contains("id=\"allow-form\"") && body.contains("this entry already exists"),
        "{body}"
    );
    assert_eq!(df_db::list_entries(&app.pool).await.unwrap().len(), 1);
}

#[tokio::test]
async fn the_dialog_is_a_modal_fragment() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let plain = app
        .get("/egress/log/allow?host=pypi.org&port=443", &cookie)
        .await;
    assert_eq!(plain.status(), StatusCode::SEE_OTHER);
    let dialog = text(
        app.get_htmx(
            "/egress/log/allow?host=pypi.org&port=443&project=DF",
            &cookie,
        )
        .await,
    )
    .await;
    assert!(
        dialog.contains("value=\"project\" checked") && !dialog.contains("value=\"task\""),
        "{dialog}"
    );
}
