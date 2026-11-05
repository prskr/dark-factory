//! Tests of the setting "Wake tasks on reviews and mentions" in the project form.

use axum::http::StatusCode;

use super::{REPO, assert_saved, audit, project_form};
use crate::testkit::{TestApp, seed, tag_around, text};

/// The project form of `DF` with `webhook_wake` set to `wake`, or without it for `None`.
fn with_wake(wake: Option<&str>) -> String {
    let replacement = wake.map_or_else(String::new, |wake| format!("&webhook_wake={wake}"));
    project_form("DF", REPO).replace("&webhook_wake=members", &replacement)
}

async fn stored_wake(app: &TestApp) -> String {
    sqlx::query_scalar("SELECT webhook_wake FROM projects WHERE key = 'DF'")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn the_new_project_form_offers_the_wake_setting_with_members_selected() {
    let app = TestApp::new().await;
    let html = text(app.get_htmx("/projects/new", &app.login().await).await).await;

    assert!(
        html.contains("<label for=\"webhook_wake\" class=\"block text-sm font-medium text-muted\">Wake tasks on reviews and mentions</label>"),
        "{html}"
    );
    let select = tag_around(&html, "id=\"webhook_wake\"");
    assert!(select.contains("name=\"webhook_wake\""), "{select}");
    for option in [
        "<option value=\"none\">Nobody</option>",
        "<option value=\"members\" selected>Members with write access</option>",
        "<option value=\"anyone\">Anyone</option>",
    ] {
        assert!(html.contains(option), "{option}: {html}");
    }
    assert!(
        html.contains("Reviews and @mentions of the bot account wake the task. Merged or closed PRs and failed checks apply regardless."),
        "{html}"
    );
}

#[tokio::test]
async fn the_wake_setting_is_stored_audited_and_shown_on_the_next_edit() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    let response = app
        .post("/projects/DF", &with_wake(Some("anyone")), &cookie)
        .await;

    assert_saved(&response);
    assert_eq!(stored_wake(&app).await, "anyone");
    let audit = audit(&app, "project.update").await;
    assert_eq!(audit[0]["before"]["webhook_wake"], "members");
    assert_eq!(audit[0]["after"]["webhook_wake"], "anyone");
    let edit = text(app.get_htmx("/projects/DF/edit", &cookie).await).await;
    assert!(
        edit.contains("<option value=\"anyone\" selected>Anyone</option>"),
        "{edit}"
    );
}

#[tokio::test]
async fn a_new_project_stores_the_chosen_wake_setting() {
    let app = TestApp::new().await;
    let response = app
        .post("/projects", &with_wake(Some("none")), &app.login().await)
        .await;

    assert_saved(&response);
    assert_eq!(stored_wake(&app).await, "none");
}

#[tokio::test]
async fn an_unknown_or_missing_wake_setting_is_rejected() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    for body in [with_wake(Some("everyone")), with_wake(None)] {
        let response = app.post("/projects/DF", &body, &cookie).await;

        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
        assert!(
            text(response).await.contains("Choose who wakes tasks."),
            "{body}"
        );
        assert_eq!(stored_wake(&app).await, "members");
    }
}
