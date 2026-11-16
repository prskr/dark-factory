//! The Forgejo client against a wiremock server: method, path, authentication and body per call.

use serde_json::json;
use wiremock::matchers::{basic_auth, body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{Forgejo, ReviewEvent};

#[test]
fn a_review_event_is_sent_as_the_string_forgejo_expects() {
    assert_eq!(ReviewEvent::Approve.as_str(), "APPROVED");
    assert_eq!(ReviewEvent::RequestChanges.as_str(), "REQUEST_CHANGES");
    assert_eq!(ReviewEvent::Comment.as_str(), "COMMENT");
}

fn admin(server: &MockServer) -> Forgejo {
    Forgejo::with_password(&server.uri(), "admin", "secret").unwrap()
}

#[tokio::test]
async fn a_token_client_sends_the_token_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/factory/demo/branches/main"))
        .and(header("Authorization", "token t0ken"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let forgejo = Forgejo::new(&server.uri(), "t0ken").unwrap();
    assert!(forgejo.branch_exists("main").await.unwrap());
}

#[tokio::test]
async fn a_password_client_sends_basic_auth() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/factory"))
        .and(basic_auth("admin", "secret"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    assert!(admin(&server).user_exists("factory").await.unwrap());
}

#[tokio::test]
async fn a_missing_user_does_not_exist() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/factory"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    assert!(!admin(&server).user_exists("factory").await.unwrap());
}

#[tokio::test]
async fn a_user_is_created_with_a_password_that_needs_no_change() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/admin/users"))
        .and(basic_auth("admin", "secret"))
        .and(body_json(json!({
            "username": "factory",
            "password": "pw",
            "email": "factory@example.com",
            "must_change_password": false,
        })))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;
    admin(&server)
        .create_user("factory", "pw", "factory@example.com")
        .await
        .unwrap();
}

#[tokio::test]
async fn a_token_is_deleted_and_a_missing_one_is_fine() {
    for status in [204, 404] {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/api/v1/users/factory/tokens/e2e"))
            .and(basic_auth("admin", "secret"))
            .respond_with(ResponseTemplate::new(status))
            .expect(1)
            .mount(&server)
            .await;
        admin(&server).delete_token("factory", "e2e").await.unwrap();
    }
}

#[tokio::test]
async fn a_token_is_created_with_all_scopes_and_returned() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/users/factory/tokens"))
        .and(body_json(json!({ "name": "e2e", "scopes": ["all"] })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha1": "abc123" })))
        .expect(1)
        .mount(&server)
        .await;
    let token = admin(&server).create_token("factory", "e2e").await.unwrap();
    assert_eq!(token, "abc123");
}

#[tokio::test]
async fn a_repository_is_created_with_an_initial_commit_on_main() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/user/repos"))
        .and(body_json(
            json!({ "name": "demo", "auto_init": true, "default_branch": "main" }),
        ))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;
    admin(&server).create_repository("demo").await.unwrap();
}

#[tokio::test]
async fn a_missing_repository_is_deleted_without_error() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/repos/factory/demo"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    admin(&server).delete_repository("demo").await.unwrap();
}

#[tokio::test]
async fn a_collaborator_gets_write_access() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/repos/factory/demo/collaborators/reviewer"))
        .and(body_json(json!({ "permission": "write" })))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    admin(&server).add_collaborator("reviewer").await.unwrap();
}

#[tokio::test]
async fn a_webhook_posts_the_events_signed_with_the_secret() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/repos/factory/demo/hooks"))
        .and(body_json(json!({
            "type": "forgejo",
            "active": true,
            "events": ["issue_comment"],
            "config": { "url": "http://cp/hook", "content_type": "json", "secret": "s3" },
        })))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;
    admin(&server)
        .create_webhook("http://cp/hook", "s3", &["issue_comment"])
        .await
        .unwrap();
}

#[tokio::test]
async fn a_failed_request_names_method_path_and_status_but_not_the_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/admin/users"))
        .respond_with(ResponseTemplate::new(500).set_body_string("leaked-token-value"))
        .mount(&server)
        .await;
    let error = admin(&server)
        .create_user("factory", "pw", "f@example.com")
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(error, "POST admin/users: HTTP 500");
}
