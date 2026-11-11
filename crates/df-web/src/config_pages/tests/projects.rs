//! Tests of creating, validating and editing projects.

use axum::http::StatusCode;

use super::{
    REPO, assert_project_created, assert_project_saved, audit, project_form,
    project_form_with_tracker, stored_key,
};
use crate::testkit::{TestApp, new_task, seed, tag_around, text};

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
    assert_project_created(&response);
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
    assert_project_created(&response);
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
    assert_project_created(&response);

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

#[tokio::test]
async fn the_edit_form_shows_the_key_read_only_with_and_without_tasks() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    for with_tasks in [false, true] {
        if with_tasks {
            new_task(&app.pool, &seed, "first").await;
        }
        let html = text(app.get("/projects/DF", &cookie).await).await;
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
    assert_project_saved(&ok);
    assert_eq!(audit(&app, "project.update").await.len(), 1);
}

#[tokio::test]
async fn the_project_form_pairs_key_with_name_and_base_branch_with_credential() {
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
    for full in [
        "repo_url",
        "issue_tracker_url",
        "image",
        "adapter_command",
        "webhook_wake",
    ] {
        assert!(spans_two(full), "{full} is full width");
    }
    for half in ["key", "name", "base_branch", "forge_credential_id"] {
        assert!(!spans_two(half), "{half} shares a row");
        assert!(
            html.contains(&format!("<div class=\"space-y-1\"><label for=\"{half}\"")),
            "{half}"
        );
    }
    // Rows in the order of the approved board.
    let order = [
        "key",
        "name",
        "repo_url",
        "base_branch",
        "forge_credential_id",
        "issue_tracker_url",
        "image",
        "adapter_command",
        "webhook_wake",
    ];
    let at: Vec<usize> = order
        .iter()
        .map(|n| html.find(&format!("for=\"{n}\"")).unwrap())
        .collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{at:?}");
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
