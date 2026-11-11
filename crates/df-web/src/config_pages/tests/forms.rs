//! Tests of the shared behavior of the modal forms.

use axum::http::StatusCode;

use super::credential_form_body;
use crate::testkit::{TestApp, form, header_str, seed, seed_provider, tag_around, text};

#[tokio::test]
async fn unknown_resources_are_404() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    for path in [
        "/projects/NOPE",
        "/personas/00000000-0000-0000-0000-000000000000/edit",
        "/credentials/00000000-0000-0000-0000-000000000000/edit",
    ] {
        assert_eq!(
            app.get_htmx(path, &cookie).await.status(),
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}

#[tokio::test]
async fn forms_are_modal_fragments_and_plain_requests_go_to_the_list() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    for (fragment, list, title) in [
        ("/projects/new", "/projects", "New project"),
        ("/personas/new", "/personas", "New persona"),
        (
            "/credentials/new",
            "/credentials",
            "New credential reference",
        ),
    ] {
        let modal = text(app.get_htmx(fragment, &cookie).await).await;
        assert!(
            modal.starts_with("<dialog id=\"modal\""),
            "{fragment}: {modal}"
        );
        assert!(
            modal.contains(title) && modal.contains("h-dvh"),
            "full-height sheet on phones"
        );
        assert!(modal.contains("<form id=\"form\""));
        assert!(!modal.contains("<html"), "a fragment, not a page");
        let plain = app.get(fragment, &cookie).await;
        assert_eq!(plain.status(), StatusCode::SEE_OTHER, "{fragment}");
        assert_eq!(header_str(&plain, "location"), list);
    }
}

#[tokio::test]
async fn the_full_page_form_routes_are_gone() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    for path in [
        "/personas/00000000-0000-0000-0000-000000000000",
        "/credentials/00000000-0000-0000-0000-000000000000",
    ] {
        let response = app.get(path, &cookie).await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
    }
}

#[tokio::test]
async fn a_validation_error_keeps_the_modal_open_with_values_and_the_error_at_the_field() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let response = app
        .post(
            "/credentials",
            &credential_form_body("kept", "secret", ""),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    // No retarget: the form replaces itself inside the open modal.
    assert_eq!(header_str(&response, "hx-retarget"), "");
    assert_eq!(header_str(&response, "hx-trigger"), "");
    let html = text(response).await;
    assert!(html.starts_with("<form id=\"form\""), "{html}");
    assert!(
        html.contains("Key is required.")
            && html.contains("value=\"kept\"")
            && html.contains("value=\"secret\"")
    );
    let key_error = html.find("id=\"secret_key-error\"").unwrap();
    assert!(html.find("id=\"secret_key\"").unwrap() < key_error);
}

#[tokio::test]
async fn forms_end_with_cancel_and_a_named_primary_action_at_the_bottom_right() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await.to_string();
    seed(&app.pool).await;
    let persona = form(&[
        ("name", "Ada"),
        ("system_prompt", "x"),
        ("model", "claude-sonnet-5-5"),
        ("model_provider_id", &provider),
    ]);
    app.post("/personas", &persona, &cookie).await;
    app.post(
        "/credentials",
        &credential_form_body("c", "s", "k"),
        &cookie,
    )
    .await;
    let persona_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM personas WHERE name = 'Ada'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let credential_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM credential_refs")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    for (uri, label) in [
        ("/projects/new".to_string(), "Create project"),
        ("/personas/new".to_string(), "Create persona"),
        (format!("/personas/{persona_id}/edit"), "Save persona"),
        (
            "/credentials/new".to_string(),
            "Create credential reference",
        ),
        (
            format!("/credentials/{credential_id}/edit"),
            "Save credential reference",
        ),
    ] {
        let html = text(app.get_htmx(&uri, &cookie).await).await;
        let footer = &html[html
            .find("justify-end")
            .unwrap_or_else(|| panic!("{uri}: {html}"))..];
        let footer = &footer[..footer.find("</form>").unwrap()];
        let cancel = footer
            .find(">Cancel</button>")
            .unwrap_or_else(|| panic!("{uri}: {footer}"));
        let primary = footer
            .find(&format!(">{label}</button>"))
            .unwrap_or_else(|| panic!("{uri}: {footer}"));
        assert!(cancel < primary, "{uri}: Cancel before the primary action");
        // Cancel only closes the dialog; the primary action submits.
        let cancel_tag = tag_around(footer, ">Cancel</button>");
        assert!(
            cancel_tag.contains("type=\"button\""),
            "{uri}: {cancel_tag}"
        );
        assert!(
            cancel_tag.contains("closest('dialog').close()"),
            "{uri}: {cancel_tag}"
        );
        assert!(footer[..primary].contains("type=\"submit\""), "{uri}");
        assert!(
            footer.contains("bg-accent"),
            "{uri}: primary is the accent fill"
        );
    }
}
