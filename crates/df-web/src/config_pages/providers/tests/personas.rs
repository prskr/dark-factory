//! Tests of the personas that use a model provider.

use axum::http::StatusCode;

use super::{credential, provider_form, stored};
use crate::testkit::{TestApp, form, seed, seed_provider, text};

#[tokio::test]
async fn persona_form_only_offers_compatible_providers() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    app.post(
        "/model-providers",
        &provider_form(&credential, &[]),
        &cookie,
    )
    .await;
    let openai = form(&[
        ("name", "OpenAI only"),
        ("credential_ref_id", &credential),
        ("scheme", "bearer"),
        ("api_openai_chat", "on"),
        ("url_openai_chat", "https://api.openai.com"),
    ]);
    assert_eq!(
        app.post("/model-providers", &openai, &cookie)
            .await
            .status(),
        StatusCode::OK
    );

    let html = text(app.get_htmx("/personas/new", &cookie).await).await;

    assert!(html.contains("Anthropic work"));
    assert!(!html.contains("OpenAI only"), "{html}");
}

#[tokio::test]
async fn persona_with_an_incompatible_provider_is_rejected_on_the_server() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    let openai = form(&[
        ("name", "OpenAI only"),
        ("credential_ref_id", &credential),
        ("scheme", "bearer"),
        ("api_openai_chat", "on"),
        ("url_openai_chat", "https://api.openai.com"),
    ]);
    app.post("/model-providers", &openai, &cookie).await;
    let provider = stored(&app, "OpenAI only").await.to_string();
    let persona = form(&[
        ("name", "Ada"),
        ("system_prompt", "x"),
        ("model", "gpt-6-luna"),
        ("model_provider_id", &provider),
    ]);

    let response = app.post("/personas", &persona, &cookie).await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        text(response)
            .await
            .contains("offers none of the wire APIs")
    );
    let personas: i64 = sqlx::query_scalar("SELECT count(*) FROM personas")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(personas, 0);
}

#[tokio::test]
async fn a_persona_is_created_with_a_compatible_provider() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await.to_string();
    let persona = form(&[
        ("name", "Linus"),
        ("system_prompt", "Be Linus."),
        ("model", "claude-sonnet-5-5"),
        ("model_provider_id", &provider),
    ]);

    let response = app.post("/personas", &persona, &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    let stored: uuid::Uuid =
        sqlx::query_scalar("SELECT model_provider_id FROM personas WHERE name = 'Linus'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(stored.to_string(), provider);
}

#[tokio::test]
async fn editing_a_provider_cannot_make_its_personas_incompatible() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let credential = credential(&app, "k").await;
    let openai_only = form(&[
        ("name", "Claude subscription"),
        ("credential_ref_id", &credential),
        ("scheme", "bearer"),
        ("api_openai_chat", "on"),
        ("url_openai_chat", "https://api.openai.com"),
        ("model_id_0", "claude-sonnet-5-5"),
        ("model_api_0", "openai-chat"),
    ]);

    let response = app
        .post(
            &format!("/model-providers/{}", seed.provider),
            &openai_only,
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(response).await.contains("Persona Linus"));
}
