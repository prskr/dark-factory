//! Tests of the project subscription settings (`event-subscriptions` spec).

use axum::http::StatusCode;
use df_core::EventType;
use df_db::event_subscriptions::{self, Subscription};

use crate::testkit::{TestApp, form, header_str, seed, text};

const PAGE: &str = "/projects/DF/subscriptions";

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

    let empty = app.get(PAGE, &cookie).await;
    assert_eq!(empty.status(), StatusCode::OK);
    assert!(text(empty).await.contains("No subscriptions yet"));

    let modal = text(app.get_htmx(&format!("{PAGE}/new"), &cookie).await).await;
    assert!(modal.contains("value=\"sandbox.provisioned\""), "{modal}");

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
    assert_eq!(header_str(&created, "HX-Retarget"), "#config-list");
    assert!(text(created).await.contains("https://hooks.example.org/a"));
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
        edit.contains("value=\"sandbox.destroyed\" checked"),
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
    let page = text(app.get(PAGE, &cookie).await).await;
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

    let cases: [(&[(&str, &str)], &str); 6] = [
        (&[("url", "http://example.com/hook")], "https"),
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
    }
    assert!(subscriptions(&app, seeded.project).await.is_empty());
    assert!(audit_actions(&app).await.is_empty());
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

    let page = text(app.get(PAGE, &cookie).await).await;
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

    let page = text(app.get(PAGE, &cookie).await).await;
    assert!(page.contains("Lifecycle events are off"), "{page}");
}

#[tokio::test]
async fn unknown_projects_and_other_projects_subscriptions_are_not_found() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let cookie = app.login_admin().await;
    let id = create(&app, &cookie, "https://hooks.example.org/", &[]).await;

    assert_eq!(
        app.get("/projects/NOPE/subscriptions", &cookie)
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
async fn the_project_list_links_the_settings_for_admins_only() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let link = "href=\"/projects/DF/subscriptions\"";

    let admin = text(app.get("/projects", &app.login_admin().await).await).await;
    assert!(admin.contains(link), "{admin}");
    let member = text(app.get("/projects", &app.login().await).await).await;
    assert!(!member.contains(link));
}
