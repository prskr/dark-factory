//! Tests of the credential reference forms and list.

use axum::http::StatusCode;

use super::{assert_saved, audit};
use crate::testkit::{TestApp, form, text};

#[tokio::test]
async fn credential_form_has_no_value_field() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get_htmx("/credentials/new", &cookie).await).await;
    let form_html = html.split("<form").nth(1).expect("form");
    let controls: Vec<&str> = form_html
        .split(['<'])
        .filter(|tag| {
            tag.starts_with("input ") || tag.starts_with("textarea") || tag.starts_with("select")
        })
        .map(|tag| {
            tag.split("name=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .unwrap_or("")
        })
        .collect();
    assert_eq!(controls, ["name", "secret_name", "secret_key"], "{html}");
    assert!(!html.contains("type=\"password\""));
}

#[tokio::test]
async fn credential_reference_is_created_listed_and_audited() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let body = form(&[
        ("name", "claude-subscription"),
        ("secret_name", "claude"),
        ("secret_key", "token"),
    ]);
    let response = app.post("/credentials", &body, &cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_saved(&response);

    let row: (String, String, String) =
        sqlx::query_as("SELECT name, secret_name, secret_key FROM credential_refs")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        row,
        (
            "claude-subscription".into(),
            "claude".into(),
            "token".into()
        )
    );
    let list = text(app.get("/credentials", &cookie).await).await;
    assert!(list.contains("claude-subscription") && list.contains("claude"));
    assert_eq!(
        audit(&app, "credential_ref.create").await[0]["after"]["secret_key"],
        "token"
    );

    // Edit: change the key.
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM credential_refs")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let edited = form(&[
        ("name", "claude-subscription"),
        ("secret_name", "claude"),
        ("secret_key", "oauth"),
    ]);
    app.post(&format!("/credentials/{id}"), &edited, &cookie)
        .await;
    let updates = audit(&app, "credential_ref.update").await;
    assert_eq!(
        (
            updates[0]["before"]["secret_key"].clone(),
            updates[0]["after"]["secret_key"].clone()
        ),
        ("token".into(), "oauth".into())
    );
}

#[tokio::test]
async fn invalid_credential_references_are_not_stored() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let body = form(&[
        ("name", ""),
        ("secret_name", "Not Valid"),
        ("secret_key", "a b"),
    ]);
    let response = app.post("/credentials", &body, &cookie).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("Name is required.")
            && html.contains("Kubernetes name")
            && html.contains("letters, digits")
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM credential_refs")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
