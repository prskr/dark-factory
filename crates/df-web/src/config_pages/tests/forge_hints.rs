//! Tests of the forge icons and integration hints in the project forms and lists.

use super::{forge_fragment, project_list, project_row};
use crate::testkit::{TestApp, form, text};

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
            ("webhook_wake", "members"),
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
    let row = |key: &str| project_row(&html, key);
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
