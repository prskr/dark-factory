//! Tests of the "Branches" group of the project's Security tab: the branch rewrite policy and the
//! agent branches (`configuration-management`, "Projects"; design D10).

use axum::http::StatusCode;

use super::{REPO, audit, project_form};
use crate::testkit::{TestApp, form, open_tab, seed, tag_around, text};

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

async fn stored(app: &TestApp) -> (String, Vec<String>) {
    sqlx::query_as(
        "SELECT branch_rewrite_policy, agent_branch_globs FROM projects WHERE key = 'DF'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn the_security_tab_has_the_groups_push_checks_and_branches() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let html = text(
        app.get("/projects/DF?tab=security", &app.login().await)
            .await,
    )
    .await;

    let panel = &html[html.find("id=\"panel-security\"").unwrap()..];
    let push = panel
        .find(">Push checks</h3>")
        .expect("Push checks heading");
    let branches = panel.find(">Branches</h3>").expect("Branches heading");
    let policy = panel.find("id=\"branch_rewrite_policy\"").unwrap();
    assert!(
        push < panel.find("id=\"protected_globs\"").unwrap()
            && panel.find("id=\"linear_history\"").unwrap() < branches
            && branches < policy
            && policy < panel.find("id=\"agent_branch_globs\"").unwrap(),
        "{panel}"
    );
    let section = tag_around(panel, "aria-labelledby=\"group-branches\"");
    assert!(
        section.contains("border-t"),
        "divider above the group: {section}"
    );
}

#[tokio::test]
async fn the_rewrite_policy_select_defaults_to_with_lease_and_explains_it() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let html = text(
        app.get("/projects/DF?tab=security", &app.login().await)
            .await,
    )
    .await;

    let select = &html[html.find("id=\"branch_rewrite_policy\"").unwrap()..];
    let select = &select[..select.find("</select>").unwrap()];
    assert!(
        select.contains("<option value=\"never\">Never</option>")
            && select
                .contains("<option value=\"with-lease\" selected>With lease (default)</option>"),
        "{select}"
    );
    assert!(html.contains(">Branch rewrites</label>"), "{html}");
    assert!(
        html.contains("With lease, agents may rebase their task branch; commits pushed by others in the meantime are never overwritten by the runtime."),
        "{html}"
    );
}

#[tokio::test]
async fn changing_the_rewrite_policy_is_stored_and_audited_with_before_and_after() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    let response = app
        .post(
            "/projects/DF",
            &edit_body(&[("branch_rewrite_policy", "never")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(stored(&app).await.0, "never");
    let updated = audit(&app, "project.update").await;
    assert_eq!(updated[0]["before"]["branch_rewrite_policy"], "with-lease");
    assert_eq!(updated[0]["after"]["branch_rewrite_policy"], "never");
    let html = text(app.get("/projects/DF?tab=security", &cookie).await).await;
    assert!(
        html.contains("<option value=\"never\" selected>Never</option>"),
        "{html}"
    );
}

#[tokio::test]
async fn an_unknown_rewrite_policy_is_rejected_on_the_security_tab() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    let response = app
        .post(
            "/projects/DF",
            &edit_body(&[("branch_rewrite_policy", "always")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert_eq!(open_tab(&html), "security");
    assert_eq!(stored(&app).await.0, "with-lease");
}

#[tokio::test]
async fn a_new_project_gets_the_default_rewrite_policy_and_agent_branches() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let response = app
        .post("/projects", &project_form("DF", REPO), &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        stored(&app).await,
        (
            "with-lease".to_owned(),
            vec!["renovate/**".to_owned(), "dependabot/**".to_owned()]
        )
    );
}

#[tokio::test]
async fn the_agent_branches_textarea_shows_the_default_globs_one_per_line_with_its_help() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let html = text(
        app.get("/projects/DF?tab=security", &app.login().await)
            .await,
    )
    .await;

    assert!(html.contains(">Agent branches</label>"), "{html}");
    let textarea = &html[html.find("id=\"agent_branch_globs\"").unwrap()..];
    assert!(
        textarea.contains(">renovate/**\ndependabot/**</textarea>"),
        "{textarea}"
    );
    assert!(
        html.contains("One glob per line. Existing branches agents may start tasks on with create_task. Board members can pick any branch in the composer."),
        "{html}"
    );
}

#[tokio::test]
async fn agent_branches_are_stored_one_glob_per_line_and_audited_with_before_and_after() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    let response = app
        .post(
            "/projects/DF",
            &edit_body(&[("agent_branch_globs", "renovate/**\r\n\r\n  agent/**  \n")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(stored(&app).await.1, ["renovate/**", "agent/**"]);
    let updated = audit(&app, "project.update").await;
    assert_eq!(
        updated[0]["before"]["agent_branch_globs"],
        serde_json::json!(["renovate/**", "dependabot/**"])
    );
    assert_eq!(
        updated[0]["after"]["agent_branch_globs"],
        serde_json::json!(["renovate/**", "agent/**"])
    );
}

#[tokio::test]
async fn an_empty_agent_branches_textarea_stores_no_agent_branches() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    app.post(
        "/projects/DF",
        &edit_body(&[("agent_branch_globs", "")]),
        &cookie,
    )
    .await;

    assert!(stored(&app).await.1.is_empty());
}

#[tokio::test]
async fn a_form_without_the_agent_branches_field_keeps_the_stored_globs() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    app.post("/projects/DF", &edit_body(&[]), &cookie).await;

    assert_eq!(stored(&app).await.1, ["renovate/**", "dependabot/**"]);
}

#[tokio::test]
async fn an_invalid_agent_branch_glob_names_its_line_reopens_the_tab_and_stores_nothing() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    let response = app
        .post(
            "/projects/DF",
            &edit_body(&[("agent_branch_globs", "renovate/**\nrenovate/[")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert_eq!(open_tab(&html), "security");
    let textarea = tag_around(&html, "id=\"agent_branch_globs\"");
    assert!(
        textarea.contains("aria-invalid=\"true\"")
            && textarea
                .contains("aria-describedby=\"agent_branch_globs-error agent_branch_globs-hint\""),
        "{textarea}"
    );
    assert!(
        html.contains("Line 2: &quot;renovate/[&quot; is not a valid glob"),
        "{html}"
    );
    assert!(
        html.contains(">renovate/**\nrenovate/[</textarea>"),
        "{html}"
    );
    assert_eq!(stored(&app).await.1, ["renovate/**", "dependabot/**"]);
}
