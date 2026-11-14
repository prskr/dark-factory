//! Tests of the configuration lists: tables, cards, counts and the phone layout.

use super::super::repo_display;
use super::{
    assert_project_created, assert_saved, credential_form_body, project_form, project_with_tasks,
};
use crate::testkit::{
    TestApp, form, head_of, phone_list_of, seed_provider, table_of, tag_around, text,
};

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
    let table = table_of(&html);
    // Headers: a hidden "Key", the project and the four counts; no repository, branch or actions.
    let head = head_of(table);
    let labels: Vec<&str> = head
        .split("<th ")
        .skip(1)
        .map(|th| {
            let inner = &th[th.find('>').unwrap() + 1..th.find("</th>").unwrap()];
            let inner = inner.trim_end_matches("</span>");
            inner.rsplit('>').next().unwrap_or(inner)
        })
        .collect();
    assert_eq!(
        labels,
        ["Key", "Project", "Queued", "Running", "Waiting", "Done"],
        "{head}"
    );
    assert!(
        head.contains("<span class=\"sr-only\">Key</span>"),
        "{head}"
    );
    // The row: key chip, the name link covering the row, then forge, URL, branch icon and branch.
    let name = tag_around(table, ">Dark Factory</a>");
    assert!(
        name.contains("href=\"/projects/DF\"") && name.contains("after:inset-0"),
        "{name}"
    );
    let chip = tag_around(table, ">DF</span>");
    assert!(chip.contains("group-hover:bg-panel"), "{chip}");
    let details = &table[table.find("data-forge=").unwrap()..];
    // The desktop row shows the repository without scheme and `.git`.
    let url = details
        .find(">git.example.org/peter/dark-factory<")
        .unwrap();
    assert!(!table.contains("https://git.example.org"), "{table}");
    // The generic forge icon is the same git glyph, so look for the branch icon after the URL.
    let branch = url + details[url..].find("M18 9a9 9 0 0 1-9 9").unwrap();
    assert!(details.find(">main<").unwrap() > branch, "{details}");
    assert!(!table.contains("Edit project DF"), "{table}");
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
    // A non-zero count stays a link above the row's cover.
    let running = tag_around(table, "href=\"/tasks?project=DF&amp;status=running\"");
    assert!(running.contains("relative z-10"), "{running}");
}

#[test]
fn the_displayed_repository_drops_http_scheme_and_git_suffix_only() {
    assert_eq!(
        repo_display("https://git.example.org/peter/dark-factory.git"),
        "git.example.org/peter/dark-factory"
    );
    assert_eq!(repo_display("http://git.local/o/r"), "git.local/o/r");
    assert_eq!(
        repo_display("ssh://git@git.example.org/o/r.git"),
        "ssh://git@git.example.org/o/r.git"
    );
    assert_eq!(
        repo_display("git@git.example.org:o/r.git"),
        "git@git.example.org:o/r.git"
    );
}

#[tokio::test]
async fn a_zero_count_in_the_desktop_table_is_only_announced() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    app.post(
        "/projects",
        &project_form("WEB", "https://g.example/o/r.git"),
        &cookie,
    )
    .await;
    let html = text(app.get("/projects", &cookie).await).await;
    let table = table_of(&html);
    assert!(
        table.contains("<span class=\"sr-only\">Running: 0 tasks of WEB</span>"),
        "{table}"
    );
    assert!(!table.contains("href=\"/tasks"), "{table}");
    // The phone card keeps its muted zero.
    let phone = phone_list_of(&html);
    let marker = "href=\"/tasks?project=WEB&amp;status=running\"";
    assert!(tag_around(phone, marker).contains("text-faint"), "{phone}");
    let link = &phone[phone.find(marker).unwrap()..];
    let link = &link[..link.find("</a>").unwrap()];
    assert!(
        link.contains("<span>0</span><span class=\"font-sans text-[13px]\">running</span>"),
        "{link}"
    );
}

#[tokio::test]
async fn the_credential_row_is_a_button_that_covers_it_with_secret_and_key_below() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let response = app
        .post(
            "/credentials",
            &credential_form_body("anthropic-api", "factory-anthropic", "api-key"),
            &cookie,
        )
        .await;
    assert_saved(&response);
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM credential_refs")
        .fetch_one(&app.pool)
        .await
        .unwrap();

    let html = text(app.get("/credentials", &cookie).await).await;

    let table = table_of(&html);
    let head = head_of(table);
    assert!(
        head.contains(">Name</th>") && !head.contains("Secret") && !head.contains("Actions"),
        "{head}"
    );
    let button = tag_around(table, ">anthropic-api</button>");
    for part in [
        "aria-label=\"Edit credential reference anthropic-api\"",
        &format!("hx-get=\"/credentials/{id}/edit\""),
        "after:inset-0",
    ] {
        assert!(button.contains(part), "{part}: {button}");
    }
    assert!(
        table.contains("<span class=\"font-mono\">factory-anthropic <span class=\"mx-1 text-faint\">/</span> api-key</span>"),
        "{table}"
    );
    // The credentials page says what the references are, below the title.
    assert!(
        html.contains("<p class=\"mt-1 text-sm text-muted\">References to Kubernetes Secrets. The values never reach the control plane database.</p>"),
        "{html}"
    );
    assert!(!table.contains("<svg"), "no pencil: {table}");
}

#[tokio::test]
async fn projects_are_a_compact_list_with_counts_on_phones_without_a_wide_table() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    project_with_tasks(&app, &["waiting"]).await;
    let html = text(app.get("/projects", &cookie).await).await;

    let phone = phone_list_of(&html);
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
            html.contains(
                "<tr class=\"group relative border-t border-line hover:bg-sunk\"><td class=\""
            ) && html.contains("w-full max-w-0"),
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
    let phone = phone_list_of(&html);

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
