//! Tests of the "Incoming webhook" group of the Integrations tab and of the secret endpoint
//! (`forge-webhooks` spec).

use axum::http::StatusCode;

use crate::testkit::{
    FakeWebhooks, TestApp, WEBHOOK_SECRET, default_forge, forge_routes, header_str, seed,
    tag_around, text,
};
use crate::webhooks::WebhookSetup;

const PAGE: &str = "/projects/DF?tab=integrations";
const SECRET: &str = "/projects/DF/webhook-secret";

/// A credential for project `DF`, so a forge plugin serves it.
async fn give_forge_credential(app: &TestApp) {
    sqlx::query(
        "WITH c AS (INSERT INTO credential_refs (id, name, secret_name, secret_key)
                    VALUES (gen_random_uuid(), 'forge', 's', 'k') RETURNING id)
         UPDATE projects SET forge_credential_id = (SELECT id FROM c) WHERE key = 'DF'",
    )
    .execute(&app.pool)
    .await
    .unwrap();
}

/// An app on `https://factory.example.org` with a ready `forgejo` that serves project `DF`.
async fn ready_app() -> TestApp {
    let app = TestApp::with_webhooks(
        default_forge("forgejo"),
        Some("https://factory.example.org"),
        FakeWebhooks::ready("forgejo", WEBHOOK_SECRET),
    )
    .await;
    seed(&app.pool).await;
    give_forge_credential(&app).await;
    app
}

/// The markup of the "Incoming webhook" group.
fn group(html: &str) -> &str {
    let start = html
        .find("<section aria-labelledby=\"group-incoming-webhook\"")
        .unwrap_or_else(|| panic!("no group: {html}"));
    let rest = &html[start..];
    &rest[..rest.find("</section>").unwrap()]
}

#[tokio::test]
async fn the_group_shows_the_full_url_from_the_public_url_and_the_shared_secret_note() {
    let app = ready_app().await;
    let html = text(app.get(PAGE, &app.login_admin().await).await).await;
    let group = group(&html);

    assert!(
        group.contains("value=\"https://factory.example.org/webhooks/forgejo\""),
        "{group}"
    );
    assert!(group.contains("Webhook URL"), "{group}");
    assert!(
        group.contains("shared by every project") && group.contains("forgejo"),
        "{group}"
    );
    assert!(
        group.contains(
            "href=\"https://www.dark-factory.eu/docs/2026.11.0/integrations/forges/webhooks/\""
        ),
        "{group}"
    );
}

#[tokio::test]
async fn without_a_public_url_the_group_shows_the_path_and_a_hint_naming_it() {
    let app = TestApp::with_webhooks(
        default_forge("forgejo"),
        None,
        FakeWebhooks::ready("forgejo", WEBHOOK_SECRET),
    )
    .await;
    seed(&app.pool).await;
    give_forge_credential(&app).await;

    let html = text(app.get(PAGE, &app.login_admin().await).await).await;
    let group = group(&html);

    assert!(group.contains("value=\"/webhooks/forgejo\""), "{group}");
    assert!(
        group.contains("`publicUrl`") || group.contains("publicUrl"),
        "{group}"
    );
    assert!(!group.contains("https://factory"), "{group}");
}

#[tokio::test]
async fn each_unavailable_state_says_why_and_offers_no_secret_controls() {
    struct Case {
        name: &'static str,
        app: TestApp,
        credential: bool,
        says: &'static str,
    }
    let cases = [
        Case {
            name: "no plugin",
            app: TestApp::with_webhooks(
                df_core::routing::ForgeRoutes::default(),
                None,
                FakeWebhooks::default(),
            )
            .await,
            credential: true,
            says: "No forge integration serves this project",
        },
        Case {
            name: "no forge credential",
            app: TestApp::with_webhooks(
                default_forge("forgejo"),
                None,
                FakeWebhooks::ready("forgejo", WEBHOOK_SECRET),
            )
            .await,
            credential: false,
            says: "forge credential",
        },
        Case {
            name: "not loaded",
            app: TestApp::with_webhooks(
                forge_routes(&[("forgejo", &[])], &[]),
                None,
                FakeWebhooks::default(),
            )
            .await,
            credential: true,
            says: "did not load",
        },
        Case {
            // Routing serves the project, the plugin host reports it as not loaded.
            name: "served but reported as not loaded by the receiver",
            app: TestApp::with_webhooks(default_forge("forgejo"), None, FakeWebhooks::default())
                .await,
            credential: true,
            says: "did not load",
        },
        Case {
            name: "no webhooks export",
            app: TestApp::with_webhooks(
                default_forge("forgejo"),
                None,
                FakeWebhooks::with("forgejo", WebhookSetup::Unsupported, None),
            )
            .await,
            credential: true,
            says: "does not support webhooks",
        },
        Case {
            name: "no secret",
            app: TestApp::with_webhooks(
                default_forge("forgejo"),
                None,
                FakeWebhooks::with("forgejo", WebhookSetup::NoSecret, None),
            )
            .await,
            credential: true,
            says: "Incoming webhooks are off for forgejo",
        },
    ];
    for case in cases {
        seed(&case.app.pool).await;
        if case.credential {
            give_forge_credential(&case.app).await;
        }
        let cookie = case.app.login_admin().await;
        let html = text(case.app.get(PAGE, &cookie).await).await;
        let group = group(&html);
        assert!(group.contains(case.says), "{}: {group}", case.name);
        assert!(
            group.contains("bg-sunk"),
            "{}: a grey panel: {group}",
            case.name
        );
        for absent in [
            "Show secret",
            "Copy secret",
            "Copy webhook URL",
            "webhook-secret",
        ] {
            assert!(!group.contains(absent), "{}: {absent}", case.name);
        }
        let response = case.app.get(SECRET, &cookie).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{}", case.name);
    }
}

#[tokio::test]
async fn the_page_masks_the_secret_and_never_contains_it() {
    let app = ready_app().await;
    let html = text(app.get(PAGE, &app.login_admin().await).await).await;
    let group = group(&html);

    assert!(group.contains("••••••••••••••••"), "{group}");
    // The component gets the mask from the page, so the script has no copy of it.
    assert!(
        group.contains("&quot;, &quot;••••••••••••••••&quot;)\""),
        "{group}"
    );
    assert!(
        !crate::assets::script_source("secret-field.ts").contains('•'),
        "the script holds no mask of its own"
    );
    assert!(
        !html.contains(WEBHOOK_SECRET),
        "the secret is not in the page"
    );
    assert!(group.contains("/projects/DF/webhook-secret"), "{group}");
}

#[tokio::test]
async fn the_buttons_have_accessible_names_and_the_https_note_is_hidden_until_needed() {
    let app = ready_app().await;
    let html = text(app.get(PAGE, &app.login_admin().await).await).await;
    let group = group(&html);

    for name in ["Show secret", "Copy secret", "Copy webhook URL"] {
        assert!(
            group.contains(&format!("aria-label=\"{name}\"")),
            "{name}: {group}"
        );
    }
    let eye = tag_around(group, "aria-label=\"Show secret\"");
    assert!(eye.contains("aria-pressed=\"false\""), "{eye}");
    // The copy buttons wait for the client to find a secure clipboard; the note says why not.
    for name in ["Copy secret", "Copy webhook URL"] {
        let tag = tag_around(group, &format!("aria-label=\"{name}\""));
        assert!(
            tag.contains("x-show=\"canCopy\"") && tag.contains("x-cloak"),
            "{tag}"
        );
    }
    let note = tag_around(group, "Copying needs HTTPS");
    assert!(
        note.contains("x-show=\"!canCopy\"") && note.contains("x-cloak"),
        "{note}"
    );
    assert!(group.contains("Copying needs HTTPS. Use Show secret and copy the value by hand."));
    // The confirmation is a polite live region.
    assert!(
        group.contains("role=\"status\"") && group.contains("aria-live=\"polite\""),
        "{group}"
    );
}

#[tokio::test]
async fn an_admin_reads_the_secret_as_plain_text_without_caching_and_the_read_is_audited() {
    let app = ready_app().await;
    let response = app.get(SECRET, &app.login_admin().await).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(header_str(&response, "content-type").starts_with("text/plain"));
    assert_eq!(header_str(&response, "cache-control"), "no-store");
    assert_eq!(text(response).await, WEBHOOK_SECRET);

    let entries: Vec<(serde_json::Value, String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT actor, resource_type, resource_id, details FROM audit_log WHERE action = 'webhook_secret.read'",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(entries.len(), 1);
    let (actor, resource_type, resource_id, details) = &entries[0];
    assert_eq!(resource_type, "webhook_secret");
    assert_eq!(resource_id, "forgejo");
    assert_eq!(details["project"], "DF");
    assert_eq!(details["plugin"], "forgejo");
    assert!(
        actor.to_string().contains("sub") || !actor.is_null(),
        "{actor}"
    );
    assert!(!details.to_string().contains(WEBHOOK_SECRET));
}

#[tokio::test]
async fn a_board_member_cannot_read_the_secret_and_nothing_is_audited() {
    let app = ready_app().await;
    let response = app.get(SECRET, &app.login().await).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'webhook_secret.read'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn an_unknown_project_has_no_secret() {
    let app = ready_app().await;
    let response = app
        .get("/projects/NOPE/webhook-secret", &app.login_admin().await)
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
