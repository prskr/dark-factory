//! Tests of the "Lifecycle events" tab in the project edit form.

use axum::http::StatusCode;

use super::project_form;
use crate::testkit::{TestApp, seed, tag_around, text};

#[tokio::test]
async fn an_admins_edit_form_ends_with_a_lazily_loaded_lifecycle_events_tab() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let html = text(
        app.get_htmx("/projects/DF/edit", &app.login_admin().await)
            .await,
    )
    .await;
    let order: Vec<usize> = [">General<", ">Push checks<", ">Lifecycle events<"]
        .iter()
        .map(|label| html.find(label).expect(label))
        .collect();
    assert!(order.is_sorted(), "{html}");
    let panel = tag_around(&html, "hx-get=\"/projects/DF/subscriptions\"");
    assert!(panel.contains("hx-trigger=\"intersect once\""), "{panel}");
    assert!(html.contains("id=\"panel-lifecycle-events\""), "{html}");
}

#[tokio::test]
async fn a_board_members_edit_form_has_no_lifecycle_events_tab() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let html = text(app.get_htmx("/projects/DF/edit", &app.login().await).await).await;
    assert!(html.contains(">Push checks<"), "{html}");
    assert!(!html.contains("Lifecycle events"), "{html}");
    assert!(!html.contains("/subscriptions"), "{html}");
}

#[tokio::test]
async fn a_rejected_project_save_keeps_the_lifecycle_events_tab_of_an_admin() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let response = app
        .post(
            "/projects/DF",
            &project_form("DF", "http://insecure.example/o/r.git"),
            &app.login_admin().await,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(html.contains(">Lifecycle events<"), "{html}");
    assert!(!html.contains("subscription-form"), "{html}");
}

#[tokio::test]
async fn the_project_list_has_no_lifecycle_events_action() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    for cookie in [app.login_admin().await, app.login().await] {
        let html = text(app.get("/projects", &cookie).await).await;
        assert!(!html.contains("/subscriptions"), "{html}");
        assert!(!html.contains("Lifecycle events"), "{html}");
    }
}

#[tokio::test]
async fn saving_the_project_ignores_an_unsaved_subscription_form() {
    let app = TestApp::new().await;
    let seeded = seed(&app.pool).await;
    let body = format!(
        "{}&url=https%3A%2F%2Fhooks.example.org%2Fx&events=sandbox.destroyed",
        project_form("DF", "https://git.example.org/o/changed.git")
    );

    let response = app
        .post("/projects/DF", &body, &app.login_admin().await)
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let project = df_db::catalog::get_project_by_key(&app.pool, "DF")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(project.repo_url, "https://git.example.org/o/changed.git");
    let subscriptions = df_db::event_subscriptions::list(&app.pool, seeded.project)
        .await
        .unwrap();
    assert!(subscriptions.is_empty());
}
