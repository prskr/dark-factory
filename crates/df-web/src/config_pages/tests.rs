use axum::http::StatusCode;

use crate::testkit::{TestApp, form, header_str, new_task, seed, text};

async fn audit(app: &TestApp, action: &str) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT details FROM audit_log WHERE action = $1 ORDER BY id")
        .bind(action)
        .fetch_all(&app.pool)
        .await
        .unwrap()
}

fn project_form(key: &str, repo: &str) -> String {
    form(&[
        ("key", key),
        ("name", "Dark Factory"),
        ("repo_url", repo),
        ("issue_tracker_url", ""),
        ("image", "ghcr.io/example/sandbox:1"),
        ("base_branch", "main"),
        ("forge_credential_id", ""),
    ])
}

#[tokio::test]
async fn credential_form_has_no_value_field() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get("/credentials/new", &cookie).await).await;
    let form_html = html.split("<form").nth(1).expect("form");
    let controls: Vec<&str> = form_html
        .split(['<'])
        .filter(|tag| {
            tag.starts_with("input ") || tag.starts_with("textarea") || tag.starts_with("select")
        })
        .map(|tag| {
            tag.split("name=\"")
                .nth(1)
                .and_then(|s| s.split('"').next())
                .unwrap_or("")
        })
        .collect();
    assert_eq!(controls, ["name", "secret_name", "secret_key"], "{html}");
    assert!(!html.contains("type=\"password\""));
}

#[tokio::test]
async fn credential_reference_is_created_listed_and_audited() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let body = form(&[
        ("name", "claude-subscription"),
        ("secret_name", "claude"),
        ("secret_key", "token"),
    ]);
    let response = app.post("/credentials", &body, &cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-redirect"), "/credentials");

    let row: (String, String, String) =
        sqlx::query_as("SELECT name, secret_name, secret_key FROM credential_refs")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        row,
        (
            "claude-subscription".into(),
            "claude".into(),
            "token".into()
        )
    );
    let list = text(app.get("/credentials", &cookie).await).await;
    assert!(list.contains("claude-subscription") && list.contains("claude"));
    assert_eq!(
        audit(&app, "credential_ref.create").await[0]["after"]["secret_key"],
        "token"
    );

    // Edit: change the key.
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM credential_refs")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let edited = form(&[
        ("name", "claude-subscription"),
        ("secret_name", "claude"),
        ("secret_key", "oauth"),
    ]);
    app.post(&format!("/credentials/{id}"), &edited, &cookie)
        .await;
    let updates = audit(&app, "credential_ref.update").await;
    assert_eq!(
        (
            updates[0]["before"]["secret_key"].clone(),
            updates[0]["after"]["secret_key"].clone()
        ),
        ("token".into(), "oauth".into())
    );
}

#[tokio::test]
async fn invalid_credential_references_are_not_stored() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let body = form(&[
        ("name", ""),
        ("secret_name", "Not Valid"),
        ("secret_key", "a b"),
    ]);
    let response = app.post("/credentials", &body, &cookie).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("Name is required.")
            && html.contains("Kubernetes name")
            && html.contains("letters, digits")
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM credential_refs")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn project_is_created_and_listed() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let response = app
        .post(
            "/projects",
            &project_form("DF", "https://git.example.org/peter/dark-factory.git"),
            &cookie,
        )
        .await;
    assert_eq!(header_str(&response, "hx-redirect"), "/projects");
    let list = text(app.get("/projects", &cookie).await).await;
    assert!(list.contains("DF") && list.contains("dark-factory.git"));
    assert_eq!(audit(&app, "project.create").await[0]["after"]["key"], "DF");
}

#[tokio::test]
async fn non_https_repository_is_rejected_with_a_validation_error() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    for repo in [
        "ssh://git@git.example.org/o/r.git",
        "http://git.example.org/o/r.git",
        "not a url",
    ] {
        let response = app
            .post("/projects", &project_form("DF", repo), &cookie)
            .await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{repo}"
        );
        let html = text(response).await;
        assert!(html.contains("Must be an HTTPS URL"), "{repo}");
        // The entered values stay in the form.
        assert!(html.contains("Dark Factory"));
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM projects")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn project_key_is_validated_and_unique() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let repo = "https://git.example.org/o/r.git";
    let bad = app
        .post("/projects", &project_form("df", repo), &cookie)
        .await;
    assert!(text(bad).await.contains("uppercase letters"));
    app.post("/projects", &project_form("DF", repo), &cookie)
        .await;
    let duplicate = app
        .post("/projects", &project_form("DF", repo), &cookie)
        .await;
    assert_eq!(duplicate.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(duplicate).await.contains("already exists"));
}

#[tokio::test]
async fn key_of_a_project_with_tasks_cannot_change() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let repo = "https://git.example.org/peter/dark-factory.git";
    // Without tasks the key may still change.
    let renamed = app
        .post("/projects/DF", &project_form("DFX", repo), &cookie)
        .await;
    assert_eq!(header_str(&renamed, "hx-redirect"), "/projects");
    new_task(&app.pool, &seed, "first").await;

    let rejected = app
        .post("/projects/DFX", &project_form("OTHER", repo), &cookie)
        .await;
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        text(rejected)
            .await
            .contains("cannot change once the project has tasks")
    );
    let key: String = sqlx::query_scalar("SELECT key FROM projects")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(key, "DFX");
    // Other fields of the same project can still be edited.
    let ok = app
        .post("/projects/DFX", &project_form("DFX", repo), &cookie)
        .await;
    assert_eq!(header_str(&ok, "hx-redirect"), "/projects");
    let edit_page = text(app.get("/projects/DFX", &cookie).await).await;
    assert!(edit_page.contains("cannot change once the project has tasks"));
}

#[tokio::test]
async fn persona_is_created_selectable_and_edits_are_audited() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let body = form(&[
        ("name", "Torvalds"),
        ("system_prompt", "old prompt"),
        ("adapter_command", "claude-code-acp --flag"),
        ("model", "opus"),
        ("harness_credential_id", ""),
    ]);
    let response = app.post("/personas", &body, &cookie).await;
    assert_eq!(header_str(&response, "hx-redirect"), "/personas");
    let (id, command): (uuid::Uuid, Vec<String>) =
        sqlx::query_as("SELECT id, adapter_command FROM personas WHERE name = 'Torvalds'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(command, ["claude-code-acp", "--flag"]);
    let new_task_page = text(app.get("/tasks/compose", &cookie).await).await;
    assert!(new_task_page.contains("Torvalds") && new_task_page.contains("Linus"));

    let edited = form(&[
        ("name", "Torvalds"),
        ("system_prompt", "new prompt"),
        ("adapter_command", ""),
        ("model", ""),
        ("harness_credential_id", ""),
    ]);
    let response = app.post(&format!("/personas/{id}"), &edited, &cookie).await;
    assert_eq!(header_str(&response, "hx-redirect"), "/personas");
    let updates = audit(&app, "persona.update").await;
    assert_eq!(updates[0]["before"]["system_prompt"], "old prompt");
    assert_eq!(updates[0]["after"]["system_prompt"], "new prompt");
}

#[tokio::test]
async fn persona_requires_name_and_prompt_and_known_credential() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let body = form(&[
        ("name", ""),
        ("system_prompt", ""),
        ("harness_credential_id", &uuid::Uuid::now_v7().to_string()),
    ]);
    let html = text(app.post("/personas", &body, &cookie).await).await;
    assert!(
        html.contains("Name is required.")
            && html.contains("System prompt is required.")
            && html.contains("Unknown credential reference.")
    );
}

#[tokio::test]
async fn unknown_resources_are_404() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    for path in [
        "/projects/NOPE",
        "/personas/00000000-0000-0000-0000-000000000000",
        "/credentials/00000000-0000-0000-0000-000000000000",
    ] {
        assert_eq!(
            app.get(path, &cookie).await.status(),
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
}

#[tokio::test]
async fn configuration_tables_scroll_inside_their_own_box_and_the_page_has_no_phone_menu_entry() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let created = app
        .post(
            "/projects",
            &project_form("DF", "https://forge.example.test/o/r.git"),
            &cookie,
        )
        .await;
    assert_eq!(header_str(&created, "hx-redirect"), "/projects");

    let html = text(app.get("/projects", &cookie).await).await;
    // The table sits in a box that scrolls horizontally, so the page itself does not.
    let box_at = html
        .find("<div class=\"overflow-x-auto\"><table")
        .expect("scroll box");
    assert!(box_at < html.find("</table>").unwrap());
    // The configuration page is the current section of the rail but not in the phone's bottom bar.
    let bar = &html[html.find("id=\"mobile-menu\"").unwrap()..];
    let bar = &bar[..bar.find("</nav>").unwrap()];
    assert!(!bar.contains("/projects") && bar.contains("/tasks"));
}
