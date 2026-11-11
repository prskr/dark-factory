//! Tests of the project settings page (`/projects/{key}`), its tabs and its groups.

use axum::http::StatusCode;

use super::{REPO, audit, project_form, stored_key};
use crate::testkit::{TestApp, header_str, open_tab, seed, tab_labels, tag_around, text};

#[tokio::test]
async fn the_page_shows_sidebar_back_link_header_tabs_and_save() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let response = app.get("/projects/DF", &app.login().await).await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(html.contains("<html"), "a full page: {html}");
    assert!(!html.contains("<dialog id=\"modal\""), "{html}");
    // The settings sidebar marks Projects; the rail marks Settings.
    let sidebar = &html[html.find("id=\"settings-menu\"").unwrap()..];
    let sidebar = &sidebar[..sidebar.find("</nav>").unwrap()];
    assert!(
        sidebar[..sidebar.find("aria-current").unwrap()].ends_with("<a href=\"/projects\" "),
        "{sidebar}"
    );
    // Back link, then the header with key, name and forge icon.
    let back = tag_around(&html, "class=\"mb-3 inline-flex min-h-11");
    assert!(back.contains("href=\"/projects\""), "{back}");
    let heading = &html[html.find("id=\"project-heading\"").unwrap()..];
    let heading = &heading[..heading.find("</div>").unwrap()];
    assert!(
        heading.contains(">DF<")
            && heading.contains("<h1")
            && heading.contains("Dark Factory")
            && heading.contains("data-forge=\"git\""),
        "{heading}"
    );
    assert_eq!(tab_labels(&html), ["General", "Security"]);
    assert_eq!(open_tab(&html), "general");
    assert!(html.contains(">Save project</button>"), "{html}");
    // On phones no bottom bar and no create button.
    assert!(!html.contains("id=\"mobile-menu\""), "{html}");
    assert!(!html.contains("aria-label=\"New project\""), "{html}");
}

#[tokio::test]
async fn the_page_starts_next_to_the_settings_sidebar_and_is_at_most_1280_px_wide() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let html = text(app.get("/projects/DF", &app.login().await).await).await;

    let frame = tag_around(&html, "id=\"project-settings\"");
    assert!(frame.contains("max-w-7xl"), "capped at 1280 px: {frame}");
    assert!(
        !frame.contains("mx-auto"),
        "left-aligned, not centered: {frame}"
    );
}

#[tokio::test]
async fn the_query_chooses_the_open_tab_and_unknown_or_forbidden_values_open_general() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let member = app.login().await;
    let admin = app.login_admin().await;
    let tab = |uri: &'static str, cookie: &String| {
        let app = &app;
        let cookie = cookie.clone();
        async move { open_tab(&text(app.get(uri, &cookie).await).await) }
    };

    assert_eq!(tab("/projects/DF?tab=security", &member).await, "security");
    // The former id of the tab is an unknown value.
    assert_eq!(
        tab("/projects/DF?tab=push-checks", &member).await,
        "general"
    );
    assert_eq!(
        tab("/projects/DF?tab=integrations", &admin).await,
        "integrations"
    );
    assert_eq!(
        tab("/projects/DF?tab=integrations", &member).await,
        "general"
    );
    assert_eq!(tab("/projects/DF?tab=nonsense", &admin).await, "general");
    assert_eq!(tab("/projects/DF", &admin).await, "general");
}

#[tokio::test]
async fn unknown_project_and_the_old_edit_route_answer_404() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    for uri in ["/projects/NOPE", "/projects/DF/edit"] {
        assert_eq!(
            app.get(uri, &cookie).await.status(),
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
}

#[tokio::test]
async fn the_project_list_links_name_and_edit_action_to_the_page() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let html = text(app.get("/projects", &app.login().await).await).await;

    let edit = tag_around(&html, "aria-label=\"Edit project DF\"");
    assert!(edit.starts_with("<a href=\"/projects/DF\""), "{edit}");
    assert!(!html.contains("/projects/DF/edit"), "{html}");
    let name = tag_around(&html, ">Dark Factory</a>");
    assert!(name.contains("href=\"/projects/DF\""), "{name}");
}

#[tokio::test]
async fn a_save_re_renders_the_form_and_says_project_saved_without_closing_a_modal() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let body = project_form("DF", REPO).replace("Dark+Factory", "Factory+Two");

    let response = app.post("/projects/DF", &body, &cookie).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-trigger"), "");
    assert_eq!(header_str(&response, "hx-retarget"), "");
    let html = text(response).await;
    assert!(html.starts_with("<form id=\"form\""), "{html}");
    assert!(html.contains("value=\"Factory Two\""), "{html}");
    // The status lands in the live region of the header; the heading follows the new name.
    let status = &html[html.find("hx-target=\"#save-status\"").unwrap()..];
    assert!(status.contains("Project saved"), "{status}");
    let heading = &html[html.find("hx-target=\"#project-heading\"").unwrap()..];
    assert!(heading.contains("Factory Two"), "{heading}");
    assert_eq!(audit(&app, "project.update").await.len(), 1);
}

#[tokio::test]
async fn the_page_has_a_polite_empty_status_region_for_the_save_message() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let html = text(app.get("/projects/DF", &app.login().await).await).await;

    let status = tag_around(&html, "id=\"save-status\"");
    assert!(
        status.contains("role=\"status\"") && status.contains("aria-live=\"polite\""),
        "{status}"
    );
    assert!(!html.contains("Project saved"), "{html}");
}

#[tokio::test]
async fn a_save_keeps_the_tab_the_user_was_on() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let body = format!("{}&tab=security", project_form("DF", REPO));

    let html = text(app.post("/projects/DF", &body, &cookie).await).await;

    assert_eq!(open_tab(&html), "security");
}

#[tokio::test]
async fn a_validation_error_opens_the_tab_of_the_first_error_and_keeps_the_values() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    // The user is on Security; the error is on General.
    let body = format!(
        "{}&tab=security",
        project_form("DF", "ssh://git.example.org/o/r.git")
    );

    let response = app.post("/projects/DF", &body, &cookie).await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert_eq!(open_tab(&html), "general");
    assert!(html.contains("Must be an HTTPS URL"), "{html}");
    assert!(
        html.contains("value=\"ssh://git.example.org/o/r.git\""),
        "{html}"
    );
    assert!(!html.contains("Project saved"), "{html}");
    assert_eq!(stored_key(&app).await, "DF");
}

#[tokio::test]
async fn creating_a_project_sends_the_browser_to_its_settings_page() {
    let app = TestApp::new().await;
    let response = app
        .post(
            "/projects",
            &project_form("WEB", "https://git.example.org/o/web.git"),
            &app.login().await,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-location"), "/projects/WEB");
    assert_eq!(header_str(&response, "hx-trigger"), "");
    assert_eq!(
        audit(&app, "project.create").await[0]["after"]["key"],
        "WEB"
    );
}

/// The markup of the group `title`: from its `<section` to the next group or the end of the panel.
fn group<'a>(html: &'a str, title: &str) -> &'a str {
    let marker = format!(
        "<section aria-labelledby=\"group-{}\"",
        title.to_lowercase().replace(' ', "-")
    );
    let start = html
        .find(&marker)
        .unwrap_or_else(|| panic!("{title}: {html}"));
    let rest = &html[start + 1..];
    let end = rest.find("<section").map_or(html.len(), |e| start + 1 + e);
    &html[start..end]
}

fn field_order(html: &str, names: &[&str]) {
    let at: Vec<usize> = names
        .iter()
        .map(|n| {
            html.find(&format!("for=\"{n}\""))
                .unwrap_or_else(|| panic!("{n}: {html}"))
        })
        .collect();
    assert!(at.is_sorted(), "{names:?}: {at:?}");
}

const GROUPS: [(&str, &[&str]); 4] = [
    ("Project", &["key", "name"]),
    (
        "Repository",
        &[
            "repo_url",
            "base_branch",
            "forge_credential_id",
            "issue_tracker_url",
        ],
    ),
    ("Sandbox", &["image", "adapter_command"]),
    ("Forge events", &["webhook_wake"]),
];

fn assert_groups(html: &str) {
    let titles: Vec<usize> = GROUPS
        .iter()
        .map(|(title, _)| {
            html.find(&format!(
                "id=\"group-{}\"",
                title.to_lowercase().replace(' ', "-")
            ))
            .unwrap()
        })
        .collect();
    assert!(titles.is_sorted(), "{titles:?}");
    for (title, fields) in GROUPS {
        let region = group(html, title);
        assert!(region.contains(&format!(">{title}</h3>")), "{region}");
        field_order(region, fields);
        // A group holds only its own fields.
        for (other, others) in GROUPS {
            if other != title {
                for field in others {
                    assert!(
                        !region.contains(&format!("for=\"{field}\"")),
                        "{title} has {field}"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn the_settings_page_and_the_new_project_form_show_the_approved_groups_in_order() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let cookie = app.login().await;

    assert_groups(&text(app.get("/projects/DF", &cookie).await).await);
    let new = text(app.get_htmx("/projects/new", &cookie).await).await;
    assert_groups(&new);
    // The new-project form is untabbed and its key is editable.
    assert!(!new.contains("role=\"tablist\""), "{new}");
    let key = tag_around(&new, "id=\"key\"");
    assert!(!key.contains("readonly"), "{key}");
    assert!(new.contains(">Create project</button>"), "{new}");
}

#[tokio::test]
async fn groups_are_separated_by_a_line_except_above_the_first() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let html = text(app.get("/projects/DF", &app.login().await).await).await;

    let sections = html.matches("<section aria-labelledby=\"group-").count();
    assert_eq!(sections, 4, "{html}");
    assert_eq!(html.matches("first:border-t-0").count(), 4, "{html}");
}

#[tokio::test]
async fn a_validation_error_renders_inside_its_group_with_the_values_kept() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    let response = app
        .post(
            "/projects/DF",
            &project_form("DF", "ssh://git.example.org/o/r.git"),
            &cookie,
        )
        .await;

    let html = text(response).await;
    let repository = group(&html, "Repository");
    assert!(repository.contains("Must be an HTTPS URL"), "{repository}");
    assert!(
        repository.contains("value=\"ssh://git.example.org/o/r.git\""),
        "{repository}"
    );
    assert!(group(&html, "Project").contains("value=\"Dark Factory\""));
    assert!(group(&html, "Sandbox").contains("value=\"ghcr.io/example/sandbox:1\""));
}

#[tokio::test]
async fn grouping_keeps_the_field_names() {
    let app = TestApp::new().await;
    seed(&app.pool).await;
    let html = text(app.get("/projects/DF", &app.login().await).await).await;

    for name in [
        "key",
        "name",
        "repo_url",
        "issue_tracker_url",
        "image",
        "base_branch",
        "adapter_command",
        "forge_credential_id",
        "webhook_wake",
    ] {
        assert!(html.contains(&format!("name=\"{name}\"")), "{name}");
    }
}
