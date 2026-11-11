//! Tests of the Models tab: rows, limits, presets, add and remove.

use axum::http::StatusCode;

use super::{credential, provider_form};
use crate::testkit::{TestApp, form, seed, tag_around, text};

/// Model rows as `(id, wire API, context, output)`.
type Rows<'a> = &'a [(&'a str, &'a str, &'a str, &'a str)];

/// `provider_form` with an Anthropic API and `models` as `(id, wire API, context, output)` rows.
fn with_models(credential: &str, models: Rows<'_>) -> String {
    let mut fields: Vec<(String, String)> = Vec::new();
    for (n, (id, api, context, output)) in models.iter().enumerate() {
        fields.push((format!("model_id_{n}"), (*id).to_string()));
        fields.push((format!("model_api_{n}"), (*api).to_string()));
        fields.push((format!("model_context_{n}"), (*context).to_string()));
        fields.push((format!("model_output_{n}"), (*output).to_string()));
    }
    let extra: Vec<(&str, &str)> = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    provider_form(credential, &extra)
}

async fn stored_models(app: &TestApp) -> Vec<(String, String, Option<i64>, Option<i64>)> {
    sqlx::query_as(
        "SELECT model_id, wire_api, context_window_tokens, max_output_tokens \
         FROM model_provider_models ORDER BY model_id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn models_with_limits_are_stored_with_the_provider() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    let body = with_models(
        &credential,
        &[
            (
                "claude-sonnet-5-5",
                "anthropic-messages",
                "1000000",
                "128000",
            ),
            ("claude-haiku-5-5", "anthropic-messages", "", ""),
        ],
    );

    let response = app.post("/model-providers", &body, &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        stored_models(&app).await,
        [
            (
                "claude-haiku-5-5".into(),
                "anthropic-messages".into(),
                None,
                None
            ),
            (
                "claude-sonnet-5-5".into(),
                "anthropic-messages".into(),
                Some(1_000_000),
                Some(128_000)
            ),
        ]
    );
}

#[tokio::test]
async fn model_problems_are_shown_under_their_row_on_the_models_tab() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    let cases: [(Rows<'_>, &str); 3] = [
        (
            &[("m", "anthropic-messages", "1000", "")],
            "Set both limits or neither.",
        ),
        (
            &[
                ("glm-5.3", "anthropic-messages", "", ""),
                ("glm-5.3", "anthropic-messages", "", ""),
            ],
            "Model glm-5.3 is listed twice.",
        ),
        (
            &[("gpt-6-luna", "openai-responses", "", "")],
            "gpt-6-luna uses OpenAI Responses, which this provider does not offer (tab General).",
        ),
    ];
    for (models, message) in cases {
        let response = app
            .post(
                "/model-providers",
                &with_models(&credential, models),
                &cookie,
            )
            .await;

        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{message}"
        );
        let html = text(response).await;
        assert!(html.contains(message), "{message}: {html}");
        assert!(
            html.contains("tabs(&quot;models&quot;, false)"),
            "{message}: the tab opens"
        );
    }
    assert!(stored_models(&app).await.is_empty());
}

#[tokio::test]
async fn the_duplicate_error_marks_the_second_row() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    let body = with_models(
        &credential,
        &[
            ("glm-5.3", "anthropic-messages", "", ""),
            ("glm-5.3", "anthropic-messages", "", ""),
        ],
    );

    let html = text(app.post("/model-providers", &body, &cookie).await).await;

    let marked = |n: usize| {
        tag_around(&html, &format!("name=\"model_id_{n}\"")).contains("aria-invalid=\"true\"")
    };
    assert!(!marked(0) && marked(1), "{html}");
}

#[tokio::test]
async fn presets_prefill_their_models() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let preset = |id: &str| {
        let uri = format!("/model-providers/preset?preset={id}&name=P");
        let (app, cookie) = (&app, &cookie);
        async move { text(app.get(&uri, cookie).await).await }
    };

    let go = preset("opencode-go").await;
    for (id, api, context, output) in [
        ("glm-5.3", "openai-chat", "1000000", "131072"),
        ("kimi-k3", "openai-chat", "1048576", "131072"),
        ("deepseek-v4-pro", "openai-chat", "1000000", "384000"),
        ("minimax-m3", "anthropic-messages", "1000000", "131072"),
        ("qwen3.8-max", "anthropic-messages", "1000000", "131072"),
        (
            "claude-haiku-5-5",
            "anthropic-messages",
            "1000000",
            "128000",
        ),
        ("gpt-6-luna", "openai-responses", "1050000", "128000"),
    ] {
        let n = go
            .find(&format!("value=\"{id}\""))
            .unwrap_or_else(|| panic!("{id}"));
        let row = &go[n..n + go[n..].find("aria-label=\"Remove model").unwrap()];
        assert!(
            row.contains(&format!("<option value=\"{api}\" selected>"))
                && row.contains(&format!("value=\"{context}\""))
                && row.contains(&format!("value=\"{output}\"")),
            "{id}: {row}"
        );
    }
    assert_eq!(go.matches("name=\"model_id_").count(), 7);

    let anthropic = preset("claude-subscription").await;
    for id in ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"] {
        assert!(anthropic.contains(&format!("value=\"{id}\"")), "{id}");
    }
    let openai = preset("openai").await;
    for id in ["gpt-6-luna", "gpt-6.1-terra", "gpt-6.1-sol", "gpt-6-astra"] {
        assert!(openai.contains(&format!("value=\"{id}\"")), "{id}");
    }
    assert!(openai.contains("value=\"1050000\""));
}

#[tokio::test]
async fn preset_models_can_be_changed_before_saving() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    // The Claude preset's rows without `claude-haiku-5-5`, plus `claude-opus-5-6`.
    let body = with_models(
        &credential,
        &[
            ("claude-opus-5-5", "anthropic-messages", "1000000", "128000"),
            (
                "claude-sonnet-5-5",
                "anthropic-messages",
                "1000000",
                "128000",
            ),
            ("claude-opus-5-6", "anthropic-messages", "", ""),
        ],
    );

    app.post("/model-providers", &body, &cookie).await;

    let ids: Vec<String> = stored_models(&app).await.into_iter().map(|m| m.0).collect();
    assert_eq!(
        ids,
        ["claude-opus-5-5", "claude-opus-5-6", "claude-sonnet-5-5"]
    );
}

#[tokio::test]
async fn add_and_remove_render_the_form_again_with_the_row_changed() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    let body = with_models(
        &credential,
        &[
            ("a", "anthropic-messages", "", ""),
            ("b", "anthropic-messages", "", ""),
        ],
    );

    let added = text(
        app.post(
            "/model-providers/refresh",
            &format!("{body}&op=add"),
            &cookie,
        )
        .await,
    )
    .await;
    assert!(added.contains("name=\"model_id_2\""), "{added}");
    assert!(added.contains("tabs(&quot;models&quot;, false)"));
    assert!(
        added.contains("value=\"Anthropic work\""),
        "typed values stay"
    );

    let removed = text(
        app.post(
            "/model-providers/refresh",
            &format!("{body}&op=remove:0"),
            &cookie,
        )
        .await,
    )
    .await;
    assert!(
        removed.contains("value=\"b\"") && !removed.contains("value=\"a\""),
        "{removed}"
    );
    assert!(!removed.contains("name=\"model_id_1\""));
}

#[tokio::test]
async fn a_model_a_persona_uses_cannot_be_removed() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let credential = credential(&app, "k").await;

    let response = app
        .post(
            &format!("/model-providers/{}", seed.provider),
            &provider_form(&credential, &[]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("claude-sonnet-5-5 cannot be removed: persona Linus uses it."),
        "{html}"
    );
    assert!(html.contains("tabs(&quot;models&quot;, true)"));
}

#[tokio::test]
async fn a_model_on_an_api_the_provider_does_not_offer_is_flagged_with_its_name() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    let body = form(&[
        ("name", "Anthropic work"),
        ("credential_ref_id", &credential),
        ("scheme", "bearer"),
        ("api_openai_chat", "on"),
        ("url_openai_chat", "https://api.openai.com"),
        ("model_id_0", "claude-sonnet-5-5"),
        ("model_api_0", "anthropic-messages"),
    ]);

    let response = app.post("/model-providers", &body, &cookie).await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains(
            "claude-sonnet-5-5 uses Anthropic Messages, which this provider does not offer"
        ),
        "{html}"
    );
}

#[tokio::test]
async fn a_duplicate_provider_name_is_shown_on_the_general_tab_not_the_models_tab() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    let body = provider_form(&credential, &[]);
    app.post("/model-providers", &body, &cookie).await;

    let response = app.post("/model-providers", &body, &cookie).await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("a model provider with this name already exists"),
        "{html}"
    );
    assert!(!html.contains("tabs(&quot;models&quot;, false)"), "{html}");
}

#[tokio::test]
async fn refreshing_the_provider_form_keeps_its_create_or_edit_mode() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "k").await;
    let id = uuid::Uuid::now_v7().to_string();
    let body = format!("{}&op=add", provider_form(&credential, &[]));

    let created = text(app.post("/model-providers/refresh", &body, &cookie).await).await;
    let edited = text(
        app.post(
            "/model-providers/refresh",
            &format!("{body}&entity_id={id}"),
            &cookie,
        )
        .await,
    )
    .await;

    assert!(
        created.contains("hx-post=\"/model-providers\"")
            && created.contains("Create model provider")
    );
    assert!(
        edited.contains(&format!("hx-post=\"/model-providers/{id}\""))
            && edited.contains("Save model provider")
            && !edited.contains("Create model provider")
    );
}

#[tokio::test]
async fn the_create_dialog_keeps_narrow_model_columns_and_a_scrolling_list_in_every_render() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let credential = credential(&app, "k").await;
    let opened = text(app.get_htmx("/model-providers/new", &cookie).await).await;
    let body = format!("{}&op=add", provider_form("", &[]));
    let refreshed = text(app.post("/model-providers/refresh", &body, &cookie).await).await;
    let preset = text(
        app.get_htmx("/model-providers/preset?preset=opencode-go&name=x", &cookie)
            .await,
    )
    .await;
    let invalid = provider_form(
        &credential,
        &[("url_anthropic_messages", "http://llm.internal")],
    );
    let response = app.post("/model-providers", &invalid, &cookie).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let failed = text(response).await;

    for html in [opened, refreshed, preset, failed] {
        let panel = &html[html.find("id=\"panel-models\"").unwrap()..];
        assert!(
            panel.contains("max-h-96 space-y-2 overflow-y-auto"),
            "{panel}"
        );
        assert!(panel.contains("w-48") && panel.contains("w-21"), "{panel}");
        assert!(
            !panel.contains("w-55") && !panel.contains("w-35"),
            "{panel}"
        );
    }
}
