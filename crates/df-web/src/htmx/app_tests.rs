//! The error answers through the real router: guards, handlers and the middleware together.
//! They need a PostgreSQL container (testcontainers).

use axum::http::StatusCode;

use crate::testkit::{TestApp, assert_toast, header_str, text};

#[tokio::test]
async fn an_action_on_an_unknown_task_answers_a_toast_with_the_error_status() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let response = app.delete("/tasks/DF-404/messages/abc", &cookie).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(header_str(&response, "hx-reswap"), "none");
    assert_eq!(header_str(&response, "hx-push-url"), "false");
    let body = text(response).await;
    assert_toast(&body);
    assert!(
        body.contains("404 Not found") && body.contains("The task does not exist."),
        "{body}"
    );
    assert!(!body.contains("<html"), "{body}");
}

#[tokio::test]
async fn a_page_load_of_an_unknown_task_shows_the_error_page() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let response = app.get("/tasks/DF-404", &cookie).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = text(response).await;
    assert!(
        body.contains("<html") && body.contains("404 Not found"),
        "{body}"
    );
    assert!(
        body.contains(">Home<") && body.contains("Log out"),
        "{body}"
    );
    assert!(!body.contains("<hx-partial"), "{body}");
}

#[tokio::test]
async fn a_guard_refusal_of_an_htmx_request_answers_a_toast() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let response = app.get_htmx("/projects/DF/subscriptions", &cookie).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = text(response).await;
    assert_toast(&body);
    assert!(body.contains("403") && body.contains("`admin`"), "{body}");
}
