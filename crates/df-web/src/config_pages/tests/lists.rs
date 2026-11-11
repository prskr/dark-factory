//! Tests of the configuration lists: tables, cards, counts and the phone layout.

use super::{
    assert_project_created, assert_saved, credential_form_body, project_form, project_with_tasks,
};
use crate::testkit::{TestApp, form, seed_provider, tag_around, text};

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
    assert_project_created(&created);

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
        ">DF</span><a href=\"/projects/DF\" class=\"font-medium hover:underline\">Dark Factory</a>",
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
    // The rail has the menu on every page; only the screens of the phone navigation add the
    // header copy (hidden from 768 px).
    assert_eq!(html.matches("Account menu of Peter").count(), 2);
    assert_eq!(
        text(app.get("/personas", &cookie).await)
            .await
            .matches("Account menu of Peter")
            .count(),
        1
    );
}

#[tokio::test]
async fn a_saved_entry_updates_the_count_in_the_header_of_the_answer() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let provider = seed_provider(&app.pool).await.to_string();
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
        assert_project_created(&response);
        let page = text(app.get("/projects", &cookie).await).await;
        assert_eq!(count(&page, "Projects"), n.to_string());
    }
    let persona = form(&[
        ("name", "Ada"),
        ("system_prompt", "x"),
        ("model", "claude-sonnet-5-5"),
        ("model_provider_id", &provider),
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
    // The seeded model provider owns a credential reference as well.
    assert_eq!(count(&text(response).await, "Credentials"), "2");
    // The page and the answer render the same header.
    let page = text(app.get("/projects", &cookie).await).await;
    assert_eq!(count(&page, "Projects"), "2");
}

#[tokio::test]
async fn projects_and_credentials_share_the_table_card_that_scrolls_inside_itself() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    assert_project_created(
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
async fn the_phone_project_list_follows_the_mockup() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    project_with_tasks(&app, &["queued", "waiting"]).await;
    let html = text(app.get("/projects", &cookie).await).await;
    let phone = &html[html.find("<ul class=\"divide-y").unwrap()..];
    let phone = &phone[..phone.find("</ul>").unwrap()];

    // Top line: key chip, name, "Tasks" link on the right.
    let key = phone.find(">DF</span>").unwrap();
    let name = phone.find(">Dark Factory</a>").unwrap();
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
async fn the_rail_menu_shows_the_version_of_the_instance() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let html = text(app.get("/tasks", &cookie).await).await;
    assert!(html.contains("Dark Factory 2026.11.0"), "{html}");
}
