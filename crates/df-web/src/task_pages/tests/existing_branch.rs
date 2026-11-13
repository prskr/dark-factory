//! The composer's "Existing branch" option (`task-ui`, "New task composer"; `task-branches`).

use axum::http::StatusCode;
use df_core::{ExistingBranch, PullRequest, PullRequestRef, PullRequestState, RepoRef};

use crate::testkit::{Seed, TestApp, form, header_str, seed, tag_around, text};

fn branch_form(seed: &Seed, description: &str, branch: &str) -> String {
    form(&[
        ("description", description),
        ("project", &seed.project.to_string()),
        ("persona", &seed.persona.to_string()),
        ("issue_number", "7"),
        ("branch", branch),
    ])
}

fn open_pr(number: u64, head: &str, base: &str) -> PullRequest {
    PullRequest {
        id: PullRequestRef {
            repo: RepoRef {
                owner: "peter".into(),
                name: "dark-factory".into(),
            },
            number,
        },
        url: format!("https://git.example.org/peter/dark-factory/pulls/{number}"),
        head: head.into(),
        base: base.into(),
        title: "Update serde".into(),
        body: String::new(),
        draft: false,
        state: PullRequestState::Open,
        head_sha: "abc".into(),
    }
}

/// A task of the seed's project that holds `branch`.
async fn hold_branch(app: &TestApp, seed: &Seed, branch: &str) {
    df_db::create_task_on(
        &app.pool,
        df_db::NewTask {
            project_id: df_core::ProjectId::from_uuid(seed.project),
            persona_id: df_core::PersonaId::from_uuid(seed.persona),
            title: Some("holder".into()),
            description: String::new(),
            issue_number: None,
            created_by: df_core::Principal::system("test"),
            parent_id: None,
            depends_on: Vec::new(),
        },
        df_db::TaskPlacement {
            branch: Some(ExistingBranch::parse(branch, "main").unwrap()),
            attached_pr: None,
        },
    )
    .await
    .unwrap();
}

async fn task_branch(app: &TestApp, key: &str) -> (String, Option<i64>, bool) {
    sqlx::query_as("SELECT branch, pr_number, pr_attached FROM tasks WHERE key = $1")
        .bind(key)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn the_options_popover_offers_the_linked_issue_then_the_existing_branch() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    let html = text(app.get("/tasks/compose", &cookie).await).await;

    let popover = tag_around(&html, "aria-label=\"Linked issue and branch\"");
    assert!(popover.contains("x-show=\"linking\""), "{popover}");
    let at = html.find("aria-label=\"Linked issue and branch\"").unwrap();
    let inside = &html[at..];
    let issue = inside.find(">Linked issue</label>").unwrap();
    let branch = inside.find(">Existing branch</label>").unwrap();
    assert!(issue < branch, "{inside}");
    let field = tag_around(&html, "id=\"branch\"");
    assert!(
        field.contains("name=\"branch\"") && field.contains("x-model=\"branch\""),
        "{field}"
    );
    assert!(
        inside.contains("Work on this branch instead of a new factory/task branch. An open pull request from it is attached."),
        "{inside}"
    );
}

#[tokio::test]
async fn an_entered_branch_shows_as_a_chip_with_the_git_icon_and_a_remove_button_after_the_issue_chip()
 {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    let html = text(
        app.post(
            "/tasks",
            &branch_form(&seed, "", "renovate/serde-1.x"),
            &cookie,
        )
        .await,
    )
    .await;

    assert!(
        html.contains("branch: &quot;renovate/serde-1.x&quot;")
            && html.contains("x-text=\"branch\""),
        "{html}"
    );
    let issue = html.find("aria-label=\"Remove linked issue\"").unwrap();
    let chip = html.find("aria-label=\"Remove existing branch\"").unwrap();
    assert!(
        issue < chip,
        "the branch chip follows the issue chip: {html}"
    );
    let chip = tag_around(&html, "x-show=\"branch\"");
    assert!(
        chip.contains("rounded-[10px]") && chip.contains("bg-accent-soft"),
        "{chip}"
    );
    let chip_at = html.find("x-show=\"branch\"").unwrap();
    let chip_html = &html[chip_at..chip_at + html[chip_at..].find("</span>").unwrap()];
    assert!(
        chip_html.contains("<svg") && chip_html.contains("Branch"),
        "{chip_html}"
    );
    let remove = tag_around(&html, "aria-label=\"Remove existing branch\"");
    assert!(remove.contains("branch = ''"), "{remove}");
}

#[tokio::test]
async fn a_branch_with_quotes_cannot_break_out_of_the_composer_state() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    let html = text(
        app.post(
            "/tasks",
            &branch_form(&seed, "", "a\"});alert(1);//"),
            &cookie,
        )
        .await,
    )
    .await;

    // A JSON string literal: the quote stays escaped inside the attribute.
    assert!(
        html.contains("branch: &quot;a\\&quot;});alert(1);//&quot;"),
        "{html}"
    );
}

#[tokio::test]
async fn creating_a_task_on_an_existing_branch_stores_it_and_attaches_the_open_pr() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    app.forge
        .push_list_open_pull_requests_result(Ok(vec![open_pr(40, "renovate/serde-1.x", "main")]));

    let response = app
        .post(
            "/tasks",
            &branch_form(&seed, "Rebase it", "renovate/serde-1.x"),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-redirect"), "/tasks/DF-1");
    assert_eq!(
        task_branch(&app, "DF-1").await,
        ("renovate/serde-1.x".to_owned(), Some(40), true)
    );
}

#[tokio::test]
async fn a_board_member_may_pick_a_branch_outside_the_agent_branches() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    let response = app
        .post(
            "/tasks",
            &branch_form(&seed, "Backport it", "release/1.2"),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        task_branch(&app, "DF-1").await,
        ("release/1.2".to_owned(), None, false)
    );
}

#[tokio::test]
async fn an_empty_branch_field_creates_a_task_on_its_own_branch() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    app.post("/tasks", &branch_form(&seed, "Fix it", "  "), &cookie)
        .await;

    assert_eq!(
        task_branch(&app, "DF-1").await,
        ("factory/task/DF-1".to_owned(), None, false)
    );
}

#[tokio::test]
async fn a_held_branch_keeps_the_composer_open_with_an_error_naming_the_holder() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    hold_branch(&app, &seed, "renovate/serde-1.x").await;

    let response = app
        .post(
            "/tasks",
            &branch_form(&seed, "Rebase it", "renovate/serde-1.x"),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("renovate/serde-1.x is held by DF-1. Finish or cancel DF-1 first, or pick another branch."),
        "{html}"
    );
    // Values are kept.
    assert!(html.contains(">Rebase it</textarea>"), "{html}");
    assert!(html.contains("issue: '7'"), "{html}");
    assert!(
        html.contains("branch: &quot;renovate/serde-1.x&quot;"),
        "{html}"
    );
    assert!(html.contains(&format!("<option value=\"{}\" selected>", seed.project)));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn rejected_branches_keep_the_composer_open_with_the_reason() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    for (branch, reason) in [
        ("main", "base branch"),
        ("factory/task/DF-7", "branch of the factory"),
        ("fix/../main", "not a valid branch name"),
        ("refs/heads/fix", "refs/heads"),
    ] {
        let response = app
            .post("/tasks", &branch_form(&seed, "Fix it", branch), &cookie)
            .await;

        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{branch}"
        );
        let html = text(response).await;
        assert!(html.contains(reason), "{branch}: {html}");
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn a_branch_with_several_open_prs_keeps_the_composer_open_naming_them() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    app.forge.push_list_open_pull_requests_result(Ok(vec![
        open_pr(42, "fix/shared", "main"),
        open_pr(43, "fix/shared", "release/1.2"),
    ]));

    let response = app
        .post(
            "/tasks",
            &branch_form(&seed, "Fix it", "fix/shared"),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(html.contains("#42, #43"), "{html}");
}

#[tokio::test]
async fn a_forge_failure_keeps_the_composer_open_with_the_forge_error() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    app.forge
        .push_list_open_pull_requests_result(Err(df_core::IntegrationError::Transport(
            "forge down".into(),
        )));

    let response = app
        .post(
            "/tasks",
            &branch_form(&seed, "Fix it", "fix/login"),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(text(response).await.contains("forge down"));
}

/// A task on `renovate/serde-1.x` whose PR #40 was opened outside the factory (`attached`) or by it.
async fn task_with_pr(app: &TestApp, seed: &Seed, attached: bool) {
    hold_branch(app, seed, "renovate/serde-1.x").await;
    sqlx::query(
        "UPDATE tasks SET pr_number = 40, pr_url = 'https://git.example.org/o/r/pulls/40', \
         pr_title = 'Update serde', pr_attached = $1",
    )
    .bind(attached)
    .execute(&app.pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn the_details_mark_an_attached_pr_with_a_pill_after_the_link_and_show_the_stored_branch() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    task_with_pr(&app, &seed, true).await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;

    let link = html
        .find("href=\"https://git.example.org/o/r/pulls/40\"")
        .unwrap();
    let links = &html[link..];
    let pill_at = links.find(">attached</span>").expect("attached pill");
    assert!(links.find("PR #40").unwrap() < pill_at, "{links}");
    let pill = tag_around(&html, "title=\"Opened outside the factory");
    assert!(
        pill.contains(
            "title=\"Opened outside the factory; the factory comments but never changes it\""
        ) && pill.contains("rounded-full")
            && pill.contains("bg-status-queued-bg"),
        "{pill}"
    );
    assert!(html.contains(">renovate/serde-1.x<"), "{html}");
}

#[tokio::test]
async fn the_details_of_a_factory_pr_have_no_attached_pill() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    task_with_pr(&app, &seed, false).await;

    let html = text(app.get("/tasks/DF-1", &cookie).await).await;

    assert!(html.contains("PR #40"), "{html}");
    assert!(!html.contains(">attached<"), "{html}");
}

#[tokio::test]
async fn a_message_to_a_final_task_on_a_held_branch_names_the_holder_and_how_to_free_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    hold_branch(&app, &seed, "renovate/serde-1.x").await;
    sqlx::query("UPDATE tasks SET status = 'cancelled' WHERE key = 'DF-1'")
        .execute(&app.pool)
        .await
        .unwrap();
    hold_branch(&app, &seed, "renovate/serde-1.x").await;

    let response = app
        .post(
            "/tasks/DF-1/messages",
            &form(&[("message", "try again")]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains(
            "Can't reopen: renovate/serde-1.x is held by DF-2. Finish or cancel DF-2 first."
        ),
        "{html}"
    );
}
