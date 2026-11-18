//! Tests of the project's subtask merge policy in the "Branches" group of the Security tab
//! (`configuration-management`, "Subtask merge policy of a project").

use axum::http::StatusCode;

use super::{REPO, audit, project_form};
use crate::testkit::{TestApp, form, open_tab, seed, text};

/// The edit form of project `DF` from the seed with the given extra fields.
fn edit_body(extra: &[(&str, &str)]) -> String {
    let mut pairs = vec![
        ("key", "DF"),
        ("name", "Dark Factory"),
        ("repo_url", REPO),
        ("issue_tracker_url", ""),
        ("image", "img:latest"),
        ("base_branch", "main"),
        ("adapter_command", ""),
        ("forge_credential_id", ""),
        ("webhook_wake", "members"),
    ];
    pairs.extend_from_slice(extra);
    form(&pairs)
}

async fn stored(app: &TestApp) -> String {
    sqlx::query_scalar("SELECT subtask_merge FROM projects WHERE key = 'DF'")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

// Scenario "New project".
#[tokio::test]
async fn a_new_project_leaves_subtask_merges_to_humans() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let response = app
        .post("/projects", &project_form("DF", REPO), &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(stored(&app).await, "human");
}

#[tokio::test]
async fn the_select_offers_both_values_and_explains_each_in_one_line() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let html = text(
        app.get("/projects/DF?tab=security", &app.login().await)
            .await,
    )
    .await;

    let select = &html[html.find("id=\"subtask_merge\"").unwrap()..];
    let select = &select[..select.find("</select>").unwrap()];
    assert!(
        select.contains("<option value=\"human\" selected>Human (default)</option>")
            && select.contains("<option value=\"parent-agent\">Parent agent</option>"),
        "{select}"
    );
    assert!(html.contains(">Subtask merges</label>"), "{html}");
    assert!(
        html.contains("Human: board members merge the pull requests of subtasks."),
        "{html}"
    );
    assert!(
        html.contains("Parent agent: the agent of the parent task may also merge them into its own task branch."),
        "{html}"
    );
}

// Scenario "Opting in".
#[tokio::test]
async fn opting_in_is_stored_and_audited_with_the_old_and_new_value() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    let response = app
        .post(
            "/projects/DF",
            &edit_body(&[("subtask_merge", "parent-agent")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(stored(&app).await, "parent-agent");
    let updated = audit(&app, "project.update").await;
    assert_eq!(updated[0]["before"]["subtask_merge"], "human");
    assert_eq!(updated[0]["after"]["subtask_merge"], "parent-agent");
    let html = text(app.get("/projects/DF?tab=security", &cookie).await).await;
    assert!(
        html.contains("<option value=\"parent-agent\" selected>Parent agent</option>"),
        "{html}"
    );
}

#[tokio::test]
async fn an_unknown_value_is_rejected_on_the_security_tab() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    let response = app
        .post(
            "/projects/DF",
            &edit_body(&[("subtask_merge", "anyone")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(open_tab(&text(response).await), "security");
    assert_eq!(stored(&app).await, "human");
}

#[tokio::test]
async fn a_form_without_the_field_keeps_the_stored_value() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    app.post(
        "/projects/DF",
        &edit_body(&[("subtask_merge", "parent-agent")]),
        &cookie,
    )
    .await;

    app.post("/projects/DF", &edit_body(&[]), &cookie).await;

    assert_eq!(stored(&app).await, "parent-agent");
}
