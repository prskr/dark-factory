//! Tests of the persona form, creating and editing personas, and the persona cards.

use axum::http::StatusCode;
use df_db::DbError;

use super::{assert_saved, audit};
use crate::config_pages::personas::save_error;
use crate::testkit::{TestApp, form, seed, seed_provider, tag_around, text};

/// A provider "Go" with `glm-5.3` on Chat Completions, `minimax-m3` on Anthropic Messages and
/// `gpt-6-luna` on Responses; returns its id.
async fn go_provider(app: &TestApp, cookie: &str) -> String {
    let credential = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO credential_refs (id, name, secret_name, secret_key) VALUES ($1, 'go-key', 's', 'k')")
        .bind(credential)
        .execute(&app.pool)
        .await
        .unwrap();
    let body = form(&[
        ("name", "Go"),
        ("credential_ref_id", &credential.to_string()),
        ("scheme", "bearer"),
        ("api_openai_chat", "on"),
        ("url_openai_chat", "https://go.example/openai"),
        ("api_openai_responses", "on"),
        ("url_openai_responses", "https://go.example/openai"),
        ("api_anthropic_messages", "on"),
        ("url_anthropic_messages", "https://go.example/anthropic"),
        ("model_id_0", "glm-5.3"),
        ("model_api_0", "openai-chat"),
        ("model_context_0", "1000000"),
        ("model_output_0", "131072"),
        ("model_id_1", "minimax-m3"),
        ("model_api_1", "anthropic-messages"),
        ("model_id_2", "gpt-6-luna"),
        ("model_api_2", "openai-responses"),
    ]);
    let response = app.post("/model-providers", &body, cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM model_providers WHERE name = 'Go'")
        .fetch_one(&app.pool)
        .await
        .unwrap()
        .to_string()
}

fn persona(harness: &str, provider: &str, model: &str) -> String {
    form(&[
        ("name", "Ada"),
        ("system_prompt", "Write docs."),
        ("harness", harness),
        ("model_provider_id", provider),
        ("model", model),
    ])
}

#[tokio::test]
async fn an_opencode_persona_with_a_chat_completions_model_is_stored() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let go = go_provider(&app, &cookie).await;

    let response = app
        .post("/personas", &persona("opencode", &go, "glm-5.3"), &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let stored: (String, String) =
        sqlx::query_as("SELECT harness, model FROM personas WHERE name = 'Ada'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(stored, ("opencode".to_string(), "glm-5.3".to_string()));
}

#[tokio::test]
async fn a_persona_without_a_model_is_rejected() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await.to_string();

    let response = app
        .post("/personas", &persona("claude-code", &provider, ""), &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(response).await.contains("Model is required."));
}

#[tokio::test]
async fn a_claude_code_persona_cannot_take_a_chat_completions_model() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let go = go_provider(&app, &cookie).await;

    let response = app
        .post(
            "/personas",
            &persona("claude-code", &go, "glm-5.3"),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("cannot use OpenAI Chat Completions"),
        "{html}"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM personas")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn a_claude_subscription_provider_is_not_offered_to_and_refused_for_opencode() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let subscription = seed_provider(&app.pool).await.to_string();

    let response = app
        .post(
            "/personas",
            &persona("opencode", &subscription, "claude-sonnet-5-5"),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        text(response)
            .await
            .contains("only be used with the claude-code harness")
    );
}

#[tokio::test]
async fn the_model_select_lists_the_models_of_the_provider_the_harness_can_use() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let go = go_provider(&app, &cookie).await;
    let options = |html: &str| {
        let select = &html[html.find("<select id=\"model\"").unwrap()..];
        let select = &select[..select.find("</select>").unwrap()];
        select.matches("<option value=\"").count() - 1
    };

    let opencode = text(
        app.post("/personas/refresh", &persona("opencode", &go, ""), &cookie)
            .await,
    )
    .await;
    let claude = text(
        app.post(
            "/personas/refresh",
            &persona("claude-code", &go, ""),
            &cookie,
        )
        .await,
    )
    .await;

    assert_eq!(options(&opencode), 3, "{opencode}");
    assert!(opencode.contains("glm-5.3 (OpenAI Chat Completions)"));
    assert_eq!(options(&claude), 1, "{claude}");
    assert!(claude.contains("minimax-m3 (Anthropic Messages)") && !claude.contains("glm-5.3"));
}

#[tokio::test]
async fn changing_the_harness_drops_the_provider_and_model_it_cannot_use() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let subscription = seed_provider(&app.pool).await.to_string();

    let html = text(
        app.post(
            "/personas/refresh",
            &persona("opencode", &subscription, "claude-sonnet-5-5"),
            &cookie,
        )
        .await,
    )
    .await;

    assert!(!html.contains("Claude subscription"), "{html}");
    let provider = tag_around(&html, "id=\"model_provider_id\"");
    assert!(provider.contains("hx-post=\"/personas/refresh\""));
    assert!(html.contains("value=\"opencode\" selected"), "{html}");
}

#[tokio::test]
async fn the_form_offers_both_harnesses_and_a_required_model_with_its_explanation() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let html = text(app.get_htmx("/personas/new", &cookie).await).await;

    assert!(html.contains("value=\"claude-code\"") && html.contains("value=\"opencode\""));
    assert!(
        html.contains("Required. Only models of the provider whose wire API the harness can use.")
    );
    assert!(!html.contains("Optional."));
}

#[tokio::test]
async fn refreshing_the_persona_form_keeps_its_create_or_edit_mode() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await.to_string();
    let id = uuid::Uuid::now_v7().to_string();
    let body = persona("claude-code", &provider, "claude-sonnet-5-5");

    let created = text(app.post("/personas/refresh", &body, &cookie).await).await;
    let edited = text(
        app.post(
            "/personas/refresh",
            &format!("{body}&entity_id={id}"),
            &cookie,
        )
        .await,
    )
    .await;

    assert!(created.contains("hx-post=\"/personas\"") && created.contains("Create persona"));
    assert!(
        edited.contains(&format!("hx-post=\"/personas/{id}\"")) && edited.contains("Save persona")
    );
}

#[test]
fn a_model_conflict_is_shown_under_the_model_and_a_duplicate_under_the_name() {
    let model = save_error(DbError::ModelConflict("not a model".into())).unwrap();
    let name = save_error(DbError::Conflict("duplicate".into())).unwrap();

    assert_eq!(model.get("model"), Some("not a model"));
    assert_eq!(model.get("name"), None);
    assert_eq!(name.get("name"), Some("duplicate"));
    assert_eq!(name.get("model"), None);
}

fn described(name: &str, description: &str, provider: &str) -> String {
    form(&[
        ("name", name),
        ("description", description),
        ("system_prompt", "You review pull requests with great care."),
        ("model", "claude-sonnet-5-5"),
        ("model_provider_id", provider),
    ])
}

#[tokio::test]
async fn the_card_shows_the_description_in_place_of_the_start_of_the_system_prompt() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await.to_string();
    let saved = app
        .post(
            "/personas",
            &described(
                "Reviewer",
                "Reviews Rust PRs for correctness and tests",
                &provider,
            ),
            &cookie,
        )
        .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let plain = app
        .post("/personas", &described("Plain", "", &provider), &cookie)
        .await;
    assert_eq!(plain.status(), StatusCode::OK);

    let page = text(app.get("/personas", &cookie).await).await;

    let card = |name: &str| {
        page.split("<li ")
            .find(|card| card.contains(&format!(">{name}<")))
            .unwrap_or_else(|| panic!("{name}: {page}"))
    };
    let reviewer = card("Reviewer");
    assert!(reviewer.contains("Reviews Rust PRs for correctness and tests"));
    assert!(!reviewer.contains("You review pull requests"), "{reviewer}");
    let plain = card("Plain");
    assert!(
        plain.contains("You review pull requests with great care."),
        "{plain}"
    );
}

#[tokio::test]
async fn a_description_over_500_characters_is_rejected_and_the_persona_is_unchanged() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await.to_string();
    app.post(
        "/personas",
        &described("Reviewer", "short", &provider),
        &cookie,
    )
    .await;
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM personas WHERE name = 'Reviewer'")
        .fetch_one(&app.pool)
        .await
        .unwrap();

    let at_limit = "x".repeat(500);
    let ok = app
        .post(
            &format!("/personas/{id}"),
            &described("Reviewer", &at_limit, &provider),
            &cookie,
        )
        .await;
    assert_eq!(ok.status(), StatusCode::OK);

    let too_long = "y".repeat(501);
    let response = app
        .post(
            &format!("/personas/{id}"),
            &described("Reviewer", &too_long, &provider),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("Description is at most 500 characters."),
        "{html}"
    );
    assert!(html.contains(">501</span> / 500"), "{html}");
    let stored: String = sqlx::query_scalar("SELECT description FROM personas WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(stored, at_limit);
}

#[tokio::test]
async fn the_form_has_a_description_with_a_counter_between_name_and_system_prompt() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await.to_string();
    app.post(
        "/personas",
        &described("Reviewer", "Reviews PRs", &provider),
        &cookie,
    )
    .await;
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM personas WHERE name = 'Reviewer'")
        .fetch_one(&app.pool)
        .await
        .unwrap();

    let html = text(app.get_htmx(&format!("/personas/{id}/edit"), &cookie).await).await;

    let at = |needle: &str| {
        html.find(needle)
            .unwrap_or_else(|| panic!("{needle}: {html}"))
    };
    assert!(at(r#"name="name""#) < at(r#"name="description""#));
    assert!(at(r#"name="description""#) < at(r#"name="system_prompt""#));
    assert!(html.contains(">11</span> / 500"), "{html}");
    let new_form = text(app.get_htmx("/personas/new", &cookie).await).await;
    assert!(new_form.contains(">0</span> / 500"), "{new_form}");
}

#[tokio::test]
async fn persona_is_created_selectable_and_edits_are_audited() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await.to_string();
    seed(&app.pool).await;
    let body = form(&[
        ("name", "Torvalds"),
        ("system_prompt", "old prompt"),
        ("model", "claude-sonnet-5-5"),
        ("model_provider_id", &provider),
    ]);
    let response = app.post("/personas", &body, &cookie).await;
    assert_saved(&response);
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM personas WHERE name = 'Torvalds'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let new_task_page = text(app.get("/tasks/compose", &cookie).await).await;
    assert!(new_task_page.contains("Torvalds") && new_task_page.contains("Linus"));

    let edited = form(&[
        ("name", "Torvalds"),
        ("system_prompt", "new prompt"),
        ("model", "claude-sonnet-5-5"),
        ("model_provider_id", &provider),
    ]);
    let response = app.post(&format!("/personas/{id}"), &edited, &cookie).await;
    assert_saved(&response);
    let updates = audit(&app, "persona.update").await;
    assert_eq!(updates[0]["before"]["system_prompt"], "old prompt");
    assert_eq!(updates[0]["after"]["system_prompt"], "new prompt");
}

#[tokio::test]
async fn the_persona_form_says_which_model_providers_it_offers() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed_provider(&app.pool).await;

    let html = text(app.get_htmx("/personas/new", &cookie).await).await;

    assert!(
        html.contains("Only providers that offer a wire API of the harness."),
        "{html}"
    );
}

#[tokio::test]
async fn persona_requires_name_prompt_and_a_known_model_provider() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let body = form(&[
        ("name", ""),
        ("system_prompt", ""),
        ("model_provider_id", &uuid::Uuid::now_v7().to_string()),
    ]);
    let html = text(app.post("/personas", &body, &cookie).await).await;
    assert!(
        html.contains("Name is required.")
            && html.contains("System prompt is required.")
            && html.contains("Choose a model provider.")
    );
}

#[tokio::test]
async fn personas_are_cards_with_harness_model_prompt_excerpt_and_model_provider() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await.to_string();
    let linus = form(&[
        ("name", "Linus"),
        (
            "system_prompt",
            "Be Linus.\nReview carefully.\nShip small changes.\nNever skip tests.",
        ),
        ("model", "claude-sonnet-5-5"),
        ("model_provider_id", &provider),
    ]);
    app.post("/personas", &linus, &cookie).await;
    let plain = form(&[
        ("name", "Ada"),
        ("system_prompt", "Be Ada."),
        ("model", "claude-sonnet-5-5"),
        ("model_provider_id", &provider),
    ]);
    app.post("/personas", &plain, &cookie).await;

    let html = text(app.get("/personas", &cookie).await).await;
    let card = |name: &str| {
        let at = html.find(&format!(">{name}</h2>")).unwrap();
        let start = html[..at].rfind("<li ").unwrap();
        html[start..start + html[start..].find("</li>").unwrap()].to_string()
    };
    let linus = card("Linus");
    assert!(linus.contains(">L</span>"), "initial: {linus}");
    assert!(linus.contains("Claude Code · claude-sonnet-5-5"), "{linus}");
    assert!(linus.contains("line-clamp-3") && linus.contains("Be Linus."));
    assert!(linus.contains(">Claude subscription<"), "{linus}");
    assert!(linus.contains("aria-label=\"Edit persona Linus\"") && linus.contains("/edit\""));
    let ada = card("Ada");
    assert!(
        ada.contains("Claude Code · claude-sonnet-5-5") && ada.contains(">Claude subscription<"),
        "{ada}"
    );
}

#[tokio::test]
async fn the_persona_form_pairs_harness_and_model() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get_htmx("/personas/new", &cookie).await).await;
    assert!(html.contains("md:grid-cols-2"));
    for full in ["name", "system_prompt", "model"] {
        assert!(
            html.contains(&format!(
                "<div class=\"md:col-span-2\"><div class=\"space-y-1\"><label for=\"{full}\""
            )),
            "{full}"
        );
    }
    for half in ["harness", "model_provider_id"] {
        assert!(!html.contains(&format!("<div class=\"md:col-span-2\"><div class=\"space-y-1\"><label class=\"block text-sm font-medium text-muted\" for=\"{half}\"")), "{half}");
        assert!(html.contains(&format!("for=\"{half}\"")));
    }
    assert!(
        html.find("for=\"harness\"").unwrap() < html.find("for=\"model_provider_id\"").unwrap()
    );
    assert!(html.find("for=\"model_provider_id\"").unwrap() < html.find("for=\"model\"").unwrap());
    assert!(
        !html.contains("adapter_command"),
        "persona form has no adapter command"
    );
}
