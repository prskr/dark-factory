use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use crate::testkit::{TestApp, form, header_str, post_request, text};

fn query_param(url: &str, name: &str) -> String {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}

/// The `Cookie` header value for the `Set-Cookie` of a response.
fn cookie_of(response: &axum::http::Response<axum::body::Body>) -> String {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("df_session="))
        .and_then(|v| v.split(';').next())
        .unwrap_or_default()
        .to_string()
}

async fn session_count(app: &TestApp) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM web_sessions")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

/// Starts a login and returns the session cookie and the authorization URL.
async fn start_login(app: &TestApp, next: &str) -> (String, String) {
    let response = app.get(&format!("/login?next={next}"), "").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = header_str(&response, "location").to_string();
    (cookie_of(&response), location)
}

#[tokio::test]
async fn unauthenticated_pages_redirect_to_login_and_back() {
    let app = TestApp::new().await;
    let response = app.get("/tasks?x=1", "").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        header_str(&response, "location"),
        "/login?next=%2Ftasks%3Fx%3D1"
    );

    let (cookie, authorize) = start_login(&app, "%2Ftasks%3Fx%3D1").await;
    assert!(
        authorize.starts_with(&format!("{}/auth?", app.idp.server.uri())),
        "{authorize}"
    );
    assert_eq!(query_param(&authorize, "code_challenge_method"), "S256");
    assert_eq!(
        query_param(&authorize, "redirect_uri"),
        "https://factory.test/auth/callback"
    );
    assert_eq!(query_param(&authorize, "client_id"), "dark-factory");
    assert!(query_param(&authorize, "scope").contains("openid"));

    // The provider sends the browser back with the state; the login completes.
    app.idp
        .set(|s| s.nonce = Some(query_param(&authorize, "nonce")));
    let state = query_param(&authorize, "state");
    let callback = app
        .get(
            &format!("/auth/callback?code=good-code&state={state}"),
            &cookie,
        )
        .await;
    assert_eq!(callback.status(), StatusCode::SEE_OTHER);
    assert_eq!(header_str(&callback, "location"), "/tasks?x=1");

    // PKCE: the verifier sent to the token endpoint matches the challenge of the first step.
    let verifier = app
        .idp
        .state
        .lock()
        .unwrap()
        .code_verifier
        .clone()
        .expect("verifier sent");
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    assert_eq!(challenge, query_param(&authorize, "code_challenge"));

    // The cookie is HTTP-only, Secure and SameSite=Lax and holds no token.
    let set_cookie = callback
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("df_session="))
        .unwrap()
        .to_string();
    for attribute in ["HttpOnly", "Secure", "SameSite=Lax"] {
        assert!(set_cookie.contains(attribute), "{set_cookie}");
    }
    assert!(!set_cookie.contains("access") && !set_cookie.contains("refresh"));

    let session = cookie_of(&callback);
    let page = app.get("/tasks", &session).await;
    assert_eq!(page.status(), StatusCode::OK);
}

#[tokio::test]
async fn first_login_creates_the_user_record() {
    let app = TestApp::new().await;
    let (cookie, authorize) = start_login(&app, "%2F").await;
    app.idp
        .set(|s| s.nonce = Some(query_param(&authorize, "nonce")));
    let state = query_param(&authorize, "state");
    app.get(
        &format!("/auth/callback?code=good-code&state={state}"),
        &cookie,
    )
    .await;

    let (issuer, subject, name): (String, String, String) =
        sqlx::query_as("SELECT issuer, subject, display_name FROM users")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        (issuer, subject, name),
        (app.idp.server.uri(), "sub-1".into(), "Peter Test".into())
    );
    let (first_seen, last_seen): (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as("SELECT first_seen, last_seen FROM users")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(first_seen.is_some() && last_seen.is_some());
}

#[tokio::test]
async fn tampered_state_fails_without_creating_a_session() {
    let app = TestApp::new().await;
    let (cookie, authorize) = start_login(&app, "%2F").await;
    app.idp
        .set(|s| s.nonce = Some(query_param(&authorize, "nonce")));
    let response = app
        .get("/auth/callback?code=good-code&state=forged", &cookie)
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(text(response).await.contains("Login failed"));
    assert_eq!(session_count(&app).await, 0);
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(users, 0);
    // The login state is single use: even the right state is refused afterwards.
    let state = query_param(&authorize, "state");
    let replay = app
        .get(
            &format!("/auth/callback?code=good-code&state={state}"),
            &cookie,
        )
        .await;
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn callback_without_a_started_login_is_refused() {
    let app = TestApp::new().await;
    let response = app.get("/auth/callback?code=good-code&state=x", "").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn user_without_the_role_gets_403_naming_it() {
    let app = TestApp::new().await;
    app.idp.set(|s| s.roles = vec!["viewer".into()]);
    let (cookie, authorize) = start_login(&app, "%2Ftasks").await;
    app.idp
        .set(|s| s.nonce = Some(query_param(&authorize, "nonce")));
    let state = query_param(&authorize, "state");
    let callback = app
        .get(
            &format!("/auth/callback?code=good-code&state={state}"),
            &cookie,
        )
        .await;
    let session = cookie_of(&callback);
    for path in ["/tasks", "/projects", "/tasks/new"] {
        let response = app.get(path, &session).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        assert!(text(response).await.contains("board-member"), "{path}");
    }
}

#[tokio::test]
async fn role_removed_at_the_provider_takes_effect_on_refresh() {
    let app = TestApp::new().await;
    // An expired access token forces a refresh on the next request.
    let (cookie, _) =
        crate::auth::testing::seed_session(&app.pool, &["board-member"], -10, Some("refresh-0"))
            .await;
    app.idp.set(|s| s.roles = vec![]);
    let response = app.get("/tasks", &cookie).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // The refreshed token is not expired, so the stored roles are now empty.
    let again = app.get("/tasks", &cookie).await;
    assert_eq!(again.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn refresh_keeps_a_board_member_logged_in_and_failure_ends_the_session() {
    let app = TestApp::new().await;
    let (cookie, _) =
        crate::auth::testing::seed_session(&app.pool, &["board-member"], -10, Some("refresh-0"))
            .await;
    assert_eq!(app.get("/tasks", &cookie).await.status(), StatusCode::OK);

    let (expired, _) =
        crate::auth::testing::seed_session(&app.pool, &["board-member"], -10, Some("refresh-0"))
            .await;
    app.idp.set(|s| s.refresh_fails = true);
    let response = app.get("/tasks", &expired).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(header_str(&response, "location").starts_with("/login"));
    // The session is gone, not just refused.
    let again = app.get("/tasks", &expired).await;
    assert!(header_str(&again, "location").starts_with("/login"));
}

#[tokio::test]
async fn concurrent_requests_refresh_an_expired_session_once() {
    let app = TestApp::new().await;
    let (cookie, _) =
        crate::auth::testing::seed_session(&app.pool, &["board-member"], -10, Some("refresh-0"))
            .await;
    app.idp.set(|s| {
        s.rotate_refresh_tokens = true;
        s.token_delay = std::time::Duration::from_millis(200);
    });

    let responses = futures::future::join_all((0..4).map(|_| app.get("/tasks", &cookie))).await;
    for response in &responses {
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(app.idp.state.lock().unwrap().refresh_calls, 1);
    // The session holds the rotated token, so it stays usable.
    assert_eq!(app.get("/tasks", &cookie).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn htmx_requests_without_a_session_are_told_to_log_in() {
    let app = TestApp::new().await;
    let response = app.send(post_request("/tasks", "", "", true)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(header_str(&response, "hx-redirect"), "/login");
}

#[tokio::test]
async fn cross_site_form_post_is_rejected_and_creates_nothing() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let body = form(&[("title", "evil"), ("project", "x"), ("persona", "y")]);
    // A browser form post from another site carries the cookie but not the htmx header.
    let response = app
        .send(post_request("/tasks", &body, &cookie, false))
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(tasks, 0);
    // The same request from the UI gets past the guard (and fails validation instead).
    let from_ui = app.send(post_request("/tasks", &body, &cookie, true)).await;
    assert_eq!(from_ui.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn logout_deletes_the_session_and_redirects_to_the_provider() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let response = app.send(post_request("/logout", "", &cookie, true)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let target = header_str(&response, "hx-redirect").to_string();
    assert!(
        target.starts_with(&format!("{}/logout?", app.idp.server.uri())),
        "{target}"
    );
    assert_eq!(query_param(&target, "id_token_hint"), "id-token");
    assert_eq!(
        query_param(&target, "post_logout_redirect_uri"),
        "https://factory.test/"
    );
    assert_eq!(session_count(&app).await, 0);

    // Reusing the cookie of the logged-out session is unauthenticated.
    let reused = app.get("/tasks", &cookie).await;
    assert_eq!(reused.status(), StatusCode::SEE_OTHER);
    assert!(header_str(&reused, "location").starts_with("/login"));
}

#[tokio::test]
async fn login_next_must_be_a_local_path() {
    let app = TestApp::new().await;
    // Browsers drop tabs and newlines from URLs, which would turn `/\t/evil` into `//evil`.
    for next in [
        "%2F%2Fevil.example",
        "%2F%09%2Fevil.example",
        "%2F%0A%2Fevil.example",
    ] {
        let (cookie, authorize) = start_login(&app, next).await;
        app.idp
            .set(|s| s.nonce = Some(query_param(&authorize, "nonce")));
        let state = query_param(&authorize, "state");
        let callback = app
            .get(
                &format!("/auth/callback?code=good-code&state={state}"),
                &cookie,
            )
            .await;
        assert_eq!(header_str(&callback, "location"), "/", "{next}");
    }
}

#[tokio::test]
async fn login_fails_when_the_session_id_cannot_be_replaced() {
    let app = TestApp::new().await;
    let (cookie, authorize) = start_login(&app, "%2F").await;
    app.idp
        .set(|s| s.nonce = Some(query_param(&authorize, "nonce")));
    // Replacing the id deletes the old row; make exactly that fail.
    sqlx::raw_sql(
        "CREATE FUNCTION refuse_delete() RETURNS trigger LANGUAGE plpgsql AS
             $$ BEGIN RAISE EXCEPTION 'store unavailable'; END $$;
         CREATE TRIGGER refuse_delete BEFORE DELETE ON web_sessions
             FOR EACH ROW EXECUTE FUNCTION refuse_delete();",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let state = query_param(&authorize, "state");
    let callback = app
        .get(
            &format!("/auth/callback?code=good-code&state={state}"),
            &cookie,
        )
        .await;

    assert_eq!(callback.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let logged_in: i64 =
        sqlx::query_scalar("SELECT count(*) FROM web_sessions WHERE data ? 'user'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(logged_in, 0);
}

#[tokio::test]
async fn health_and_assets_need_no_login() {
    let app = TestApp::new().await;
    assert_eq!(app.get("/healthz", "").await.status(), StatusCode::OK);
    assert_eq!(app.get("/readyz", "").await.status(), StatusCode::OK);
    assert_eq!(
        app.get("/assets/vendor/htmx.min.js", "").await.status(),
        StatusCode::OK
    );
}
