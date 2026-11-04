use axum::http::StatusCode;

use crate::testkit::{
    TestApp, form, has_disabled_attribute, header_str, seed, seed_provider, tag_around, text,
};

/// The form of an Anthropic provider; `extra` overrides or adds fields.
fn provider_form(credential: &str, extra: &[(&str, &str)]) -> String {
    let mut pairs = vec![
        ("name", "Anthropic work"),
        ("credential_ref_id", credential),
        ("scheme", "x-api-key"),
        ("api_anthropic_messages", "on"),
        ("url_anthropic_messages", "https://api.anthropic.com"),
    ];
    for (name, value) in extra {
        pairs.retain(|(existing, _)| existing != name);
        pairs.push((name, value));
    }
    form(&pairs)
}

async fn credential(app: &TestApp, name: &str) -> String {
    let id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO credential_refs (id, name, secret_name, secret_key) VALUES ($1, $2, 'llm-secret', 'api-key')")
        .bind(id)
        .bind(name)
        .execute(&app.pool)
        .await
        .unwrap();
    id.to_string()
}

async fn stored(app: &TestApp, name: &str) -> uuid::Uuid {
    sqlx::query_scalar("SELECT id FROM model_providers WHERE name = $1")
        .bind(name)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_board_member_creates_a_provider_that_personas_can_select() {
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
    assert_eq!(header_str(&response, "hx-retarget"), "#config-list");
    let html = text(response).await;
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
async fn a_custom_credential_header_can_be_api_key_without_prefix() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "azure-key").await;

    let response = app
        .post(
            "/model-providers",
            &provider_form(
                &credential,
                &[("scheme", "header"), ("credential_header", "api-key")],
            ),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-retarget"), "#config-list");
    let (header, prefix): (String, Option<String>) = sqlx::query_as(
        "SELECT credential_header, credential_prefix FROM model_providers WHERE name = 'Anthropic work'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(header, "api-key");
    assert!(prefix.is_none_or(|prefix| prefix.is_empty()));
}

#[tokio::test]
async fn a_custom_credential_header_can_be_authorization_with_a_token_prefix() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "token-key").await;

    let response = app
        .post(
            "/model-providers",
            &provider_form(
                &credential,
                &[
                    ("scheme", "header"),
                    ("credential_header", "Authorization"),
                    ("credential_prefix", "Token "),
                ],
            ),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-retarget"), "#config-list");
    let (header, prefix): (String, String) = sqlx::query_as(
        "SELECT credential_header, credential_prefix FROM model_providers WHERE name = 'Anthropic work'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        (header.as_str(), prefix.as_str()),
        ("Authorization", "Token ")
    );
}

#[tokio::test]
async fn a_custom_credential_header_cannot_be_a_connection_header() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "host-key").await;

    let response = app
        .post(
            "/model-providers",
            &provider_form(
                &credential,
                &[("scheme", "header"), ("credential_header", "Host")],
            ),
            &cookie,
        )
        .await;

    let html = text(response).await;
    assert!(html.contains("set by the gateway"), "{html}");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM model_providers")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
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
    let edit = text(
        app.get_htmx(&format!("/model-providers/{id}/edit"), &cookie)
            .await,
    )
    .await;

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

/// The provider form as the server renders it back for `extra` posted on top of a default form
/// (the empty name makes the post fail, so the form comes back with the posted values).
async fn rendered_form(app: &TestApp, cookie: &str, extra: &[(&str, &str)]) -> String {
    let mut fields = vec![("name", "")];
    fields.extend_from_slice(extra);
    text(
        app.post(
            "/model-providers",
            &provider_form("00000000-0000-0000-0000-000000000000", &fields),
            cookie,
        )
        .await,
    )
    .await
}

/// The `x-data` state of the form in `html` as JSON.
fn form_state(html: &str) -> serde_json::Value {
    let tag = tag_around(html, "id=\"form\"");
    let raw = tag
        .split("x-data=\"")
        .nth(1)
        .and_then(|rest| rest.split("\" ").next())
        .expect("x-data");
    let json = raw
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    serde_json::from_str(&json).expect(&json)
}

#[tokio::test]
async fn a_scheme_with_a_fixed_header_shows_it_as_a_disabled_preview() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    for (scheme, header, prefix) in [
        ("x-api-key", "x-api-key", ""),
        ("bearer", "Authorization", "Bearer "),
        ("claude-subscription", "Authorization", "Bearer "),
    ] {
        let html = rendered_form(&app, &cookie, &[("scheme", scheme)]).await;

        let header_tag = tag_around(&html, "id=\"credential_header\"");
        assert!(has_disabled_attribute(header_tag), "{scheme}: {header_tag}");
        assert!(
            header_tag.contains(&format!("value=\"{header}\"")),
            "{scheme}: {header_tag}"
        );
        let prefix_tag = tag_around(&html, "id=\"credential_prefix\"");
        assert!(has_disabled_attribute(prefix_tag), "{scheme}: {prefix_tag}");
        assert!(
            prefix_tag.contains(&format!("value=\"{prefix}\"")),
            "{scheme}: {prefix_tag}"
        );
        let state = form_state(&html);
        assert_eq!(state["fixed"][scheme]["header"], header, "{scheme}");
        assert_eq!(state["fixed"][scheme]["prefix"], prefix, "{scheme}");
    }
}

#[tokio::test]
async fn the_custom_header_scheme_has_an_editable_header_and_prefix() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let html = rendered_form(
        &app,
        &cookie,
        &[
            ("scheme", "header"),
            ("credential_header", "api-key"),
            ("credential_prefix", "Token "),
        ],
    )
    .await;

    let header_tag = tag_around(&html, "id=\"credential_header\"");
    assert!(!has_disabled_attribute(header_tag), "{header_tag}");
    assert!(header_tag.contains("value=\"api-key\""), "{header_tag}");
    let prefix_tag = tag_around(&html, "id=\"credential_prefix\"");
    assert!(!has_disabled_attribute(prefix_tag), "{prefix_tag}");
    assert!(prefix_tag.contains("value=\"Token \""), "{prefix_tag}");
}

#[tokio::test]
async fn typed_header_prefix_and_extra_headers_are_kept_while_the_scheme_changes() {
    // The scheme only toggles the controls in the browser: the typed values are in the form
    // state whatever scheme is chosen, and the controls show them again when it switches back.
    let app = TestApp::new().await;
    let cookie = app.login().await;

    for scheme in ["x-api-key", "bearer", "claude-subscription", "header"] {
        let html = rendered_form(
            &app,
            &cookie,
            &[
                ("scheme", scheme),
                ("credential_header", "api-key"),
                ("credential_prefix", "Token "),
                ("headers", "x-session: {{ task_id | sha256 }}"),
            ],
        )
        .await;

        let state = form_state(&html);
        assert_eq!(state["scheme"], scheme);
        assert_eq!(state["header"], "api-key", "{scheme}");
        assert_eq!(state["prefix"], "Token ", "{scheme}");
        assert_eq!(
            state["headers"], "x-session: {{ task_id | sha256 }}",
            "{scheme}"
        );
        let textarea = &html[html.find("<textarea").unwrap()..];
        assert!(
            textarea.contains("x-session: {{ task_id | sha256 }}"),
            "{scheme}"
        );
        let header_tag = tag_around(&html, "id=\"credential_header\"");
        assert!(
            header_tag
                .contains("x-bind:value=\"scheme === 'header' ? header : fixed[scheme].header\""),
            "{header_tag}"
        );
        assert!(header_tag.contains("x-on:input=\"header = $event.target.value\""));
        let prefix_tag = tag_around(&html, "id=\"credential_prefix\"");
        assert!(
            prefix_tag
                .contains("x-bind:value=\"scheme === 'header' ? prefix : fixed[scheme].prefix\""),
            "{prefix_tag}"
        );
        assert!(
            prefix_tag.contains("x-on:input=\"prefix = $event.target.value\""),
            "{prefix_tag}"
        );
        let textarea_tag = tag_around(&html, "id=\"headers\"");
        assert!(
            textarea_tag.contains("x-model=\"headers\""),
            "{textarea_tag}"
        );
        assert!(
            textarea_tag.contains("x-bind:disabled=\"scheme === 'claude-subscription'\""),
            "{textarea_tag}"
        );
        let select = tag_around(&html, "id=\"scheme\"");
        assert!(select.contains("x-model=\"scheme\""), "{select}");
    }
}

#[tokio::test]
async fn the_example_token_is_a_fixed_text_never_derived_from_the_credential() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    credential(&app, "anthropic-key").await;

    let html = text(app.get_htmx("/model-providers/new", &cookie).await).await;

    let at = html
        .find("id=\"credential_example\"")
        .expect("the example token");
    let token = &html[at..html[at..].find("</span>").map(|end| at + end).unwrap()];
    assert!(token.ends_with("sk-…example…4f7a"), "{token}");
    assert!(
        !token.contains("llm-secret") && !token.contains("api-key"),
        "{token}"
    );
}

#[tokio::test]
async fn extra_headers_are_disabled_for_the_claude_subscription_scheme_only() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let subscription = rendered_form(&app, &cookie, &[("scheme", "claude-subscription")]).await;
    let bearer = rendered_form(&app, &cookie, &[("scheme", "bearer")]).await;

    assert!(has_disabled_attribute(tag_around(
        &subscription,
        "id=\"headers\""
    )));
    assert!(!has_disabled_attribute(tag_around(
        &bearer,
        "id=\"headers\""
    )));
    // The hint that fits the scheme is the visible one.
    let hint = |html: &str| {
        let hint = &html[html.find("id=\"headers-hint\"").unwrap()..];
        hint[..hint.find("</p>").unwrap()].to_string()
    };
    let (shown, hidden) = hint(&subscription)
        .split_once("</span>")
        .map(|(first, second)| (first.to_string(), second.to_string()))
        .unwrap();
    assert!(shown.contains("Not available with the Claude subscription scheme."));
    assert!(hidden.contains("display: none"));
    assert!(hint(&bearer).contains("One “Name: value” per line."));
}

#[tokio::test]
async fn the_form_references_hint_and_error_of_the_scheme_controls() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let html = rendered_form(
        &app,
        &cookie,
        &[("scheme", "header"), ("headers", "no colon here")],
    )
    .await;

    let textarea = tag_around(&html, "id=\"headers\"");
    assert!(textarea.contains("aria-invalid=\"true\""), "{textarea}");
    assert!(
        textarea.contains("aria-describedby=\"headers-hint headers-error\""),
        "{textarea}"
    );
    assert!(html.contains("id=\"headers-error\""));
}

#[tokio::test]
async fn name_and_preset_share_a_row_and_so_do_credential_and_scheme() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let html = text(app.get_htmx("/model-providers/new", &cookie).await).await;

    let at = |marker: &str| html.find(marker).unwrap_or_else(|| panic!("{marker}"));
    let order = [
        "id=\"name\"",
        "id=\"preset\"",
        "api_anthropic_messages",
        "id=\"credential_ref_id\"",
        "id=\"scheme\"",
        "id=\"credential_header\"",
        "id=\"credential_prefix\"",
        "id=\"headers\"",
    ];
    let positions: Vec<usize> = order.iter().map(|marker| at(marker)).collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{positions:?}"
    );
    // Neither row is a full-width row: both fields are cells of the two-column grid.
    for marker in ["id=\"name\"", "id=\"credential_ref_id\""] {
        let before = &html[..at(marker)];
        let cell = &before[before.rfind("<div class=\"").unwrap()..];
        assert!(
            !cell.starts_with("<div class=\"md:col-span-2\""),
            "{marker}: {cell}"
        );
    }
}

#[tokio::test]
async fn the_edit_form_has_no_preset_and_keeps_the_name_first() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await;

    let html = text(
        app.get_htmx(&format!("/model-providers/{provider}/edit"), &cookie)
            .await,
    )
    .await;

    assert!(!html.contains("id=\"preset\""), "{html}");
    assert!(html.contains("id=\"name\""));
}

#[tokio::test]
async fn extra_headers_with_the_claude_subscription_scheme_are_rejected_and_nothing_is_saved() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "claude-key").await;

    let response = app
        .post(
            "/model-providers",
            &provider_form(
                &credential,
                &[
                    ("scheme", "claude-subscription"),
                    ("headers", "x-session: {{ task_id | sha256 }}"),
                ],
            ),
            &cookie,
        )
        .await;

    let html = text(response).await;
    assert!(
        html.contains("Extra headers are not available with the Claude subscription scheme."),
        "{html}"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM model_providers")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn header_and_prefix_values_posted_for_a_scheme_that_fixes_them_are_not_stored() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "bearer-key").await;

    let response = app
        .post(
            "/model-providers",
            &provider_form(
                &credential,
                &[
                    ("scheme", "bearer"),
                    ("credential_header", "x-left-over"),
                    ("credential_prefix", "Token "),
                ],
            ),
            &cookie,
        )
        .await;

    assert_eq!(header_str(&response, "hx-retarget"), "#config-list");
    let (header, prefix): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT credential_header, credential_prefix FROM model_providers WHERE name = 'Anthropic work'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!((header, prefix), (None, None));
}

#[tokio::test]
async fn a_form_without_the_disabled_fields_still_saves_a_fixed_scheme() {
    // Browsers do not post disabled controls: header, prefix and (subscription) extra headers.
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let credential = credential(&app, "claude-key").await;
    let body = form(&[
        ("name", "Claude work"),
        ("credential_ref_id", &credential),
        ("scheme", "claude-subscription"),
        ("api_anthropic_messages", "on"),
        ("url_anthropic_messages", "https://api.anthropic.com"),
    ]);

    let response = app.post("/model-providers", &body, &cookie).await;

    assert_eq!(header_str(&response, "hx-retarget"), "#config-list");
    stored(&app, "Claude work").await;
}
