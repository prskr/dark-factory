//! Tests of the "Integrations" tab of the project settings page: the group "Lifecycle events"
//! (`event-subscriptions`) and its place next to "Incoming webhook" (`forge-webhooks`).

use axum::http::StatusCode;

use super::project_form;
use crate::testkit::{TestApp, seed, tag_around, text};

#[tokio::test]
async fn an_admins_page_ends_with_a_tab_called_integrations_with_two_groups() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let html = text(app.get("/projects/DF", &app.login_admin().await).await).await;
    let order: Vec<usize> = [">General<", ">Push checks<", ">Integrations<"]
        .iter()
        .map(|label| html.find(label).expect(label))
        .collect();
    assert!(order.is_sorted(), "{html}");
    assert!(
        !html.contains(">Lifecycle events</button>"),
        "no such tab: {html}"
    );
    assert!(html.contains("id=\"panel-integrations\""), "{html}");
    let panel = &html[html.find("id=\"panel-integrations\"").unwrap()..];
    let lifecycle = panel.find("id=\"group-lifecycle-events\"").expect("group");
    let webhook = panel.find("id=\"group-incoming-webhook\"").expect("group");
    assert!(lifecycle < webhook, "{panel}");
    // The subscriptions load when the tab is first shown, inside the first group.
    let lazy = tag_around(panel, "hx-get=\"/projects/DF/subscriptions\"");
    assert!(lazy.contains("hx-trigger=\"intersect once\""), "{lazy}");
    assert!(panel.find("hx-get=\"/projects/DF/subscriptions\"").unwrap() < webhook);
    // A separator sits above the second group and none above the first.
    let second = tag_around(panel, "id=\"group-incoming-webhook\"");
    assert!(second.starts_with("<h3"), "{second}");
    assert!(
        panel.contains("border-t border-line pt-5 first:border-t-0"),
        "{panel}"
    );
}

#[tokio::test]
async fn a_board_members_page_has_no_integrations_tab() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let html = text(app.get("/projects/DF", &app.login().await).await).await;
    assert!(html.contains(">Push checks<"), "{html}");
    assert!(!html.contains("Integrations"), "{html}");
    assert!(!html.contains("Lifecycle events"), "{html}");
    assert!(!html.contains("/subscriptions"), "{html}");
}

#[tokio::test]
async fn a_rejected_project_save_keeps_the_integrations_tab_of_an_admin() {
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
    assert!(html.contains(">Integrations<"), "{html}");
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

#[tokio::test]
async fn a_subscription_created_in_the_tab_survives_leaving_the_page_without_saving() {
    let app = TestApp::new().await;
    let seeded = seed(&app.pool).await;
    let cookie = app.login_admin().await;

    let created = app
        .post(
            "/projects/DF/subscriptions",
            "url=https%3A%2F%2Fhooks.example.org%2Fx&events=sandbox.destroyed",
            &cookie,
        )
        .await;
    assert_eq!(created.status(), StatusCode::OK);

    // The page is opened again without a save in between.
    let page = text(app.get("/projects/DF", &cookie).await).await;
    assert!(page.contains("value=\"Dark Factory\""), "{page}");
    let region = text(app.get_htmx("/projects/DF/subscriptions", &cookie).await).await;
    assert!(region.contains("https://hooks.example.org/x"), "{region}");
    let subscriptions = df_db::event_subscriptions::list(&app.pool, seeded.project)
        .await
        .unwrap();
    assert_eq!(subscriptions.len(), 1);
}
