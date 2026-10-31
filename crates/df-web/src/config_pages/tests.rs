use axum::http::StatusCode;

use crate::testkit::{TestApp, form, header_str, new_task, seed, tag_around, text};

async fn audit(app: &TestApp, action: &str) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT details FROM audit_log WHERE action = $1 ORDER BY id")
        .bind(action)
        .fetch_all(&app.pool)
        .await
        .unwrap()
}

/// A successful save answers with the updated list for `#config-list` and closes the modal.
fn assert_saved(response: &axum::http::Response<axum::body::Body>) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(response, "hx-retarget"), "#config-list");
    assert_eq!(header_str(response, "hx-reswap"), "outerHTML");
    assert_eq!(header_str(response, "hx-trigger"), "modal-close");
}

fn project_form(key: &str, repo: &str) -> String {
    project_form_with_tracker(key, repo, "")
}

fn project_form_with_tracker(key: &str, repo: &str, tracker: &str) -> String {
    form(&[
        ("key", key),
        ("name", "Dark Factory"),
        ("repo_url", repo),
        ("issue_tracker_url", tracker),
        ("image", "ghcr.io/example/sandbox:1"),
        ("base_branch", "main"),
        ("adapter_command", ""),
        ("forge_credential_id", ""),
    ])
}

async fn stored_adapter_command(app: &TestApp) -> Vec<String> {
    sqlx::query_scalar("SELECT adapter_command FROM projects WHERE key = 'DF'")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn project_adapter_command_is_stored_shown_again_and_clearable() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let body = |command: &str| {
        form(&[
            ("key", "DF"),
            ("name", "Dark Factory"),
            ("repo_url", REPO),
            ("issue_tracker_url", ""),
            ("image", "ghcr.io/example/sandbox:1"),
            ("base_branch", "main"),
            ("adapter_command", command),
            ("forge_credential_id", ""),
        ])
    };

    let response = app
        .post(
            "/projects/DF",
            &body("/usr/local/bin/my-acp-adapter --flag"),
            &cookie,
        )
        .await;
    assert_saved(&response);
    assert_eq!(
        stored_adapter_command(&app).await,
        ["/usr/local/bin/my-acp-adapter", "--flag"]
    );
    let edit = text(app.get_htmx("/projects/DF/edit", &cookie).await).await;
    assert!(
        edit.contains("value=\"/usr/local/bin/my-acp-adapter --flag\""),
        "{edit}"
    );
    assert!(edit.contains("Adapter command (optional)"));
    assert!(edit.contains("Leave empty to use the instance"));

    let response = app.post("/projects/DF", &body(""), &cookie).await;
    assert_saved(&response);
    assert!(stored_adapter_command(&app).await.is_empty());
}

#[tokio::test]
async fn credential_form_has_no_value_field() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get_htmx("/credentials/new", &cookie).await).await;
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
    assert_saved(&response);

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
    assert_saved(&response);
    let list = text(app.get("/projects", &cookie).await).await;
    assert!(list.contains("DF") && list.contains("dark-factory.git"));
    assert_eq!(audit(&app, "project.create").await[0]["after"]["key"], "DF");
}

#[tokio::test]
async fn http_repository_is_accepted_when_the_instance_allows_plain_http() {
    let app = TestApp::with_plain_http().await;
    let cookie = app.login().await;
    let response = app
        .post(
            "/projects",
            &project_form("DF", "http://forgejo-http.forgejo.svc:3000/o/r.git"),
            &cookie,
        )
        .await;
    assert_saved(&response);
    // Other schemes stay refused.
    let response = app
        .post(
            "/projects",
            &project_form("XY", "ssh://git@git.example.org/o/r.git"),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn repository_hint_says_https_only_unless_the_instance_allows_plain_http() {
    let strict = TestApp::new().await;
    let cookie = strict.login().await;
    let html = text(strict.get_htmx("/projects/new", &cookie).await).await;
    assert!(html.contains("HTTPS only."), "{html}");

    let allowed = TestApp::with_plain_http().await;
    let cookie = allowed.login().await;
    let html = text(allowed.get_htmx("/projects/new", &cookie).await).await;
    assert!(!html.contains("HTTPS only."), "{html}");
}

#[tokio::test]
async fn http_issue_tracker_is_accepted_only_when_the_instance_allows_plain_http() {
    let allowed = TestApp::with_plain_http().await;
    let cookie = allowed.login().await;
    let response = allowed
        .post(
            "/projects",
            &project_form_with_tracker(
                "DF",
                "http://forgejo-http.forgejo.svc:3000/o/r.git",
                "http://forgejo-http.forgejo.svc:3000/o/r/issues",
            ),
            &cookie,
        )
        .await;
    assert_saved(&response);

    let strict = TestApp::new().await;
    let cookie = strict.login().await;
    let response = strict
        .post(
            "/projects",
            &project_form_with_tracker(
                "DF",
                "https://git.example.org/o/r.git",
                "http://git.example.org/o/r/issues",
            ),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(response).await.contains("Must be an HTTPS URL"));
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

const REPO: &str = "https://git.example.org/peter/dark-factory.git";

async fn stored_key(app: &TestApp) -> String {
    sqlx::query_scalar("SELECT key FROM projects")
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn the_edit_form_shows_the_key_read_only_with_and_without_tasks() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    for with_tasks in [false, true] {
        if with_tasks {
            new_task(&app.pool, &seed, "first").await;
        }
        let html = text(app.get_htmx("/projects/DF/edit", &cookie).await).await;
        let at = html.find("id=\"key\"").unwrap();
        let input = &html[html[..at].rfind("<input").unwrap()..];
        let input = &input[..input.find('>').unwrap()];
        assert!(
            input.contains(" readonly")
                && input.contains("bg-sunk")
                && input.contains("text-muted"),
            "{with_tasks}: {input}"
        );
        assert!(
            html.contains("The key cannot change after creation."),
            "{with_tasks}"
        );
    }
    // The new-project form still takes a key.
    let new = text(app.get_htmx("/projects/new", &cookie).await).await;
    let at = new.find("id=\"key\"").unwrap();
    let input = &new[new[..at].rfind("<input").unwrap()..];
    assert!(!input[..input.find('>').unwrap()].contains("readonly"));
}

#[tokio::test]
async fn changing_the_key_of_a_project_is_rejected_with_or_without_tasks() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    for with_tasks in [false, true] {
        if with_tasks {
            new_task(&app.pool, &seed, "first").await;
        }
        let rejected = app
            .post("/projects/DF", &project_form("OTHER", REPO), &cookie)
            .await;
        assert_eq!(
            rejected.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{with_tasks}"
        );
        let html = text(rejected).await;
        assert!(
            html.contains("The key cannot change after creation."),
            "{with_tasks}: {html}"
        );
        // The form shows the stored key again, read-only.
        assert!(html.contains("value=\"DF\""), "{html}");
        assert_eq!(stored_key(&app).await, "DF");
    }
    // Other fields of the same project can still be edited.
    let ok = app
        .post("/projects/DF", &project_form("DF", REPO), &cookie)
        .await;
    assert_saved(&ok);
    assert_eq!(audit(&app, "project.update").await.len(), 1);
}

#[tokio::test]
async fn persona_is_created_selectable_and_edits_are_audited() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let body = form(&[
        ("name", "Torvalds"),
        ("system_prompt", "old prompt"),
        ("model", "opus"),
        ("harness_credential_id", ""),
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
        ("model", ""),
        ("harness_credential_id", ""),
    ]);
    let response = app.post(&format!("/personas/{id}"), &edited, &cookie).await;
    assert_saved(&response);
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
        "/projects/NOPE/edit",
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
async fn configuration_tables_scroll_inside_their_own_box_and_projects_is_on_the_phone_bar() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let created = app
        .post(
            "/projects",
            &project_form("DF", "https://forge.example.test/o/r.git"),
            &cookie,
        )
        .await;
    assert_saved(&created);

    let html = text(app.get("/projects", &cookie).await).await;
    // The table sits in a box that scrolls horizontally, so the page itself does not.
    let box_at = html
        .find("<div data-table class=\"relative min-w-0 max-w-full overflow-x-auto")
        .expect("table card");
    assert!(box_at < html.find("</table>").unwrap());
    // Projects is in the phone's bottom bar, Personas and Credentials are not.
    let bar = &html[html.find("id=\"mobile-menu\"").unwrap()..];
    let bar = &bar[..bar.find("</nav>").unwrap()];
    assert!(bar.contains("/projects") && bar.contains("/tasks"));
    assert!(!bar.contains("/personas") && !bar.contains("/credentials"));
}

fn credential_form_body(name: &str, secret: &str, key: &str) -> String {
    form(&[("name", name), ("secret_name", secret), ("secret_key", key)])
}

/// A project of the testkit seed with tasks in the given statuses.
async fn project_with_tasks(app: &TestApp, statuses: &[&str]) {
    let seed = seed(&app.pool).await;
    for status in statuses {
        let task = new_task(&app.pool, &seed, "t").await;
        sqlx::query("UPDATE tasks SET status = $2 WHERE id = $1")
            .bind(task.id.as_uuid())
            .bind(status)
            .execute(&app.pool)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn project_rows_show_forge_key_name_repository_branch_and_counts_as_links() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    // One queued, two running, one rate-limited, one limit-reached and three failed tasks.
    project_with_tasks(
        &app,
        &[
            "queued",
            "running",
            "running",
            "rate_limited",
            "limit_reached",
            "failed",
            "failed",
            "failed",
        ],
    )
    .await;

    let html = text(app.get("/projects", &cookie).await).await;
    let table = &html[html.find("<table").unwrap()..html.find("</table>").unwrap()];
    for part in [
        ">DF</span><span class=\"font-medium\">Dark Factory</span>",
        "https://git.example.org/peter/dark-factory.git",
        "text-muted\">main</td>",
        "data-forge=\"git\"",
    ] {
        assert!(table.contains(part), "{part}: {table}");
    }
    let count = |status: &str| {
        let link = format!("href=\"/tasks?project=DF&amp;status={status}\"");
        let at = table.find(&link).unwrap_or_else(|| panic!("{link}"));
        let tag = &table[at..];
        tag[tag.find('>').unwrap() + 1..tag.find("</a>").unwrap()].to_string()
    };
    assert_eq!(
        [
            count("queued"),
            count("running"),
            count("waiting"),
            count("done")
        ],
        ["1", "3", "1", "3"]
    );
    // Zeros are muted: a project without tasks has no accent counts.
    let empty = TestApp::new().await;
    let cookie = empty.login().await;
    empty
        .post(
            "/projects",
            &project_form("WEB", "https://g.example/o/r.git"),
            &cookie,
        )
        .await;
    let html = text(empty.get("/projects", &cookie).await).await;
    assert!(
        html.contains("text-faint\">0</a>") && !html.contains("text-accent-text underline\">0"),
        "{html}"
    );
}

#[tokio::test]
async fn projects_are_a_compact_list_with_counts_on_phones_without_a_wide_table() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    project_with_tasks(&app, &["waiting"]).await;
    let html = text(app.get("/projects", &cookie).await).await;

    let phone = &html[html.find("<ul class=\"divide-y").unwrap()..];
    let phone = &phone[..phone.find("</ul>").unwrap()];
    assert!(html.contains("divide-y divide-line rounded-xl border border-line bg-panel md:hidden"));
    assert!(
        !phone.contains("<table")
            && !phone.contains("min-w-max")
            && !phone.contains("overflow-x-auto")
    );
    for label in ["queued", "running", "waiting", "done"] {
        assert!(phone.contains(&format!(">{label}</span>")), "{label}");
    }
    assert!(phone.contains("href=\"/tasks?project=DF&amp;status=waiting\""));
    // The wide table only exists from 768 px.
    let table_box = html.find("<div data-table").unwrap();
    assert!(html[..table_box].ends_with("<div class=\"hidden min-w-0 md:block\">"));
}

#[tokio::test]
async fn project_forge_follows_the_configured_integration_and_the_credential() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let secret = form(&[
        ("name", "forge"),
        ("secret_name", "forge"),
        ("secret_key", "token"),
    ]);
    app.post("/credentials", &secret, &cookie).await;
    let credential: uuid::Uuid = sqlx::query_scalar("SELECT id FROM credential_refs")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let with_credential = |key: &str, credential: &str| {
        form(&[
            ("key", key),
            ("name", key),
            ("repo_url", "https://git.example.org/o/r.git"),
            ("issue_tracker_url", ""),
            ("image", "img"),
            ("base_branch", "main"),
            ("forge_credential_id", credential),
        ])
    };
    app.post(
        "/projects",
        &with_credential("DF", &credential.to_string()),
        &cookie,
    )
    .await;
    app.post("/projects", &with_credential("WEB", ""), &cookie)
        .await;

    let html = text(app.get("/projects", &cookie).await).await;
    let row = |key: &str| {
        let at = html
            .find(&format!(">{key}</span><span class=\"font-medium\">"))
            .unwrap();
        let start = html[..at].rfind("<tr").unwrap();
        html[start..start + html[start..].find("</tr>").unwrap()].to_string()
    };
    assert!(
        row("DF").contains("data-forge=\"forgejo\"")
            && row("DF").contains("viewBox=\"0 0 212 212\""),
        "{}",
        row("DF")
    );
    assert!(row("WEB").contains("data-forge=\"git\"") && row("WEB").contains("aria-label=\"Git\""));

    // Without an integration every project is generic.
    sqlx::query("UPDATE projects SET key = key")
        .execute(&app.pool)
        .await
        .unwrap();
    let plain = TestApp::with_forges(df_core::routing::ForgeRoutes::default()).await;
    let cookie = plain.login().await;
    plain.post("/credentials", &secret, &cookie).await;
    let credential: uuid::Uuid = sqlx::query_scalar("SELECT id FROM credential_refs")
        .fetch_one(&plain.pool)
        .await
        .unwrap();
    plain
        .post(
            "/projects",
            &with_credential("DF", &credential.to_string()),
            &cookie,
        )
        .await;
    let html = text(plain.get("/projects", &cookie).await).await;
    assert!(html.contains("data-forge=\"git\"") && !html.contains("data-forge=\"forgejo\""));
}

/// The project list of an app with `forges` and the projects `(key, repo_url, credential)`.
async fn project_list(
    forges: df_core::routing::ForgeRoutes,
    projects: &[(&str, &str, bool)],
) -> String {
    let app = TestApp::with_forges(forges).await;
    let cookie = app.login().await;
    let secret = form(&[
        ("name", "forge"),
        ("secret_name", "forge"),
        ("secret_key", "token"),
    ]);
    app.post("/credentials", &secret, &cookie).await;
    let credential: uuid::Uuid = sqlx::query_scalar("SELECT id FROM credential_refs")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    for (key, repo, with_credential) in projects {
        let credential = if *with_credential {
            credential.to_string()
        } else {
            String::new()
        };
        let body = form(&[
            ("key", key),
            ("name", key),
            ("repo_url", repo),
            ("issue_tracker_url", ""),
            ("image", "img"),
            ("base_branch", "main"),
            ("forge_credential_id", &credential),
        ]);
        app.post("/projects", &body, &cookie).await;
    }
    text(app.get("/projects", &cookie).await).await
}

/// The table row of project `key`.
fn project_row(html: &str, key: &str) -> String {
    let at = html
        .find(&format!(">{key}</span><span class=\"font-medium\">"))
        .unwrap_or_else(|| panic!("{key}: {html}"));
    let start = html[..at].rfind("<tr").unwrap();
    html[start..start + html[start..].find("</tr>").unwrap()].to_string()
}

#[tokio::test]
async fn two_forges_side_by_side_show_their_own_icon_per_project() {
    let forges = crate::testkit::forge_routes(
        &[("github", &["github.com"]), ("forgejo", &[])],
        &["github", "forgejo"],
    );
    let html = project_list(
        forges,
        &[
            ("DF", "https://git.example.org/peter/dark-factory.git", true),
            ("WEB", "https://github.com/acme/web.git", true),
            ("OPS", "https://github.com/acme/ops.git", false),
        ],
    )
    .await;
    assert!(project_row(&html, "DF").contains("data-forge=\"forgejo\""));
    assert!(project_row(&html, "WEB").contains("data-forge=\"github\""));
    // Without a forge credential the project is generic, whatever the route.
    assert!(project_row(&html, "OPS").contains("data-forge=\"git\""));
}

#[tokio::test]
async fn a_project_no_loaded_plugin_serves_shows_the_generic_git_icon() {
    let forges = crate::testkit::forge_routes(
        &[
            ("github", &["github.com"]),
            ("forgejo", &["git.example.org"]),
        ],
        &["forgejo"],
    );
    let html = project_list(
        forges,
        &[
            ("WEB", "https://github.com/acme/web.git", true),
            ("OPS", "https://code.example.com/acme/ops.git", true),
        ],
    )
    .await;
    for key in ["WEB", "OPS"] {
        let row = project_row(&html, key);
        assert!(row.contains("data-forge=\"git\""), "{key}: {row}");
    }
}

#[tokio::test]
async fn the_form_hint_names_the_integration_routed_for_the_repository_host() {
    let forges = crate::testkit::forge_routes(
        &[
            ("github", &["github.com"]),
            ("forgejo", &["git.example.org"]),
        ],
        &["github"],
    );
    let app = TestApp::with_forges(forges).await;
    let cookie = app.login().await;

    let claimed = forge_fragment(&app, &cookie, "https://github.com/acme/web.git", "").await;
    assert!(claimed.contains("data-forge=\"github\""), "{claimed}");
    assert!(
        claimed.contains("Served by the GitHub integration"),
        "{claimed}"
    );

    let not_loaded = forge_fragment(&app, &cookie, "https://git.example.org/o/r.git", "").await;
    assert!(
        not_loaded.contains("data-forge=\"git\"") && !not_loaded.contains("data-forge=\"forgejo\""),
        "{not_loaded}"
    );
    assert!(
        not_loaded.contains("The Forgejo integration claims this host but is not loaded"),
        "{not_loaded}"
    );

    let unclaimed = forge_fragment(&app, &cookie, "https://code.example.com/o/r.git", "").await;
    assert!(unclaimed.contains("data-forge=\"git\""), "{unclaimed}");
    assert!(
        unclaimed.contains("No forge integration applies"),
        "{unclaimed}"
    );
}

#[tokio::test]
async fn the_form_hint_names_the_default_integration_that_did_not_load() {
    let forges = crate::testkit::forge_routes(
        &[("github", &["github.com"]), ("forgejo", &[])],
        &["github"],
    );
    let app = TestApp::with_forges(forges).await;
    let cookie = app.login().await;

    let fragment = forge_fragment(&app, &cookie, "https://code.example.com/o/r.git", "").await;
    assert!(fragment.contains("data-forge=\"git\""), "{fragment}");
    assert!(
        fragment.contains("The default Forgejo integration is not loaded"),
        "{fragment}"
    );
}

async fn forge_fragment(app: &TestApp, cookie: &str, repo: &str, issue: &str) -> String {
    let uri = format!(
        "/projects/forge?{}",
        form(&[("repo_url", repo), ("issue_tracker_url", issue)])
    );
    text(app.get_htmx(&uri, cookie).await).await
}

#[tokio::test]
async fn typing_a_repository_url_shows_the_integration_that_serves_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = forge_fragment(
        &app,
        &cookie,
        "https://git.example.com/factory/dark-factory.git",
        "",
    )
    .await;

    let partial = |target: &str| {
        let at = html
            .find(&format!("hx-target=\"{target}\""))
            .unwrap_or_else(|| panic!("{target}: {html}"));
        let rest = &html[at..];
        rest[..rest.find("</template>").unwrap()].to_string()
    };
    assert!(partial("#repo-forge").contains("data-forge=\"forgejo\""));
    assert!(
        partial("#forge-hint").contains("Forgejo integration"),
        "{html}"
    );
    // An empty issue tracker shows no icon.
    assert!(!partial("#issue-forge").contains("data-forge"));
    // On the repository's host the issue tracker uses the forge; elsewhere it is a plain tracker.
    let same = forge_fragment(
        &app,
        &cookie,
        "https://git.example.com/o/r.git",
        "https://git.example.com/o/r/issues",
    )
    .await;
    assert!(same[same.find("#issue-forge").unwrap()..].contains("data-forge=\"forgejo\""));
    let other = forge_fragment(
        &app,
        &cookie,
        "https://git.example.com/o/r.git",
        "https://tracker.example.org/x",
    )
    .await;
    assert!(other[other.find("#issue-forge").unwrap()..].contains("data-forge=\"issue\""));
    // htmx sends the field it fires from twice: the first value counts.
    let uri = "/projects/forge?repo_url=https%3A%2F%2Fg.example%2Fo%2Fr.git&repo_url=x&issue_tracker_url=";
    assert!(
        text(app.get_htmx(uri, &cookie).await)
            .await
            .contains("data-forge=\"forgejo\"")
    );
}

#[tokio::test]
async fn not_an_https_repository_url_maps_to_no_integration() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    for repo in [
        "ssh://git@git.example.org/o/r.git",
        "http://git.example.org/o/r.git",
        "https://git.example.org/only-owner",
        "not a url",
        "",
    ] {
        let html = forge_fragment(&app, &cookie, repo, "").await;
        assert!(
            html.contains("data-forge=\"git\"") && !html.contains("data-forge=\"forgejo\""),
            "{repo}: {html}"
        );
        assert!(html.contains("No forge integration applies"), "{repo}");
    }
    // An instance without integration says so for valid URLs, too.
    let plain = TestApp::with_forges(df_core::routing::ForgeRoutes::default()).await;
    let cookie = plain.login().await;
    let html = forge_fragment(&plain, &cookie, "https://git.example.org/o/r.git", "").await;
    assert!(html.contains("data-forge=\"git\"") && html.contains("No forge integration applies"));
}

#[tokio::test]
async fn the_project_form_has_forge_icons_inside_the_url_fields_and_a_live_hint() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get_htmx("/projects/new", &cookie).await).await;

    for part in [
        "id=\"repo-forge\"",
        "id=\"issue-forge\"",
        "hx-get=\"/projects/forge\"",
        "hx-trigger=\"keyup changed delay:300ms\"",
        "id=\"forge-hint\" aria-live=\"polite\"",
        "No forge integration applies",
    ] {
        assert!(html.contains(part), "{part}: {html}");
    }
    // The icon sits inside the field (same positioned box as the input).
    let repo = &html[html.find("id=\"repo-forge\"").unwrap()..];
    assert!(repo.find("<input id=\"repo_url\"").unwrap() < repo.find("</div>").unwrap());
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
    app.post(
        "/projects",
        &project_form("DF", "https://git.example.org/o/r.git"),
        &cookie,
    )
    .await;
    for path in [
        "/projects/DF",
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
async fn a_saved_project_updates_the_list_and_closes_the_modal() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let response = app
        .post(
            "/projects",
            &project_form("DF", "https://git.example.org/o/r.git"),
            &cookie,
        )
        .await;

    assert_saved(&response);
    let html = text(response).await;
    assert!(
        html.starts_with("<div id=\"config-list\">") && html.contains("Dark Factory"),
        "{html}"
    );
    // The list page and the answer render the same region.
    let page = text(app.get("/projects", &cookie).await).await;
    assert!(page.contains(&html));
}

#[tokio::test]
async fn personas_are_cards_with_harness_model_prompt_excerpt_and_credential() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    app.post(
        "/credentials",
        &credential_form_body("anthropic-api", "claude", "key"),
        &cookie,
    )
    .await;
    let credential: uuid::Uuid = sqlx::query_scalar("SELECT id FROM credential_refs")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let linus = form(&[
        ("name", "Linus"),
        (
            "system_prompt",
            "Be Linus.\nReview carefully.\nShip small changes.\nNever skip tests.",
        ),
        ("model", "opus"),
        ("harness_credential_id", &credential.to_string()),
    ]);
    app.post("/personas", &linus, &cookie).await;
    let plain = form(&[
        ("name", "Ada"),
        ("system_prompt", "Be Ada."),
        ("model", ""),
        ("harness_credential_id", ""),
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
    assert!(linus.contains("Claude Code · opus"), "{linus}");
    assert!(linus.contains("line-clamp-3") && linus.contains("Be Linus."));
    assert!(linus.contains(">anthropic-api<"));
    assert!(linus.contains("aria-label=\"Edit persona Linus\"") && linus.contains("/edit\""));
    let ada = card("Ada");
    assert!(
        ada.contains("Claude Code · default") && ada.contains(">none<"),
        "{ada}"
    );
}

#[tokio::test]
async fn empty_lists_explain_themselves_and_offer_the_create_action() {
    let app = TestApp::new().await;
    let cookie = app.login().await;

    let projects = text(app.get("/projects", &cookie).await).await;
    assert!(
        projects.contains("No projects yet") && projects.contains("A project ties a repository"),
        "{projects}"
    );
    let region = &projects[projects.find("id=\"config-list\"").unwrap()..];
    assert!(
        region.contains("hx-get=\"/projects/new\"") && region.contains(">New project</button>")
    );
    assert!(region.contains("href=\"/credentials\"") && region.contains("credential reference"));
    assert!(!region.contains("<table"));

    let personas = text(app.get("/personas", &cookie).await).await;
    assert!(personas.contains("No personas yet") && personas.contains("hx-get=\"/personas/new\""));
    let credentials = text(app.get("/credentials", &cookie).await).await;
    assert!(
        credentials.contains("No credential references yet")
            && credentials.contains("hx-get=\"/credentials/new\"")
    );
}

#[tokio::test]
async fn the_projects_page_has_the_profile_menu_for_phones() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get("/projects", &cookie).await).await;
    assert!(
        html.contains("aria-label=\"Account menu of Peter\"")
            && html.contains("hx-post=\"/logout\""),
        "{html}"
    );
    // Only the screens of the phone navigation have it.
    assert!(
        !text(app.get("/personas", &cookie).await)
            .await
            .contains("Account menu")
    );
}

#[tokio::test]
async fn a_saved_entry_updates_the_count_in_the_header_of_the_answer() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let count = |html: &str, title: &str| {
        let marker = format!("{title} <span class=\"font-normal text-faint\">");
        let head = html
            .find(&marker)
            .unwrap_or_else(|| panic!("no count: {html}"));
        let rest = &html[head + marker.len()..];
        rest[..rest.find('<').unwrap()].to_string()
    };
    for n in 1..=2 {
        let response = app
            .post(
                "/projects",
                &project_form(&format!("P{n}"), "https://g.example/o/r.git"),
                &cookie,
            )
            .await;
        assert_saved(&response);
        assert_eq!(count(&text(response).await, "Projects"), n.to_string());
    }
    let persona = form(&[
        ("name", "Ada"),
        ("system_prompt", "x"),
        ("model", ""),
        ("harness_credential_id", ""),
    ]);
    let response = app.post("/personas", &persona, &cookie).await;
    assert_eq!(count(&text(response).await, "Personas"), "1");
    let response = app
        .post(
            "/credentials",
            &credential_form_body("c", "s", "k"),
            &cookie,
        )
        .await;
    assert_eq!(count(&text(response).await, "Credentials"), "1");
    // The page and the answer render the same header.
    let page = text(app.get("/projects", &cookie).await).await;
    assert_eq!(count(&page, "Projects"), "2");
}

#[tokio::test]
async fn projects_and_credentials_share_the_table_card_that_scrolls_inside_itself() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    assert_saved(
        &app.post(
            "/projects",
            &project_form("DF", "https://g.example/o/r.git"),
            &cookie,
        )
        .await,
    );
    assert_saved(
        &app.post(
            "/credentials",
            &credential_form_body("c", "s", "k"),
            &cookie,
        )
        .await,
    );
    for uri in ["/projects", "/credentials"] {
        let html = text(app.get(uri, &cookie).await).await;
        assert_eq!(html.matches("data-table").count(), 1, "{uri}");
        let card = &html[html.find("data-table").unwrap()..];
        let card = &card[..card.find('>').unwrap()];
        for class in [
            "relative",
            "min-w-0",
            "max-w-full",
            "overflow-x-auto",
            "rounded-xl",
            "border-line",
            "bg-panel",
        ] {
            assert!(card.contains(class), "{uri}: {class} missing in {card}");
        }
        // Same header row and the same row borders.
        assert!(
            html.contains("<tr class=\"border-t border-line\"><td class=\"px-5 py-3.5"),
            "{uri}"
        );
        assert!(
            html.contains("text-xs font-medium uppercase tracking-wide text-faint"),
            "{uri}"
        );
    }
    // The content area itself never grows with the table.
    let page = text(app.get("/projects", &cookie).await).await;
    assert!(
        page.contains(
            "<main class=\"min-w-0 flex-1 w-full px-4 py-4 pb-38 md:px-10 md:py-8 md:pb-8\">"
        ),
        "main can shrink: {page}"
    );
}

#[tokio::test]
async fn the_project_form_groups_key_and_name_and_branch_and_credential_in_rows() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get_htmx("/projects/new", &cookie).await).await;

    assert!(
        html.contains("grid grid-cols-1 gap-4 md:grid-cols-2"),
        "two columns from 768 px, one on phones"
    );
    let spans_two = |name: &str| {
        html.contains(&format!(
            "<div class=\"md:col-span-2\"><div class=\"space-y-1\"><label for=\"{name}\""
        ))
    };
    for full in ["repo_url", "issue_tracker_url", "image"] {
        assert!(spans_two(full), "{full} is full width");
    }
    for half in ["key", "name", "base_branch", "forge_credential_id"] {
        assert!(!spans_two(half), "{half} shares a row");
        assert!(
            html.contains(&format!("<div class=\"space-y-1\"><label for=\"{half}\"")),
            "{half}"
        );
    }
    // Rows in the order of the mockup.
    let order = [
        "key",
        "name",
        "repo_url",
        "issue_tracker_url",
        "image",
        "base_branch",
        "forge_credential_id",
    ];
    let at: Vec<usize> = order
        .iter()
        .map(|n| html.find(&format!("for=\"{n}\"")).unwrap())
        .collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{at:?}");
}

#[tokio::test]
async fn the_persona_form_pairs_harness_and_model() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get_htmx("/personas/new", &cookie).await).await;
    assert!(html.contains("md:grid-cols-2"));
    for full in ["name", "system_prompt", "harness_credential_id"] {
        assert!(
            html.contains(&format!(
                "<div class=\"md:col-span-2\"><div class=\"space-y-1\"><label for=\"{full}\""
            )),
            "{full}"
        );
    }
    for half in ["harness", "model"] {
        assert!(!html.contains(&format!("<div class=\"md:col-span-2\"><div class=\"space-y-1\"><label class=\"block text-sm font-medium text-muted\" for=\"{half}\"")), "{half}");
        assert!(html.contains(&format!("for=\"{half}\"")));
    }
    assert!(html.find("for=\"harness\"").unwrap() < html.find("for=\"model\"").unwrap());
    assert!(
        !html.contains("adapter_command"),
        "persona form has no adapter command"
    );
}

#[tokio::test]
async fn forms_end_with_cancel_and_a_named_primary_action_at_the_bottom_right() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let persona = form(&[
        ("name", "Ada"),
        ("system_prompt", "x"),
        ("model", ""),
        ("harness_credential_id", ""),
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
        ("/projects/DF/edit".to_string(), "Save project"),
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

#[tokio::test]
async fn the_phone_project_list_follows_the_mockup() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    project_with_tasks(&app, &["queued", "waiting"]).await;
    let html = text(app.get("/projects", &cookie).await).await;
    let phone = &html[html.find("<ul class=\"divide-y").unwrap()..];
    let phone = &phone[..phone.find("</ul>").unwrap()];

    // Top line: key chip, name, "Tasks" link on the right.
    let key = phone.find(">DF</span>").unwrap();
    let name = phone.find(">Dark Factory</span>").unwrap();
    let tasks = phone.find("aria-label=\"Tasks of Dark Factory\"").unwrap();
    assert!(key < name && name < tasks, "{phone}");
    assert!(phone.contains("href=\"/tasks?status=all&amp;project=DF\""));
    // The forge icon sits next to the repository URL, after the first line, not before the key.
    let url = phone
        .find("https://git.example.org/peter/dark-factory.git")
        .unwrap();
    let icon = phone.find("data-forge=").unwrap();
    assert!(tasks < icon && icon < url, "{phone}");
    let url_tag = tag_around(phone, "https://git.example.org");
    assert!(url_tag.contains("truncate"), "{url_tag}");
    // Counts read "<dot> <number> <state>" with lower-case states, as links.
    let queued = phone
        .find("href=\"/tasks?project=DF&amp;status=queued\"")
        .unwrap();
    let count = &phone[queued..];
    let count = &count[..count.find("</a>").unwrap()];
    assert!(
        count.find("rounded-full").unwrap() < count.find(">1<").unwrap(),
        "{count}"
    );
    assert!(
        count.find(">1<").unwrap() < count.find(">queued<").unwrap(),
        "{count}"
    );
    // The edit button is its own column at the right of the row.
    let edit = phone.find("aria-label=\"Edit project DF\"").unwrap();
    assert!(phone.find("status=done").unwrap() < edit, "{phone}");
    let edit_tag = tag_around(phone, "aria-label=\"Edit project DF\"");
    assert!(
        edit_tag.contains("w-12") && edit_tag.contains("self-stretch"),
        "{edit_tag}"
    );
}

#[tokio::test]
async fn a_changed_key_and_another_error_are_both_shown_and_the_key_error_is_linked() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let response = app
        .post(
            "/projects/DF",
            &project_form("OTHER", "http://insecure.example/o/r.git"),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("The key cannot change after creation.")
            && html.contains("Must be an HTTPS URL"),
        "{html}"
    );
    // The stored key is shown, read-only, with the error linked to the field.
    let input = tag_around(&html, "id=\"key\"");
    assert!(
        input.contains("value=\"DF\"") && input.contains(" readonly"),
        "{input}"
    );
    assert!(
        input.contains("aria-invalid=\"true\"")
            && input.contains("aria-describedby=\"key-hint key-error\""),
        "{input}"
    );
    assert!(html.contains("id=\"key-error\""));
    assert_eq!(stored_key(&app).await, "DF");
}

#[tokio::test]
async fn a_tracker_on_the_host_of_an_unserved_repository_shows_the_issue_icon() {
    let app = TestApp::with_forges(df_core::routing::ForgeRoutes::default()).await;
    let cookie = app.login().await;
    let html = forge_fragment(
        &app,
        &cookie,
        "https://git.example.com/o/r.git",
        "https://git.example.com/o/r/issues",
    )
    .await;
    assert!(html[html.find("#issue-forge").unwrap()..].contains("data-forge=\"issue\""));
}
