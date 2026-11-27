//! Tests of the MCP servers page and the project tab "MCP servers" (`mcp-servers` spec).

use axum::http::StatusCode;
use df_core::mcp::McpCredentialScheme;
use df_db::catalog::{Actor, CredentialRefInput, save_credential_ref};
use df_db::mcp_servers::{McpServerInput, save_mcp_server};
use uuid::Uuid;

use crate::testkit::{TestApp, actor, form, header_str, seed, tag_around, text};

const PAGE: &str = "/mcp-servers";
const TAB: &str = "/projects/DF/mcp-servers";

async fn credential(app: &TestApp, name: &str) -> Uuid {
    let (principal, roles) = actor();
    save_credential_ref(
        &app.pool,
        Actor {
            principal: &principal,
            roles: &roles,
        },
        None,
        &CredentialRefInput {
            name: name.into(),
            secret_name: format!("secret-{name}"),
            secret_key: "key".into(),
        },
    )
    .await
    .unwrap()
    .id
    .as_uuid()
}

async fn register(app: &TestApp, name: &str, credential: Uuid, private: bool) -> Uuid {
    let (principal, roles) = actor();
    save_mcp_server(
        &app.pool,
        Actor {
            principal: &principal,
            roles: &roles,
        },
        None,
        &McpServerInput {
            name: name.into(),
            url: format!("https://{name}.example.com/mcp"),
            credential_ref_id: credential,
            scheme: McpCredentialScheme::Header {
                name: "CONTEXT7_API_KEY".into(),
                prefix: String::new(),
            },
            private_network: private,
        },
    )
    .await
    .unwrap()
    .id
}

fn new_server(name: &str, credential: Uuid, scheme: &str, extra: &[(&str, &str)]) -> String {
    let credential = credential.to_string();
    let mut pairs = vec![
        ("name", name),
        ("url", "https://mcp.context7.com/mcp"),
        ("credential_ref_id", credential.as_str()),
        ("scheme", scheme),
    ];
    pairs.extend_from_slice(extra);
    form(&pairs)
}

async fn audit_actions(app: &TestApp) -> Vec<String> {
    sqlx::query_scalar("SELECT action FROM audit_log WHERE action LIKE 'mcp_server.%' OR action LIKE 'project.mcp%' ORDER BY id")
        .fetch_all(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_board_member_without_admin_gets_403_for_the_page_the_form_and_the_project_tab() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    let id = Uuid::now_v7();

    let responses = [
        app.get(PAGE, &cookie).await,
        app.get_htmx(&format!("{PAGE}/new"), &cookie).await,
        app.get_htmx(&format!("{PAGE}/{id}"), &cookie).await,
        app.post(PAGE, &form(&[("name", "x")]), &cookie).await,
        app.delete(&format!("{PAGE}/{id}"), &cookie).await,
        app.get(TAB, &cookie).await,
        app.get_htmx(TAB, &cookie).await,
        app.post(TAB, &form(&[("server", &id.to_string())]), &cookie)
            .await,
    ];
    for response in responses {
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(
            text(response)
                .await
                .contains("Your account lacks the role `admin`.")
        );
    }
    assert!(audit_actions(&app).await.is_empty());
}

#[tokio::test]
async fn the_empty_page_offers_the_first_server() {
    let app = TestApp::new().await;
    let html = text(app.get(PAGE, &app.login_admin().await).await).await;

    assert!(html.contains("No MCP servers yet"), "{html}");
    assert!(html.contains("New MCP server"), "{html}");
    assert!(
        html.contains("Third-party MCP servers that agents reach through the gateway."),
        "{html}"
    );
}

#[tokio::test]
async fn the_sidebar_lists_mcp_servers_after_model_providers() {
    let app = TestApp::new().await;
    let html = text(app.get(PAGE, &app.login_admin().await).await).await;

    let sidebar = &html[html.find("id=\"settings-menu\"").unwrap()..];
    let providers = sidebar.find("Model providers").unwrap();
    let servers = sidebar.find("MCP servers").unwrap();
    let egress = sidebar.find("Egress rules").unwrap();
    assert!(providers < servers && servers < egress, "{sidebar}");
}

#[tokio::test]
async fn the_create_form_states_the_url_note_the_write_tools_warning_and_the_private_network_help()
{
    let app = TestApp::new().await;
    let form = text(
        app.get_htmx(&format!("{PAGE}/new"), &app.login_admin().await)
            .await,
    )
    .await;

    assert!(form.contains("MCP Streamable HTTP endpoint; path and query parameters are kept. The URL is not secret: it is stored and shown here. A token belongs into the credential reference, not into the URL."), "{form}");
    assert!(
        form.contains("Agents get every tool of this server, including tools that write"),
        "{form}"
    );
    assert!(form.contains("In tasks and chats, agents see every tool the server offers, with its unreviewed description. Writes bypass the Git proxy's push checks and the factory's approvals. The credential's scope is the effective permission: use a read-only token where possible, and never a forge token with write access."), "{form}");
    assert!(form.contains("Private network"), "{form}");
    assert!(form.contains("Create MCP server"), "{form}");
    for label in ["Bearer", "Header", "Query parameter"] {
        assert!(form.contains(&format!(">{label}</option>")), "{label}");
    }
}

#[tokio::test]
async fn an_admin_registers_a_server_and_the_list_shows_it_with_its_scheme_and_credential() {
    let app = TestApp::new().await;
    let key = credential(&app, "context7-key").await;
    let cookie = app.login_admin().await;

    let response = app
        .post(
            PAGE,
            &new_server(
                "context7",
                key,
                "header",
                &[("credential_header", "CONTEXT7_API_KEY")],
            ),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "HX-Trigger"), "modal-close");
    let list = text(response).await;
    assert!(list.contains("context7"), "{list}");
    assert!(list.contains("https://mcp.context7.com/mcp"), "{list}");
    assert!(list.contains("CONTEXT7_API_KEY header"), "{list}");
    assert!(
        list.contains("context7-key (secret-context7-key/key)"),
        "{list}"
    );
    assert!(list.contains("no project"), "{list}");
    assert!(
        !list.contains("Private network</span>"),
        "off by default: {list}"
    );
    assert_eq!(audit_actions(&app).await, ["mcp_server.create"]);
}

#[tokio::test]
async fn the_list_marks_private_network_servers_and_names_their_projects() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let key = credential(&app, "wiki-key").await;
    let wiki = register(&app, "wiki", key, true).await;
    register(&app, "context7", key, false).await;
    let cookie = app.login_admin().await;
    app.post(TAB, &form(&[("server", &wiki.to_string())]), &cookie)
        .await;

    let html = text(app.get(PAGE, &cookie).await).await;

    assert!(html.contains("Private network"), "{html}");
    let wiki_row = &html[html.find("Edit MCP server wiki").unwrap()..];
    assert!(
        wiki_row[..wiki_row.find("</tr>").unwrap()].contains(">DF<"),
        "{wiki_row}"
    );
    assert!(html.contains("no project"), "context7 has none: {html}");
}

#[tokio::test]
async fn invalid_input_is_refused_under_the_field_and_nothing_is_stored() {
    let app = TestApp::new().await;
    let key = credential(&app, "k").await;
    let cookie = app.login_admin().await;
    let name_error = "Use lowercase letters, digits and single hyphens: start with a letter, end with a letter or digit, at most 20 characters.";
    let cases: Vec<(String, &str)> = vec![
        (new_server("my--tools", key, "bearer", &[]), name_error),
        (new_server("tools-", key, "bearer", &[]), name_error),
        (
            form(&[
                ("name", "a"),
                ("url", "http://mcp.internal/mcp"),
                ("credential_ref_id", &key.to_string()),
                ("scheme", "bearer"),
            ]),
            "http URLs are only accepted while network.allowPlainHttp is on.",
        ),
        (
            form(&[
                ("name", "a"),
                ("url", "https://user:secret@mcp.example.com/mcp"),
                ("credential_ref_id", &key.to_string()),
                ("scheme", "bearer"),
            ]),
            "The URL must be an absolute https URL without user information or fragment.",
        ),
        (
            new_server(
                "a",
                key,
                "header",
                &[("credential_header", "Mcp-Session-Id")],
            ),
            "is set by the gateway",
        ),
        (
            form(&[
                ("name", "a"),
                ("url", "https://design.example.com/mcp?tok%65n=abc"),
                ("credential_ref_id", &key.to_string()),
                ("scheme", "query"),
                ("credential_query_param", "token"),
            ]),
            "The URL already contains the parameter",
        ),
        (
            new_server("a", Uuid::now_v7(), "bearer", &[]),
            "Choose a credential reference.",
        ),
    ];
    for (body, message) in cases {
        let response = app.post(PAGE, &body, &cookie).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}"
        );
        let html = text(response).await;
        assert!(html.contains(message), "{message} in {html}");
    }
    assert!(
        df_db::mcp_servers::list_mcp_servers(&app.pool)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn plain_http_is_accepted_when_the_instance_allows_it() {
    let app = TestApp::with_plain_http().await;
    let key = credential(&app, "k").await;
    let body = form(&[
        ("name", "wiki"),
        ("url", "http://mcp.internal/mcp"),
        ("credential_ref_id", &key.to_string()),
        ("scheme", "bearer"),
    ]);

    let response = app.post(PAGE, &body, &app.login_admin().await).await;

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_query_credential_is_stored_with_the_urls_own_parameters() {
    let app = TestApp::new().await;
    let key = credential(&app, "penpot-jwt").await;
    let body = form(&[
        ("name", "penpot"),
        ("url", "https://design.example.com/mcp/stream?file=abc"),
        ("credential_ref_id", &key.to_string()),
        ("scheme", "query"),
        ("credential_query_param", "token"),
    ]);

    app.post(PAGE, &body, &app.login_admin().await).await;

    let stored = df_db::mcp_servers::list_mcp_servers(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        stored[0].input.url,
        "https://design.example.com/mcp/stream?file=abc"
    );
    assert_eq!(
        stored[0].input.scheme,
        McpCredentialScheme::Query {
            parameter: "token".into()
        }
    );
}

#[tokio::test]
async fn the_edit_form_shows_the_name_read_only_and_the_scheme_preview_for_query_servers() {
    let app = TestApp::new().await;
    let key = credential(&app, "penpot-jwt").await;
    let cookie = app.login_admin().await;
    let body = form(&[
        ("name", "penpot"),
        ("url", "https://design.example.com/mcp/stream?file=abc"),
        ("credential_ref_id", &key.to_string()),
        ("scheme", "query"),
        ("credential_query_param", "token"),
    ]);
    app.post(PAGE, &body, &cookie).await;
    let id = df_db::mcp_servers::list_mcp_servers(&app.pool)
        .await
        .unwrap()[0]
        .id;

    let html = text(app.get_htmx(&format!("{PAGE}/{id}"), &cookie).await).await;

    assert!(html.contains("The name can't be changed: it is part of every tool name agents see (&lt;name&gt;__&lt;tool&gt;)."), "{html}");
    assert!(
        tag_around(&html, "name=\"name\"").contains("readonly"),
        "{html}"
    );
    assert!(
        html.contains("https://design.example.com/mcp/stream?file=abc&amp;token=&lt;value&gt;"),
        "{html}"
    );
    assert!(html.contains("The upstream and proxies in front of it may log full URLs, query included. Prefer Bearer or Header when the server supports them."), "{html}");
    assert!(html.contains("Save"), "{html}");
}

#[tokio::test]
async fn editing_keeps_the_name_and_increases_the_configuration_version() {
    let app = TestApp::new().await;
    let key = credential(&app, "k").await;
    let id = register(&app, "context7", key, false).await;
    let cookie = app.login_admin().await;
    // A posted name is ignored: the stored one stays.
    let body = new_server("renamed", key, "bearer", &[("private_network", "on")]);

    let response = app.post(&format!("{PAGE}/{id}"), &body, &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    let stored = df_db::mcp_servers::get_mcp_server(&app.pool, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.input.name, "context7");
    assert!(stored.input.private_network);
    assert_eq!(stored.config_version, 2);
    assert!(
        audit_actions(&app)
            .await
            .contains(&"mcp_server.update".to_owned())
    );
}

#[tokio::test]
async fn a_server_enabled_for_a_project_cannot_be_deleted_and_the_list_says_so() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let key = credential(&app, "k").await;
    let id = register(&app, "context7", key, false).await;
    let cookie = app.login_admin().await;
    app.post(TAB, &form(&[("server", &id.to_string())]), &cookie)
        .await;

    let refused = text(app.delete(&format!("{PAGE}/{id}"), &cookie).await).await;

    assert!(
        refused.contains("MCP server context7 is enabled for DF; disable it there first."),
        "{refused}"
    );
    assert!(refused.contains("role=\"alert\""), "{refused}");
    assert!(
        df_db::mcp_servers::get_mcp_server(&app.pool, id)
            .await
            .unwrap()
            .is_some()
    );

    app.post(TAB, "", &cookie).await;
    let gone = text(app.delete(&format!("{PAGE}/{id}"), &cookie).await).await;
    assert!(gone.contains("No MCP servers yet"), "{gone}");
    assert!(
        audit_actions(&app)
            .await
            .contains(&"mcp_server.delete".to_owned())
    );
}

#[tokio::test]
async fn a_credential_a_server_uses_cannot_be_deleted() {
    let app = TestApp::new().await;
    let key = credential(&app, "context7-key").await;
    register(&app, "context7", key, false).await;
    let (principal, roles) = actor();

    let refused = df_db::catalog::delete_credential_ref(
        &app.pool,
        Actor {
            principal: &principal,
            roles: &roles,
        },
        key,
    )
    .await
    .unwrap_err();

    assert_eq!(
        refused.to_string(),
        "conflict: credential `context7-key` is used by MCP server context7"
    );
}

#[tokio::test]
async fn forge_and_model_provider_credentials_are_refused_for_a_server_and_the_other_way_round() {
    let app = TestApp::new().await;
    let seeded = seed(&app.pool).await;
    let forge = credential(&app, "forge-bot").await;
    let (principal, roles) = actor();
    let project = df_db::catalog::get_project(&app.pool, seeded.project)
        .await
        .unwrap()
        .unwrap();
    df_db::catalog::save_project(
        &app.pool,
        Actor {
            principal: &principal,
            roles: &roles,
        },
        Some(project.id),
        &df_db::catalog::ProjectInput {
            forge_credential_id: Some(forge),
            ..df_db::catalog::ProjectInput {
                key: project.key.clone(),
                name: project.name.clone(),
                repo_url: project.repo_url.clone(),
                issue_tracker_url: None,
                image: project.image.clone(),
                base_branch: project.base_branch.clone(),
                adapter_command: vec![],
                forge_credential_id: None,
                webhook_wake: project.webhook_wake,
                branch_rewrite_policy: project.branch_rewrite_policy,
                agent_branch_globs: project.agent_branch_globs.clone(),
                subtask_merge: project.subtask_merge,
            }
        },
    )
    .await
    .unwrap();
    let cookie = app.login_admin().await;

    let response = app
        .post(PAGE, &new_server("forgejo", forge, "bearer", &[]), &cookie)
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        text(response)
            .await
            .contains("credential `forge-bot` is the forge credential of project DF")
    );

    // The credential of the seeded model provider.
    let provider = df_db::model_providers::list_model_providers(&app.pool)
        .await
        .unwrap()
        .remove(0);
    let response = app
        .post(
            PAGE,
            &new_server("llm", provider.input.credential_ref_id, "bearer", &[]),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        text(response)
            .await
            .contains("is the credential of model provider")
    );

    // A project cannot take the credential of a server.
    let free = credential(&app, "context7-key").await;
    register(&app, "context7", free, false).await;
    let body = form(&[
        ("key", "DF"),
        ("name", "Dark Factory"),
        ("repo_url", "https://git.example.org/peter/dark-factory.git"),
        ("image", "img:latest"),
        ("base_branch", "main"),
        ("forge_credential_id", &free.to_string()),
        ("webhook_wake", "members"),
    ]);
    let response = app.post("/projects/DF", &body, &cookie).await;
    assert!(
        text(response)
            .await
            .contains("credential `context7-key` is used by MCP server context7")
    );
}

// ---------------------------------------------------------------------------------------------
// Project tab
// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn an_admins_project_page_has_the_tab_after_integrations_and_a_board_members_does_not() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let html = text(app.get("/projects/DF", &app.login_admin().await).await).await;
    let order: Vec<usize> = ["id=\"tab-integrations\"", "id=\"tab-mcp-servers\""]
        .iter()
        .map(|l| html.find(l).expect(l))
        .collect();
    assert!(order.is_sorted(), "{html}");
    let panel = &html[html.find("id=\"panel-mcp-servers\"").unwrap()..];
    assert!(
        tag_around(panel, "hx-get=\"/projects/DF/mcp-servers\"").contains("intersect once"),
        "{panel}"
    );
    assert!(
        tag_around(&html, "data-tab=\"mcp-servers\"").contains("data-nosave"),
        "no project save on this tab"
    );
    assert!(
        html.contains("id=\"mcp-servers-form\""),
        "the form of the tab sits next to the project form"
    );

    let member = text(app.get("/projects/DF", &app.login().await).await).await;
    assert!(!member.contains("panel-mcp-servers"), "{member}");
    assert!(!member.contains("/projects/DF/mcp-servers"), "{member}");
}

#[tokio::test]
async fn the_tab_lists_every_server_unchecked_by_default_with_private_network_badges() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let key = credential(&app, "k").await;
    register(&app, "context7", key, false).await;
    register(&app, "wiki", key, true).await;

    let html = text(app.get_htmx(TAB, &app.login_admin().await).await).await;

    assert!(
        html.contains(
            "Runs of this project, chats included, get every tool of the servers checked here"
        ),
        "{html}"
    );
    assert_eq!(html.matches("type=\"checkbox\"").count(), 2, "{html}");
    assert!(
        !html.contains("form=\"mcp-servers-form\" checked"),
        "nothing is enabled by default: {html}"
    );
    assert!(html.contains("Private network"), "{html}");
    assert!(html.contains("https://wiki.example.com/mcp"), "{html}");
    assert!(html.contains("Save MCP servers"), "{html}");
}

#[tokio::test]
async fn the_tab_without_servers_points_to_the_servers_page() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let html = text(app.get_htmx(TAB, &app.login_admin().await).await).await;

    assert!(html.contains("No MCP servers registered"), "{html}");
    assert!(html.contains("href=\"/mcp-servers\""), "{html}");
    assert!(html.contains("Open MCP servers"), "{html}");
    assert!(!html.contains("Save MCP servers"), "{html}");
}

#[tokio::test]
async fn saving_the_tab_enables_and_disables_servers_and_audits_it() {
    let app = TestApp::new().await;
    let seeded = seed(&app.pool).await;
    let key = credential(&app, "k").await;
    let context7 = register(&app, "context7", key, false).await;
    let wiki = register(&app, "wiki", key, false).await;
    let cookie = app.login_admin().await;

    let saved = text(
        app.post(
            TAB,
            &form(&[
                ("server", &context7.to_string()),
                ("server", &wiki.to_string()),
            ]),
            &cookie,
        )
        .await,
    )
    .await;
    assert!(saved.contains("MCP servers saved"), "{saved}");
    assert_eq!(
        saved.matches("form=\"mcp-servers-form\" checked").count(),
        2,
        "{saved}"
    );
    assert_eq!(
        df_db::mcp_servers::project_mcp_server_ids(&app.pool, seeded.project)
            .await
            .unwrap()
            .len(),
        2
    );

    app.post(TAB, &form(&[("server", &wiki.to_string())]), &cookie)
        .await;
    assert_eq!(
        df_db::mcp_servers::project_mcp_server_ids(&app.pool, seeded.project)
            .await
            .unwrap(),
        [wiki]
    );
    let actions = audit_actions(&app).await;
    assert_eq!(
        actions
            .iter()
            .filter(|a| *a == "project.mcp_servers_update")
            .count(),
        2
    );

    let unknown = app
        .post(
            TAB,
            &form(&[("server", &Uuid::now_v7().to_string())]),
            &cookie,
        )
        .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_project_form_save_that_carries_mcp_fields_changes_no_enablement() {
    let app = TestApp::new().await;
    let seeded = seed(&app.pool).await;
    let key = credential(&app, "k").await;
    let context7 = register(&app, "context7", key, false).await;
    let wiki = register(&app, "wiki", key, false).await;
    let cookie = app.login_admin().await;
    app.post(TAB, &form(&[("server", &wiki.to_string())]), &cookie)
        .await;
    let body = form(&[
        ("key", "DF"),
        ("name", "Dark Factory"),
        ("repo_url", "https://git.example.org/peter/dark-factory.git"),
        ("image", "img:latest"),
        ("base_branch", "main"),
        ("webhook_wake", "members"),
        ("server", &context7.to_string()),
        ("mcp_servers", "none"),
    ]);

    let response = app.post("/projects/DF", &body, &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        df_db::mcp_servers::project_mcp_server_ids(&app.pool, seeded.project)
            .await
            .unwrap(),
        [wiki]
    );
}
