//! The Egress tab of the project settings page: own rules, the instance rules below them, the
//! hints and "Make global".

use axum::http::StatusCode;
use df_db::EntryKind;

use super::rules::row_of;
use super::{add_deny_rule, add_host, add_project, add_task_host, app_with_egress, rule_id};
use crate::testkit::{
    Seed, TestApp, form, header_str, new_task, open_tab, seed, tab_labels, tag_around, text,
};

/// The Egress tab region of project `key`, which the tab loads when it is first shown.
async fn region(app: &TestApp, cookie: &str, key: &str) -> String {
    let html = text(
        app.get_htmx(&format!("/projects/{key}/egress"), cookie)
            .await,
    )
    .await;
    let at = html.find("id=\"project-egress\"").expect("the region");
    html[at..].to_string()
}

/// The part of the region above the section of the instance rules.
fn own_part(region: &str) -> &str {
    &region[..region
        .find("id=\"project-egress-instance\"")
        .unwrap_or(region.len())]
}

/// Projects `DF` and `OPS`; the seed of `DF` and a board member's cookie.
async fn two_projects(app: &TestApp) -> (Seed, String) {
    let seed = seed(&app.pool).await;
    add_project(app, "OPS").await;
    (seed, app.login().await)
}

/// A task of project `key` named `title`.
async fn task_of(app: &TestApp, seed: &Seed, key: &str, title: &str) -> df_db::TaskRow {
    let project = sqlx::query_scalar("SELECT id FROM projects WHERE key = $1")
        .bind(key)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    new_task(
        &app.pool,
        &Seed {
            project,
            persona: seed.persona,
            provider: seed.provider,
        },
        title,
    )
    .await
}

#[tokio::test]
async fn the_tab_sits_between_security_and_integrations_and_opens_from_the_url() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;

    let admin = text(
        app.get("/projects/DF?tab=egress", &app.login_admin().await)
            .await,
    )
    .await;
    let member = text(app.get("/projects/DF", &app.login().await).await).await;
    let unknown = text(app.get("/projects/DF?tab=nope", &app.login().await).await).await;

    assert_eq!(open_tab(&admin), "egress");
    // The rules load when the tab is first shown.
    let panel = &admin[admin.find("id=\"panel-egress\"").unwrap()..];
    let lazy = tag_around(panel, "hx-get=\"/projects/DF/egress\"");
    assert!(
        lazy.contains("hx-trigger=\"intersect once\"") && lazy.contains("hx-target=\"this\""),
        "{lazy}"
    );
    let plain = app.get("/projects/DF/egress", &app.login().await).await;
    assert_eq!(plain.status(), StatusCode::SEE_OTHER);
    let unknown_project = app
        .get_htmx("/projects/NOPE/egress", &app.login().await)
        .await;
    assert_eq!(unknown_project.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        tab_labels(&admin),
        [
            "General",
            "Security",
            "Egress",
            "Integrations",
            "MCP servers"
        ]
    );
    assert_eq!(tab_labels(&member), ["General", "Security", "Egress"]);
    assert_eq!(open_tab(&unknown), "general");
}

#[tokio::test]
async fn the_save_buttons_are_hidden_while_the_egress_tab_is_open() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;

    let page = text(app.get("/projects/DF?tab=egress", &app.login().await).await).await;

    let hidden = "group-has-[[data-nosave][aria-selected=true]]/page:hidden";
    assert!(
        tag_around(&page, "id=\"tab-egress\"").contains("data-nosave"),
        "{page}"
    );
    assert!(!tag_around(&page, "id=\"tab-security\"").contains("data-nosave"));
    // The button of the header and the one below the form on phones.
    assert_eq!(page.matches(hidden).count(), 2, "{page}");
    assert!(page.contains("group/page"), "{page}");
}

#[tokio::test]
async fn the_tab_lists_the_rules_of_the_project_and_its_tasks_only() {
    let app = app_with_egress(vec![]).await;
    let (seed, cookie) = two_projects(&app).await;
    let seed_task = task_of(&app, &seed, "DF", "Fix CI").await;
    add_host(&app, &cookie, "DF", "*.crates.io", "443").await;
    add_host(&app, &cookie, "OPS", "registry.npmjs.org", "443").await;
    add_host(&app, &cookie, "", "api.anthropic.com", "443").await;
    add_task_host(&app, seed_task.id.as_uuid(), "pypi.org").await;

    let html = region(&app, &cookie, "DF").await;
    let own = own_part(&html);

    assert!(
        own.contains(">*.crates.io<") && own.contains(">pypi.org<"),
        "{own}"
    );
    assert!(row_of(own, ">pypi.org<").contains(&format!(">Task {}<", seed_task.key)));
    assert!(row_of(own, ">*.crates.io<").contains(">Project DF<"));
    assert!(
        !own.contains("registry.npmjs.org") && !own.contains("api.anthropic.com"),
        "{own}"
    );
    assert!(
        own.contains("Project and task rules") && own.contains(">2<"),
        "{own}"
    );
}

#[tokio::test]
async fn a_project_without_rules_says_so_and_offers_both_actions_with_the_project_preselected() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;

    let html = region(&app, &cookie, "DF").await;

    assert!(html.contains("No rules for this project yet"), "{html}");
    let new = tag_around(&html, ">New egress rule<");
    assert!(
        new.contains("hx-get=\"/egress/allowlist/host/new?return=project:DF\""),
        "{new}"
    );
    assert!(
        html.contains("hx-get=\"/egress/allowlist/service/new?return=project:DF\"")
            && html.contains(">Add cluster service<"),
        "{html}"
    );
    let dialog = text(
        app.get_htmx("/egress/allowlist/host/new?return=project:DF", &cookie)
            .await,
    )
    .await;
    assert!(
        dialog.contains("<option value=\"DF\" selected>"),
        "{dialog}"
    );
    assert!(
        dialog.contains("<input type=\"hidden\" name=\"return\" value=\"project:DF\">"),
        "{dialog}"
    );
    let service = text(
        app.get_htmx("/egress/allowlist/service/new?return=project:DF", &cookie)
            .await,
    )
    .await;
    assert!(
        service.contains("<option value=\"DF\" selected>"),
        "{service}"
    );
}

#[tokio::test]
async fn saving_from_the_tab_answers_with_the_tab_region_and_a_moved_rule_disappears() {
    let app = app_with_egress(vec![]).await;
    let (_seed, cookie) = two_projects(&app).await;

    let created = app
        .post(
            "/egress/allowlist/host",
            &form(&[
                ("project", "DF"),
                ("host", "pypi.org"),
                ("port", "443"),
                ("return", "project:DF"),
            ]),
            &cookie,
        )
        .await;
    assert_eq!(created.status(), StatusCode::OK);
    assert_eq!(header_str(&created, "hx-retarget"), "#project-egress");
    assert_eq!(header_str(&created, "hx-trigger"), "modal-close");
    let html = text(created).await;
    assert!(
        html.contains("id=\"project-egress\"") && html.contains(">pypi.org<"),
        "{html}"
    );
    let id = rule_id(&app, "pypi.org").await;

    let moved = app
        .post(
            &format!("/egress/allowlist/{id}"),
            &form(&[
                ("project", "OPS"),
                ("host", "pypi.org"),
                ("port", "443"),
                ("return", "project:DF"),
            ]),
            &cookie,
        )
        .await;

    assert_eq!(header_str(&moved, "hx-retarget"), "#project-egress");
    let html = text(moved).await;
    assert!(
        html.contains("No rules for this project yet") && !own_part(&html).contains(">pypi.org<"),
        "{html}"
    );
}

#[tokio::test]
async fn deleting_from_the_tab_rerenders_the_tab_region() {
    let app = app_with_egress(vec![]).await;
    let (_seed, cookie) = two_projects(&app).await;
    add_host(&app, &cookie, "DF", "pypi.org", "443").await;
    let id = rule_id(&app, "pypi.org").await;
    let html = region(&app, &cookie, "DF").await;
    let delete = tag_around(&html, "aria-label=\"Delete egress rule pypi.org:443\"");
    assert!(
        delete.contains(&format!(
            "hx-delete=\"/egress/allowlist/{id}?return=project:DF\""
        )) && delete.contains("hx-target=\"#project-egress\""),
        "{delete}"
    );

    let response = app
        .delete(
            &format!("/egress/allowlist/{id}?return=project:DF"),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(
        html.contains("id=\"project-egress\"") && html.contains("No rules for this project yet"),
        "{html}"
    );
    assert!(df_db::list_entries(&app.pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn instance_rules_are_listed_read_only_in_a_collapsed_section_with_links_to_the_rules() {
    let app = app_with_egress(vec![]).await;
    let (_seed, cookie) = two_projects(&app).await;
    add_host(&app, &cookie, "", "api.anthropic.com", "443").await;
    add_deny_rule(&app, "*.datadoghq.com").await;
    add_host(&app, &cookie, "OPS", "registry.npmjs.org", "443").await;

    let html = region(&app, &cookie, "DF").await;

    let at = html
        .find("id=\"project-egress-instance\"")
        .expect("the section");
    let section = &html[at..];
    assert!(
        tag_around(&html, "id=\"project-egress-instance\"").contains("x-data=\"{ open: false }\"")
    );
    let toggle = tag_around(section, "aria-expanded=");
    assert!(toggle.contains("aria-expanded=\"false\""), "{toggle}");
    assert!(
        section.contains("Also applies (instance)") && section.contains(">2<"),
        "{section}"
    );
    assert!(tag_around(section, "x-show=\"open\"").contains("x-cloak"));
    let table = &section[section.find("x-show=\"open\"").expect("collapsed content")..];
    assert!(
        table.contains("<a href=\"/egress/allowlist?highlight=api.anthropic.com:443\"")
            && table.contains("<a href=\"/egress/allowlist?highlight=*.datadoghq.com:any\""),
        "{table}"
    );
    assert!(!table.contains("registry.npmjs.org"), "{table}");
    assert!(
        !table.contains("hx-delete")
            && !table.contains("Edit egress rule")
            && !table.contains("hx-get=\"/egress/allowlist/"),
        "{table}"
    );
    assert!(
        table.contains(">Deny<") && table.contains(">Allow<"),
        "{table}"
    );
}

#[tokio::test]
async fn the_same_rule_in_another_project_shows_a_hint_with_make_global_and_a_confirmation_in_the_row()
 {
    let app = app_with_egress(vec![]).await;
    let (_seed, cookie) = two_projects(&app).await;
    add_host(&app, &cookie, "DF", "registry.npmjs.org", "443").await;
    add_host(&app, &cookie, "OPS", "registry.npmjs.org", "443").await;

    let html = region(&app, &cookie, "DF").await;
    let row = row_of(own_part(&html), ">registry.npmjs.org<");

    assert!(row.contains("Also allowed for project OPS"), "{row}");
    let action = tag_around(row, ">Make global<");
    assert!(action.contains("underline"), "{action}");
    assert!(
        html.contains("Make global? The rules of DF and OPS are replaced by one instance rule; every project may then reach it."),
        "{html}"
    );
    let confirm = tag_around(&html, "hx-post=\"/egress/allowlist/make-global\"");
    assert!(
        confirm.contains("registry.npmjs.org")
            && confirm.contains("project:DF")
            && confirm.contains("hx-target=\"#project-egress\""),
        "{confirm}"
    );
    assert!(!html.contains("hx-confirm"), "{html}");
    // The post belongs to the empty form outside the project form, so htmx sends no project field.
    assert!(confirm.contains("form=\"egress-actions\""), "{confirm}");
    let page = text(app.get("/projects/DF?tab=egress", &cookie).await).await;
    let anchor = page
        .find("<form id=\"egress-actions\"></form>")
        .expect("the anchor");
    let project_form = page.find("<form id=\"form\"").unwrap();
    assert!(
        anchor > page[project_form..].find("</form>").unwrap() + project_form,
        "the anchor is outside the project form"
    );
    assert!(html.contains(">Cancel<"), "{html}");
}

#[tokio::test]
async fn a_wildcard_and_an_exact_host_are_not_the_same_rule_and_task_rules_do_not_count() {
    let app = app_with_egress(vec![]).await;
    let (seed, cookie) = two_projects(&app).await;
    let seed_task = task_of(&app, &seed, "OPS", "Ops task").await;
    add_host(&app, &cookie, "DF", "*.crates.io", "443").await;
    add_host(&app, &cookie, "OPS", "static.crates.io", "443").await;
    add_host(&app, &cookie, "DF", "pypi.org", "443").await;
    add_task_host(&app, seed_task.id.as_uuid(), "pypi.org").await;

    let df = region(&app, &cookie, "DF").await;
    let ops = region(&app, &cookie, "OPS").await;

    assert!(
        !df.contains("Also allowed for project") && !ops.contains("Also allowed for project"),
        "{df}"
    );
    assert!(
        !df.contains("Make global?") || !own_part(&df).contains("hx-post"),
        "{df}"
    );
}

#[tokio::test]
async fn a_rule_that_an_instance_rule_covers_shows_already_allowed_everywhere_with_remove() {
    let app = app_with_egress(vec![]).await;
    let (_seed, cookie) = two_projects(&app).await;
    add_host(&app, &cookie, "", "github.com", "443").await;
    add_host(&app, &cookie, "DF", "github.com", "443").await;
    add_host(&app, &cookie, "OPS", "github.com", "443").await;
    let project_rule = df_db::list_entries(&app.pool)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.project_key.as_deref() == Some("DF"))
        .unwrap();

    let html = region(&app, &cookie, "DF").await;
    let row = row_of(own_part(&html), ">github.com<");

    assert!(
        row.contains("Already allowed everywhere"),
        "the precedence: {row}"
    );
    assert!(
        !row.contains("Also allowed for project") && !row.contains("Make global"),
        "{row}"
    );
    let remove = tag_around(row, ">Remove<");
    assert!(
        remove.contains(&format!(
            "hx-delete=\"/egress/allowlist/{}?return=project:DF\"",
            project_rule.id
        )) && remove.contains("underline"),
        "{remove}"
    );

    let removed = app
        .delete(
            &format!("/egress/allowlist/{}?return=project:DF", project_rule.id),
            &cookie,
        )
        .await;

    assert_eq!(removed.status(), StatusCode::OK);
    let rest = df_db::list_entries(&app.pool).await.unwrap();
    assert_eq!(rest.len(), 2, "the instance rule and the rule of OPS stay");
    assert!(rest.iter().any(|e| e.project_key.is_none()));
}

#[tokio::test]
async fn make_global_with_an_invalid_port_answers_an_error_toast() {
    let app = app_with_egress(vec![]).await;
    let (_seed, cookie) = two_projects(&app).await;

    let response = app
        .post(
            "/egress/allowlist/make-global",
            &form(&[
                ("host", "registry.npmjs.org"),
                ("port", "not-a-port"),
                ("return", "project:DF"),
            ]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(header_str(&response, "hx-reswap"), "none");
    let body = text(response).await;
    crate::testkit::assert_toast(&body);
    assert!(
        body.contains("400 Bad request") && body.contains("The port is not valid."),
        "{body}"
    );
}

#[tokio::test]
async fn make_global_replaces_the_project_rules_by_one_instance_rule() {
    let app = app_with_egress(vec![]).await;
    let (_seed, cookie) = two_projects(&app).await;
    add_host(&app, &cookie, "DF", "registry.npmjs.org", "443").await;
    add_host(&app, &cookie, "OPS", "registry.npmjs.org", "443").await;
    let notifier = df_db::Notifier::start(&app.pool).await.unwrap();
    let mut notifications = notifier.subscribe();

    let response = app
        .post(
            "/egress/allowlist/make-global",
            &form(&[
                ("host", "registry.npmjs.org"),
                ("port", "443"),
                ("return", "project:DF"),
            ]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(
        html.contains("id=\"project-egress\"") && html.contains("No rules for this project yet"),
        "{html}"
    );
    assert!(
        html.contains("Also applies (instance)") && html.contains(">registry.npmjs.org<"),
        "{html}"
    );
    let entries = df_db::list_entries(&app.pool).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].project_key, None);
    assert_eq!(
        entries[0].kind,
        EntryKind::HttpHost {
            host: "registry.npmjs.org".into(),
            port: 443
        }
    );
    let published = super::notifications_before_marker(&app, &mut notifications).await;
    assert_eq!(
        published
            .iter()
            .filter(|n| **n == df_db::Notification::EgressPolicy)
            .count(),
        1,
        "{published:?}"
    );
}

#[tokio::test]
async fn make_global_with_an_instance_rule_changes_nothing_and_shows_the_tab_again() {
    let app = app_with_egress(vec![]).await;
    let (_seed, cookie) = two_projects(&app).await;
    add_host(&app, &cookie, "DF", "github.com", "443").await;
    add_host(&app, &cookie, "", "github.com", "443").await;

    let response = app
        .post(
            "/egress/allowlist/make-global",
            &form(&[
                ("host", "github.com"),
                ("port", "443"),
                ("return", "project:DF"),
            ]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let html = text(response).await;
    assert!(html.contains("Already allowed everywhere"), "{html}");
    assert_eq!(df_db::list_entries(&app.pool).await.unwrap().len(), 2);
}
