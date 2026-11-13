//! Tests of the "Lifecycle events" group of the project settings page (`event-subscriptions` spec).

use axum::http::StatusCode;
use df_core::EventType;
use df_db::event_subscriptions::{self, Subscription};

use crate::testkit::{TestApp, form, header_str, seed, tag_around, text};

const PAGE: &str = "/projects/DF/subscriptions";
const SETTINGS_PAGE: &str = "/projects/DF";

/// The tab fragment an admin's htmx request gets.
async fn tab(app: &TestApp, cookie: &str) -> String {
    text(app.get_htmx(PAGE, cookie).await).await
}

async fn subscriptions(app: &TestApp, project: uuid::Uuid) -> Vec<Subscription> {
    event_subscriptions::list(&app.pool, project).await.unwrap()
}

async fn audit_actions(app: &TestApp) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT action FROM audit_log WHERE resource_type = 'event_subscription' ORDER BY id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap()
}

/// Creates a subscription through the form and returns its ID.
async fn create(app: &TestApp, cookie: &str, url: &str, events: &[&str]) -> uuid::Uuid {
    let mut pairs = vec![("url", url)];
    pairs.extend(events.iter().map(|e| ("events", *e)));
    let response = app.post(PAGE, &form(&pairs), cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    let project = df_db::catalog::get_project_by_key(&app.pool, "DF")
        .await
        .unwrap()
        .unwrap();
    subscriptions(app, project.id).await.last().unwrap().id
}

#[tokio::test]
async fn a_board_member_without_admin_gets_403_naming_the_role() {
    let app = TestApp::new().await;
    let seeded = seed(&app.pool).await;
    let cookie = app.login().await;
    let body = form(&[("url", "https://hooks.example.org/a")]);

    let responses = [
        app.get(PAGE, &cookie).await,
        app.get_htmx(PAGE, &cookie).await,
        app.get_htmx(&format!("{PAGE}/new"), &cookie).await,
        app.post(PAGE, &body, &cookie).await,
        app.delete(&format!("{PAGE}/{}", uuid::Uuid::now_v7()), &cookie)
            .await,
    ];
    for response in responses {
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(text(response).await.contains("`admin`"));
    }
    assert!(subscriptions(&app, seeded.project).await.is_empty());
}

#[tokio::test]
async fn an_admin_lists_creates_edits_and_deletes_subscriptions() {
    let app = TestApp::new().await;
    let seeded = seed(&app.pool).await;
    let cookie = app.login_admin().await;

    assert!(tab(&app, &cookie).await.contains("No subscriptions yet"));

    let editor = text(app.get_htmx(&format!("{PAGE}/new"), &cookie).await).await;
    assert!(editor.contains("value=\"sandbox.provisioned\""), "{editor}");

    let created = app
        .post(
            PAGE,
            &form(&[
                ("url", " https://hooks.example.org/a "),
                ("events", "sandbox.destroyed"),
                ("events", "volume.destroyed"),
            ]),
            &cookie,
        )
        .await;
    assert_eq!(created.status(), StatusCode::OK);
    assert_eq!(header_str(&created, "HX-Retarget"), "#lifecycle-events");
    let region = text(created).await;
    assert!(region.contains("https://hooks.example.org/a"), "{region}");
    assert!(region.contains("id=\"lifecycle-events\""), "{region}");
    assert!(!region.contains("id=\"subscription-editor\""), "{region}");
    assert!(region.contains("No delivery yet"), "{region}");
    let stored = subscriptions(&app, seeded.project).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].url, "https://hooks.example.org/a");
    assert_eq!(
        stored[0].events,
        [EventType::VolumeDestroyed, EventType::SandboxDestroyed]
    );
    let id = stored[0].id;

    let edit = text(app.get_htmx(&format!("{PAGE}/{id}/edit"), &cookie).await).await;
    assert!(
        edit.contains("value=\"https://hooks.example.org/a\""),
        "{edit}"
    );
    assert!(
        edit.contains("value=\"sandbox.destroyed\" form=\"subscription-form\" checked"),
        "{edit}"
    );

    let updated = app
        .post(
            &format!("{PAGE}/{id}"),
            &form(&[("url", "https://hooks.example.org/b")]),
            &cookie,
        )
        .await;
    assert_eq!(updated.status(), StatusCode::OK);
    let page = tab(&app, &cookie).await;
    assert!(page.contains("https://hooks.example.org/b"));
    assert!(page.contains("All event types"), "{page}");
    assert!(
        subscriptions(&app, seeded.project).await[0]
            .events
            .is_empty()
    );

    let deleted = app.delete(&format!("{PAGE}/{id}"), &cookie).await;
    assert_eq!(deleted.status(), StatusCode::OK);
    assert!(!text(deleted).await.contains("https://hooks.example.org/b"));
    assert!(subscriptions(&app, seeded.project).await.is_empty());
    assert_eq!(
        audit_actions(&app).await,
        [
            "event_subscription.create",
            "event_subscription.update",
            "event_subscription.delete"
        ]
    );
}

#[tokio::test]
async fn plain_http_internal_addresses_and_unknown_events_are_rejected_on_save() {
    let app = TestApp::new().await;
    let seeded = seed(&app.pool).await;
    let cookie = app.login_admin().await;

    let cases: [(&[(&str, &str)], &str); 8] = [
        (&[("url", "http://example.com/hook")], "https"),
        (
            &[("url", "https://user:secret@hooks.example.org/df")],
            "username or password",
        ),
        (
            &[("url", "https://token@hooks.example.org/df")],
            "username or password",
        ),
        (&[("url", "https://10.0.0.5/hook")], "public"),
        (&[("url", "https://169.254.169.254/latest")], "public"),
        (&[("url", "https://[::1]:8443/")], "public"),
        (&[("url", "not a url")], "valid URL"),
        (
            &[
                ("url", "https://hooks.example.org/"),
                ("events", "task.created"),
            ],
            "Unknown event type",
        ),
    ];
    for (pairs, message) in cases {
        let response = app.post(PAGE, &form(pairs), &cookie).await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = text(response).await;
        assert!(body.contains(message), "{pairs:?}: {body}");
        assert!(
            tag_around(&body, "id=\"url\"").contains("font-mono"),
            "{body}"
        );
    }
    assert!(subscriptions(&app, seeded.project).await.is_empty());
    assert!(audit_actions(&app).await.is_empty());
}

#[tokio::test]
async fn unrestricted_targets_accept_plain_http_and_internal_hosts() {
    let app = TestApp::with_unrestricted_project_targets().await;
    let seeded = seed(&app.pool).await;
    let cookie = app.login_admin().await;

    create(&app, &cookie, "http://receiver.receiver.svc:8080/hook", &[]).await;
    create(&app, &cookie, "http://10.0.0.5/hook", &[]).await;
    let page = tab(&app, &cookie).await;

    let urls: Vec<String> = subscriptions(&app, seeded.project)
        .await
        .into_iter()
        .map(|s| s.url)
        .collect();
    assert_eq!(
        urls,
        [
            "http://receiver.receiver.svc:8080/hook",
            "http://10.0.0.5/hook"
        ]
    );
    assert!(page.contains("unrestricted"), "{page}");
    let invalid = app
        .post(PAGE, &form(&[("url", "not a url")]), &cookie)
        .await;
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let with_credentials = app
        .post(
            PAGE,
            &form(&[("url", "http://user:secret@10.0.0.5/hook")]),
            &cookie,
        )
        .await;
    assert_eq!(with_credentials.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(subscriptions(&app, seeded.project).await.len(), 2);
}

#[tokio::test]
async fn the_url_hint_follows_the_url_restrictions() {
    let restricted = TestApp::new().await;
    seed(&restricted.pool).await;
    let cookie = restricted.login_admin().await;
    let editor = text(restricted.get_htmx(&format!("{PAGE}/new"), &cookie).await).await;
    assert!(editor.contains("HTTPS only."), "{editor}");

    let unrestricted = TestApp::with_unrestricted_project_targets().await;
    seed(&unrestricted.pool).await;
    let cookie = unrestricted.login_admin().await;
    let editor = text(unrestricted.get_htmx(&format!("{PAGE}/new"), &cookie).await).await;
    assert!(!editor.contains("HTTPS only."), "{editor}");
    assert!(editor.contains("http or https"), "{editor}");
}

#[tokio::test]
async fn the_settings_show_the_last_delivery_and_the_public_key() {
    let app = TestApp::with_public_key("whpk_dGVzdC1rZXk").await;
    seed(&app.pool).await;
    let cookie = app.login_admin().await;
    let failing = create(&app, &cookie, "https://down.example.org/", &[]).await;
    let ok = create(&app, &cookie, "https://up.example.org/", &[]).await;
    let rejected = create(&app, &cookie, "https://busy.example.org/", &[]).await;
    create(&app, &cookie, "https://new.example.org/", &[]).await;
    event_subscriptions_attempt(&app, failing, None, Some("timed out after 10s")).await;
    event_subscriptions_attempt(&app, ok, Some(204), None).await;
    event_subscriptions_attempt(&app, rejected, Some(503), None).await;

    let page = tab(&app, &cookie).await;
    assert!(page.contains("timed out after 10s"), "{page}");
    assert!(page.contains("<span>HTTP 204</span>"), "{page}");
    assert!(
        page.contains("<span class=\"text-danger\">HTTP 503</span>"),
        "{page}"
    );
    assert!(page.contains("No delivery yet"), "{page}");
    assert!(page.contains("whpk_dGVzdC1rZXk"), "{page}");
    assert!(
        page.contains("href=\"/lifecycle-events/public-key\""),
        "{page}"
    );
}

async fn event_subscriptions_attempt(
    app: &TestApp,
    id: uuid::Uuid,
    status: Option<u16>,
    error: Option<&str>,
) {
    df_db::lifecycle_events::record_attempt(&app.pool, id, status, error)
        .await
        .unwrap();
}

#[tokio::test]
async fn without_a_signing_key_the_settings_say_events_are_off() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let cookie = app.login_admin().await;

    create(&app, &cookie, "https://hooks.example.org/kept", &[]).await;

    let page = tab(&app, &cookie).await;
    assert!(page.contains("Lifecycle events are off"), "{page}");
    assert!(page.contains("https://hooks.example.org/kept"), "{page}");
}

#[tokio::test]
async fn unknown_projects_and_other_projects_subscriptions_are_not_found() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let cookie = app.login_admin().await;
    let id = create(&app, &cookie, "https://hooks.example.org/", &[]).await;

    assert_eq!(
        app.get_htmx("/projects/NOPE/subscriptions", &cookie)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    df_db::catalog::save_project(
        &app.pool,
        df_db::catalog::Actor {
            principal: &crate::testkit::actor().0,
            roles: &[],
        },
        None,
        &df_db::catalog::ProjectInput {
            key: "WEB".into(),
            name: "Web".into(),
            repo_url: "https://git.example.org/peter/web.git".into(),
            issue_tracker_url: None,
            image: "img".into(),
            base_branch: "main".into(),
            adapter_command: vec![],
            forge_credential_id: None,
            webhook_wake: df_core::webhooks::WebhookWake::Members,
            branch_rewrite_policy: df_core::RewritePolicy::default(),
            agent_branch_globs: df_core::default_agent_branch_globs(),
        },
    )
    .await
    .unwrap();
    let other = "/projects/WEB/subscriptions";
    let edit = app.get_htmx(&format!("{other}/{id}/edit"), &cookie).await;
    assert_eq!(edit.status(), StatusCode::NOT_FOUND);
    let update = app
        .post(
            &format!("{other}/{id}"),
            &form(&[("url", "https://x.example.org/")]),
            &cookie,
        )
        .await;
    assert_eq!(update.status(), StatusCode::NOT_FOUND);
    let delete = app.delete(&format!("{other}/{id}"), &cookie).await;
    assert_eq!(delete.status(), StatusCode::NOT_FOUND);
    assert_eq!(audit_actions(&app).await, ["event_subscription.create"]);
}

#[tokio::test]
async fn the_tab_fragment_has_key_panel_list_and_new_action_without_a_page_frame() {
    let app = TestApp::with_public_key("whpk_dGVzdC1rZXk").await;
    seed(&app.pool).await;
    let cookie = app.login_admin().await;
    create(&app, &cookie, "https://hooks.example.org/a", &[]).await;

    let fragment = tab(&app, &cookie).await;
    assert!(
        fragment.starts_with("<div id=\"lifecycle-events\""),
        "{fragment}"
    );
    assert!(!fragment.contains("<header") && !fragment.contains("<html"));
    assert!(fragment.contains("whpk_dGVzdC1rZXk"), "{fragment}");
    assert!(fragment.contains("New subscription"), "{fragment}");
    assert!(
        fragment.contains("https://hooks.example.org/a"),
        "{fragment}"
    );
    assert!(
        fragment.contains("Subscriptions save on their own"),
        "{fragment}"
    );
}

#[tokio::test]
async fn the_editor_binds_its_inputs_to_the_separate_subscription_form() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let cookie = app.login_admin().await;
    let id = create(&app, &cookie, "https://hooks.example.org/a", &[]).await;

    for uri in [format!("{PAGE}/new"), format!("{PAGE}/{id}/edit")] {
        let editor = text(app.get_htmx(&uri, &cookie).await).await;
        assert!(editor.contains("id=\"subscription-editor\""), "{editor}");
        assert!(!editor.contains("<form"), "{editor}");
        let url = tag_around(&editor, "name=\"url\"");
        assert!(url.contains("form=\"subscription-form\""), "{url}");
        // Inputs (URL and one checkbox per event type) and the submit button.
        assert_eq!(
            editor.matches("form=\"subscription-form\"").count(),
            editor.matches("<input").count() + 1,
            "{editor}"
        );
        let submit = tag_around(&editor, "type=\"submit\"");
        assert!(submit.contains("form=\"subscription-form\""), "{submit}");
        assert!(submit.contains("hx-post"), "{submit}");
    }
}

#[tokio::test]
async fn the_admins_settings_page_has_one_subscription_form_outside_the_project_form() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let admin = text(app.get(SETTINGS_PAGE, &app.login_admin().await).await).await;
    assert_eq!(
        admin.matches("id=\"subscription-form\"").count(),
        1,
        "{admin}"
    );
    let project_form_end = admin.find("</form>").unwrap();
    assert!(admin.find("id=\"subscription-form\"").unwrap() > project_form_end);
    let member = text(app.get(SETTINGS_PAGE, &app.login().await).await).await;
    assert!(!member.contains("subscription-form"), "{member}");
}

#[tokio::test]
async fn an_invalid_url_returns_the_editor_with_the_values_and_the_field_error() {
    let app = TestApp::new().await;
    let seeded = seed(&app.pool).await;
    let cookie = app.login_admin().await;

    let response = app
        .post(
            PAGE,
            &form(&[
                ("url", "https://10.0.4.12/hooks/df"),
                ("events", "sandbox.destroyed"),
            ]),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(response.headers().get("HX-Retarget").is_none());
    let body = text(response).await;
    assert!(body.contains("id=\"subscription-editor\""), "{body}");
    assert!(
        body.contains("value=\"https://10.0.4.12/hooks/df\""),
        "{body}"
    );
    assert!(
        body.contains("value=\"sandbox.destroyed\" form=\"subscription-form\" checked"),
        "{body}"
    );
    assert!(body.contains("id=\"url-error\""), "{body}");
    assert!(subscriptions(&app, seeded.project).await.is_empty());
}

#[tokio::test]
async fn a_row_confirms_the_delete_and_only_the_confirmation_sends_it() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let cookie = app.login_admin().await;
    let id = create(&app, &cookie, "https://hooks.example.org/a", &[]).await;

    let page = tab(&app, &cookie).await;
    assert!(page.contains("x-data=\"{ confirming: false }\""), "{page}");
    assert!(
        page.contains("Delete this subscription? Deliveries to it stop at once."),
        "{page}"
    );
    assert_eq!(page.matches("hx-delete").count(), 1, "{page}");
    let delete = tag_around(&page, "hx-delete");
    assert!(delete.contains(&format!("{PAGE}/{id}")), "{delete}");
    assert!(page.contains(">Delete</button>"), "{page}");
    let trash = tag_around(
        &page,
        "aria-label=\"Delete subscription https://hooks.example.org/a\"",
    );
    assert!(
        trash.contains("confirming = true") && !trash.contains("hx-delete"),
        "{trash}"
    );
    let cancel = tag_around(&page, "confirming = false");
    assert!(!cancel.contains("hx-delete"), "{cancel}");
}

#[tokio::test]
async fn keyboard_focus_moves_into_the_delete_confirmation_and_back() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let cookie = app.login_admin().await;
    create(&app, &cookie, "https://hooks.example.org/a", &[]).await;

    let page = tab(&app, &cookie).await;
    let trash = tag_around(
        &page,
        "aria-label=\"Delete subscription https://hooks.example.org/a\"",
    );
    assert!(trash.contains("x-ref=\"trash\""), "{trash}");
    assert!(trash.contains("$refs.cancel.focus()"), "{trash}");
    let cancel = tag_around(&page, "confirming = false");
    assert!(cancel.contains("x-ref=\"cancel\""), "{cancel}");
    assert!(cancel.contains("$refs.trash.focus()"), "{cancel}");
}

#[tokio::test]
async fn an_empty_list_explains_subscriptions_and_offers_a_new_one() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let page = tab(&app, &app.login_admin().await).await;
    assert!(page.contains("No subscriptions yet"), "{page}");
    assert!(page.contains("sends signed events"), "{page}");
    assert_eq!(page.matches("New subscription").count(), 1, "{page}");
}

#[tokio::test]
async fn opening_an_editor_replaces_the_open_one_so_the_tab_holds_at_most_one() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let cookie = app.login_admin().await;
    let first = create(&app, &cookie, "https://hooks.example.org/a", &[]).await;
    let second = create(&app, &cookie, "https://hooks.example.org/b", &[]).await;

    let closed = tab(&app, &cookie).await;
    assert_eq!(closed.matches("id=\"subscription-editor\"").count(), 0);
    let new = text(app.get_htmx(&format!("{PAGE}/new"), &cookie).await).await;
    let edit_first = text(app.get_htmx(&format!("{PAGE}/{first}/edit"), &cookie).await).await;
    let edit_second = text(
        app.get_htmx(&format!("{PAGE}/{second}/edit"), &cookie)
            .await,
    )
    .await;

    // Every open answers with the whole tab, which has one editor and shows the other rows.
    for (opened, editing, row) in [
        (
            &new,
            "id=\"url\" name=\"url\" type=\"text\" value=\"\"",
            None,
        ),
        (
            &edit_first,
            "value=\"https://hooks.example.org/a\"",
            Some("b"),
        ),
        (
            &edit_second,
            "value=\"https://hooks.example.org/b\"",
            Some("a"),
        ),
    ] {
        assert!(
            opened.starts_with("<div id=\"lifecycle-events\""),
            "{opened}"
        );
        assert_eq!(
            opened.matches("id=\"subscription-editor\"").count(),
            1,
            "{opened}"
        );
        assert_eq!(opened.matches("id=\"url\"").count(), 1, "{opened}");
        assert!(
            tag_around(opened, "id=\"url\"").contains("font-mono"),
            "{opened}"
        );
        assert!(opened.contains(editing), "{opened}");
        if let Some(row) = row {
            assert!(
                opened.contains(&format!("hooks.example.org/{row}</p>")),
                "{opened}"
            );
        }
    }
}
