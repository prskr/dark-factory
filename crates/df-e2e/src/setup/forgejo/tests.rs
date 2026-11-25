//! `setup forgejo` against a wiremock Forgejo: a fresh instance, a second run, a missing repository.

use serde_json::json;
use wiremock::matchers::{basic_auth, body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{Tokens, run};
use crate::data;

const WEBHOOK_URL: &str = "http://cp.example/webhooks/forgejo";

/// Answers `status` to `verb path`, expecting `times` such requests.
async fn mock(server: &MockServer, verb: &str, p: &str, status: u16, times: u64) {
    Mock::given(method(verb))
        .and(path(p))
        .respond_with(ResponseTemplate::new(status).set_body_json(json!({})))
        .expect(times)
        .mount(server)
        .await;
}

/// Token creation of `user` as that user, answering `token`.
async fn mock_token(server: &MockServer, user: &str, password: &str, token: &str) {
    Mock::given(method("POST"))
        .and(path(format!("/api/v1/users/{user}/tokens")))
        .and(basic_auth(user, password))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "sha1": token })))
        .expect(1)
        .mount(server)
        .await;
}

/// The user part: the bot and the reviewer, who `exist` already (a kept Forgejo: the lookup finds
/// them, no user is created, the old token is deleted) or not (a fresh one: both are created and
/// the token delete answers 404), and the `tokens` the token creation answers (bot, reviewer).
async fn mock_users(server: &MockServer, exist: bool, tokens: [&str; 2]) {
    let (lookup, token_delete, created) = if exist { (200, 204, 0) } else { (404, 404, 2) };
    for user in ["factory", "reviewer"] {
        mock(server, "GET", &format!("/api/v1/users/{user}"), lookup, 1).await;
        mock(
            server,
            "DELETE",
            &format!("/api/v1/users/{user}/tokens/e2e"),
            token_delete,
            1,
        )
        .await;
    }
    mock(server, "POST", "/api/v1/admin/users", 201, created).await;
    // The persona's commit email, so that Forgejo can show its signed commits as verified.
    Mock::given(method("POST"))
        .and(path("/api/v1/user/emails"))
        .and(basic_auth("factory", data::FORGE_OWNER_PASSWORD))
        .and(body_partial_json(
            json!({ "emails": [data::PERSONA_EMAIL] }),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!([])))
        .expect(1)
        .mount(server)
        .await;
    mock_token(server, "factory", data::FORGE_OWNER_PASSWORD, tokens[0]).await;
    mock_token(server, "reviewer", data::FORGE_REVIEWER_PASSWORD, tokens[1]).await;
}

/// The repository part, which is the same on a fresh and on a kept Forgejo; `deletes` is what the
/// delete of each repository of the suite answers.
async fn mock_repositories(server: &MockServer, deletes: [u16; data::FORGE_REPOSITORIES.len()]) {
    for (repository, deleted) in data::FORGE_REPOSITORIES.into_iter().zip(deletes) {
        mock(
            server,
            "DELETE",
            &format!("/api/v1/repos/factory/{repository}"),
            deleted,
            1,
        )
        .await;
        mock(
            server,
            "PUT",
            &format!("/api/v1/repos/factory/{repository}/collaborators/reviewer"),
            204,
            1,
        )
        .await;
        Mock::given(method("POST"))
            .and(path(format!("/api/v1/repos/factory/{repository}/hooks")))
            .and(body_partial_json(json!({
                "config": { "url": WEBHOOK_URL, "secret": data::WEBHOOK_SECRET },
            })))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(server)
            .await;
    }
    mock(server, "POST", "/api/v1/user/repos", 201, 2).await;
    Mock::given(method("POST"))
        .and(path("/api/v1/repos/factory/demo/issues"))
        .and(body_partial_json(
            json!({ "title": data::FORGE_ISSUE_TITLE }),
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "number": 1 })))
        .expect(1)
        .mount(server)
        .await;
    mock(
        server,
        "POST",
        "/api/v1/repos/factory/demo/contents/secret/value.txt",
        201,
        1,
    )
    .await;
}

#[tokio::test]
async fn a_fresh_forgejo_gets_users_tokens_and_repositories() {
    let server = MockServer::start().await;
    mock_users(&server, false, ["bot-token", "reviewer-token"]).await;
    mock_repositories(&server, [404, 404]).await;

    let tokens = run(&server.uri(), "admin", "pw", WEBHOOK_URL)
        .await
        .unwrap();

    assert_eq!(
        tokens,
        Tokens {
            forgejo_token: "bot-token".into(),
            reviewer_token: "reviewer-token".into(),
        }
    );
}

#[tokio::test]
async fn a_second_run_keeps_the_users_and_replaces_their_tokens() {
    let server = MockServer::start().await;
    mock_users(&server, true, ["new-bot", "new-reviewer"]).await;
    mock_repositories(&server, [204, 204]).await;

    let tokens = run(&server.uri(), "admin", "pw", WEBHOOK_URL)
        .await
        .unwrap();

    assert_eq!(tokens.forgejo_token, "new-bot");
    assert_eq!(tokens.reviewer_token, "new-reviewer");
}

#[tokio::test]
async fn a_missing_repository_does_not_stop_the_reset_and_both_are_created_again() {
    let server = MockServer::start().await;
    mock_users(&server, true, ["bot", "rev"]).await;
    // `demo` is deleted, `existing-branch` is gone already; both are created again (the
    // expectation of two creations is checked when the server drops).
    mock_repositories(&server, [204, 404]).await;

    run(&server.uri(), "admin", "pw", WEBHOOK_URL)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_refused_request_fails_the_setup_without_tokens() {
    let server = MockServer::start().await;
    mock(&server, "GET", "/api/v1/users/factory", 500, 1).await;

    let error = run(&server.uri(), "admin", "pw", WEBHOOK_URL)
        .await
        .unwrap_err();

    assert_eq!(error.to_string(), "GET users/factory: HTTP 500");
}
