//! Tests of the configuration pages (projects, credentials, personas): shared fixtures; the tests
//! are grouped by topic in the submodules.

mod adapter_command;
mod branches;
mod credentials;
mod forge_hints;
mod forms;
mod integrations_tab;
mod lists;
mod personas;
mod project_page;
mod projects;
mod webhook_wake;

use axum::http::StatusCode;

use crate::testkit::{TestApp, form, header_str, new_task, seed, text};

pub(super) async fn audit(app: &TestApp, action: &str) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT details FROM audit_log WHERE action = $1 ORDER BY id")
        .bind(action)
        .fetch_all(&app.pool)
        .await
        .unwrap()
}

/// Creating a project answers with a navigation to its settings page.
pub(super) fn assert_project_created(response: &axum::http::Response<axum::body::Body>) {
    assert_eq!(response.status(), StatusCode::OK);
    assert!(header_str(response, "hx-location").starts_with("/projects/"));
    assert_eq!(header_str(response, "hx-trigger"), "");
}

/// Saving a project's settings page answers with the form again, not with a closing modal.
pub(super) fn assert_project_saved(response: &axum::http::Response<axum::body::Body>) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(response, "hx-trigger"), "");
    assert_eq!(header_str(response, "hx-retarget"), "");
}

/// A successful save answers with the updated list for `#config-list` and closes the modal.
pub(super) fn assert_saved(response: &axum::http::Response<axum::body::Body>) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(response, "hx-retarget"), "#config-list");
    assert_eq!(header_str(response, "hx-reswap"), "outerHTML");
    assert_eq!(header_str(response, "hx-trigger"), "modal-close");
}

pub(super) fn project_form(key: &str, repo: &str) -> String {
    project_form_with_tracker(key, repo, "")
}

pub(super) fn project_form_with_tracker(key: &str, repo: &str, tracker: &str) -> String {
    form(&[
        ("key", key),
        ("name", "Dark Factory"),
        ("repo_url", repo),
        ("issue_tracker_url", tracker),
        ("image", "ghcr.io/example/sandbox:1"),
        ("base_branch", "main"),
        ("adapter_command", ""),
        ("forge_credential_id", ""),
        ("webhook_wake", "members"),
    ])
}

pub(super) async fn stored_adapter_command(app: &TestApp) -> Vec<String> {
    sqlx::query_scalar("SELECT adapter_command FROM projects WHERE key = 'DF'")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

pub(super) const REPO: &str = "https://git.example.org/peter/dark-factory.git";

pub(super) async fn stored_key(app: &TestApp) -> String {
    sqlx::query_scalar("SELECT key FROM projects")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

pub(super) fn credential_form_body(name: &str, secret: &str, key: &str) -> String {
    form(&[("name", name), ("secret_name", secret), ("secret_key", key)])
}

/// A project of the testkit seed with tasks in the given statuses.
pub(super) async fn project_with_tasks(app: &TestApp, statuses: &[&str]) {
    let seed = seed(&app.pool).await;
    for status in statuses {
        let task = new_task(&app.pool, &seed, "t").await;
        sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
            .bind(task.id.as_uuid())
            .bind(status)
            .execute(&app.pool)
            .await
            .unwrap();
    }
}

/// The project list of an app with `forges` and the projects `(key, repo_url, credential)`.
pub(super) async fn project_list(
    forges: df_core::routing::ForgeRoutes,
    projects: &[(&str, &str, bool)],
) -> String {
    let app = TestApp::with_forges(forges).await;
    let cookie = app.login().await;
    let secret = form(&[
        ("name", "forge"),
        ("secret_name", "forge"),
        ("secret_key", "token"),
    ]);
    app.post("/credentials", &secret, &cookie).await;
    let credential: uuid::Uuid = sqlx::query_scalar("SELECT id FROM credential_refs")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    for (key, repo, with_credential) in projects {
        let credential = if *with_credential {
            credential.to_string()
        } else {
            String::new()
        };
        let body = form(&[
            ("key", key),
            ("name", key),
            ("repo_url", repo),
            ("issue_tracker_url", ""),
            ("image", "img"),
            ("base_branch", "main"),
            ("forge_credential_id", &credential),
            ("webhook_wake", "members"),
        ]);
        app.post("/projects", &body, &cookie).await;
    }
    text(app.get("/projects", &cookie).await).await
}

/// The table row of project `key`.
pub(super) fn project_row(html: &str, key: &str) -> String {
    let at = html
        .find(&format!(">{key}</span></td>"))
        .unwrap_or_else(|| panic!("{key}: {html}"));
    let start = html[..at].rfind("<tr").unwrap();
    html[start..start + html[start..].find("</tr>").unwrap()].to_string()
}

pub(super) async fn forge_fragment(app: &TestApp, cookie: &str, repo: &str, issue: &str) -> String {
    let uri = format!(
        "/projects/forge?{}",
        form(&[("repo_url", repo), ("issue_tracker_url", issue)])
    );
    text(app.get_htmx(&uri, cookie).await).await
}
