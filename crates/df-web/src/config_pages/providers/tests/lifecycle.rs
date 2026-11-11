//! Tests of creating, validating and deleting model providers.

use axum::http::StatusCode;

use super::{credential, provider_form, stored};
use crate::testkit::{TestApp, header_str, seed, seed_provider, text};

#[tokio::test]
async fn creating_a_provider_sends_the_browser_to_its_page_and_personas_can_select_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "anthropic-key").await;

    let response = app
        .post(
            "/model-providers",
            &provider_form(&credential, &[]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let id = stored(&app, "Anthropic work").await;
    assert_eq!(
        header_str(&response, "hx-location"),
        format!("/model-providers/{id}")
    );
    assert_eq!(header_str(&response, "hx-trigger"), "");
    let html = text(app.get("/model-providers", &cookie).await).await;
    assert!(html.contains("Anthropic work") && html.contains("Anthropic Messages"));
    let apis: Vec<(String, String)> =
        sqlx::query_as("SELECT wire_api, upstream_url FROM model_provider_apis")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        apis,
        [(
            "anthropic-messages".to_string(),
            "https://api.anthropic.com".to_string()
        )]
    );
    let persona_form = text(app.get_htmx("/personas/new", &cookie).await).await;
    assert!(persona_form.contains("Anthropic work"), "{persona_form}");
    let audit: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT details FROM audit_log WHERE action = 'model_provider.create'")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(audit[0]["after"]["scheme"], "x-api-key");
}

#[tokio::test]
async fn plain_http_upstream_is_rejected_by_the_form() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    let body = provider_form(
        &credential,
        &[("url_anthropic_messages", "http://llm.internal")],
    );

    let response = app.post("/model-providers", &body, &cookie).await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(response).await.contains("must be an HTTPS URL"));
}

#[tokio::test]
async fn claude_subscription_cannot_offer_another_api() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    let body = provider_form(
        &credential,
        &[
            ("scheme", "claude-subscription"),
            ("api_openai_chat", "on"),
            ("url_openai_chat", "https://api.openai.com"),
        ],
    );

    let response = app.post("/model-providers", &body, &cookie).await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        text(response)
            .await
            .contains("only be used with Anthropic Messages")
    );
}

#[tokio::test]
async fn header_rules_are_enforced_with_field_errors() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    let cases = [
        (
            &[("scheme", "header"), ("credential_header", "Host")][..],
            "set by the gateway",
        ),
        (&[("headers", "x-session: {{ task_id | md5 }}")][..], "md5"),
        (&[("headers", "X-Api-Key: x")][..], "set by the gateway"),
        (&[("headers", "no colon here")][..], "must look like"),
    ];
    for (extra, expected) in cases {
        let response = app
            .post(
                "/model-providers",
                &provider_form(&credential, extra),
                &cookie,
            )
            .await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{extra:?}"
        );
        let html = text(response).await;
        assert!(html.contains(expected), "{extra:?}: {html}");
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM model_providers")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn the_opencode_go_preset_prefills_apis_scheme_and_session_header() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let html = text(
        app.get(
            "/model-providers/preset?preset=opencode-go&name=Go",
            &cookie,
        )
        .await,
    )
    .await;

    assert!(html.contains("value=\"Go\""), "the typed name stays");
    assert!(html.contains("https://opencode.ai/inference/go/anthropic"));
    assert_eq!(
        html.matches("https://opencode.ai/inference/go/openai")
            .count(),
        2
    );
    assert!(
        html.contains("x-opencode-session: {{ task_id | sha256 }}"),
        "{html}"
    );
    assert!(
        html.contains("<option value=\"bearer\" selected>"),
        "scheme bearer: {html}"
    );
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM model_providers")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(stored, 0, "presets are not stored");
}

#[tokio::test]
async fn the_preset_list_offers_the_four_presets_without_github_models() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let html = text(app.get_htmx("/model-providers/new", &cookie).await).await;

    for label in ["Anthropic", "Claude subscription", "OpenAI", "OpenCode Go"] {
        assert!(html.contains(label), "{label}");
    }
    assert!(!html.contains("GitHub Models"), "{html}");
}

#[tokio::test]
async fn the_provider_page_shows_the_credential_reference_but_no_value() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "anthropic-key").await;
    app.post(
        "/model-providers",
        &provider_form(&credential, &[]),
        &cookie,
    )
    .await;
    let id = stored(&app, "Anthropic work").await;

    let list = text(app.get("/model-providers", &cookie).await).await;
    let edit = text(app.get(&format!("/model-providers/{id}"), &cookie).await).await;

    for html in [&list, &edit] {
        assert!(html.contains("anthropic-key"), "{html}");
        assert!(
            html.contains("llm-secret") && html.contains("api-key"),
            "{html}"
        );
    }
    assert!(list.contains("Model providers"));
    // df-web cannot read Secret values, so the most it can guard is that no field takes one.
    for html in [&list, &edit] {
        assert!(!html.contains("type=\"password\""), "{html}");
    }
}

#[tokio::test]
async fn a_provider_in_use_is_not_deleted_and_the_message_names_the_persona() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    let response = app
        .delete(&format!("/model-providers/{}", seed.provider), &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(
        html.contains("Linus") && html.contains("is used by"),
        "{html}"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM model_providers")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn an_unused_provider_is_deleted_and_audited() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await;

    let response = app
        .delete(&format!("/model-providers/{provider}"), &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(text(response).await.contains("No model providers yet"));
    let audit: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'model_provider.delete'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(audit, 1);
}
