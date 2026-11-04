use std::time::Duration;

use axum::http::StatusCode;
use df_db::push_checks::{CheckMode, PushPolicy, push_policy};
use df_db::{Notification, Notifier};

use super::*;
use crate::testkit::{TestApp, form, header_str, seed, tag_around, text};

/// The push checks of the board "Projects · edit modal, push checks": the default policy.
fn default_form() -> PushPolicyForm {
    PushPolicyForm::from_policy(&PushPolicy::default())
}

fn with_globs(globs: &str) -> PushPolicyForm {
    PushPolicyForm {
        protected_globs: globs.into(),
        ..default_form()
    }
}

/// The edit form of project `DF` from the seed with the given push check fields.
fn edit_body(push: &[(&str, &str)]) -> String {
    let mut pairs = vec![
        ("key", "DF"),
        ("name", "Dark Factory"),
        ("repo_url", "https://git.example.org/peter/dark-factory.git"),
        ("issue_tracker_url", ""),
        ("image", "img:latest"),
        ("base_branch", "main"),
        ("adapter_command", ""),
        ("forge_credential_id", ""),
    ];
    pairs.extend_from_slice(push);
    form(&pairs)
}

const DEFAULT_GLOBS: &str =
    ".forgejo/workflows/**\n.github/workflows/**\n.gitea/workflows/**\nCODEOWNERS";

#[test]
fn the_form_of_the_default_policy_validates_to_it() {
    assert_eq!(validate(&default_form()), Ok(Some(PushPolicy::default())));
}

#[test]
fn globs_are_one_per_line_trimmed_without_blank_lines() {
    let policy = validate(&with_globs("deploy/**\r\n\r\n  CODEOWNERS  \n"))
        .unwrap()
        .unwrap();
    assert_eq!(policy.protected_globs, ["deploy/**", "CODEOWNERS"]);
}

#[test]
fn an_invalid_glob_names_its_line_and_the_reason() {
    let errors = validate(&with_globs(&format!("{DEFAULT_GLOBS}\ndeploy/[\nsrc/{{a"))).unwrap_err();
    assert_eq!(
        errors.get("protected_globs"),
        Some("Line 5: \"deploy/[\" is not a valid glob (unclosed character class).")
    );
}

#[test]
fn the_globs_textarea_shows_every_line_and_at_least_four() {
    let rows = |globs: &str| {
        let html = panel(&with_globs(globs), &FormErrors::default()).into_string();
        let textarea = tag_around(&html, "id=\"protected_globs\"");
        textarea
            .split_once("rows=\"")
            .and_then(|(_, rest)| rest.split_once('"'))
            .map(|(rows, _)| rows.to_owned())
    };

    assert_eq!(
        rows(&format!("{DEFAULT_GLOBS}\ndeploy/[")).as_deref(),
        Some("5")
    );
    assert_eq!(rows("CODEOWNERS").as_deref(), Some("4"));
    assert_eq!(rows("").as_deref(), Some("4"));
}

#[test]
fn the_size_limit_is_a_whole_number_of_mib_in_range() {
    let size = |value: &str| {
        validate(&PushPolicyForm {
            max_blob_mib: value.into(),
            ..default_form()
        })
    };
    assert_eq!(
        size(" 12 ").unwrap().unwrap().max_blob_bytes,
        12 * 1024 * 1024
    );
    assert_eq!(
        size("1024").unwrap().unwrap().max_blob_bytes,
        1024 * 1024 * 1024
    );
    for invalid in ["", "0", "1.5", "-1", "1025", "five"] {
        let errors = size(invalid).unwrap_err();
        assert_eq!(
            errors.get("max_blob_mib"),
            Some("Enter a whole number of MiB from 1 to 1024."),
            "{invalid}"
        );
    }
}

#[test]
fn modes_and_the_switch_are_read_from_their_fields() {
    let form = PushPolicyForm {
        protected_mode: Some("reject".into()),
        size_mode: Some("reject".into()),
        linear_history: "on".into(),
        ..default_form()
    };
    let policy = validate(&form).unwrap().unwrap();
    assert_eq!(
        (policy.protected_mode, policy.secret_mode, policy.size_mode),
        (CheckMode::Reject, CheckMode::Approval, CheckMode::Reject)
    );
    assert!(policy.linear_history);

    let errors = validate(&PushPolicyForm {
        secret_mode: Some("warn".into()),
        ..default_form()
    })
    .unwrap_err();
    assert_eq!(
        errors.get("secret_mode"),
        Some("Choose Ask for approval or Reject.")
    );
    let errors = validate(&PushPolicyForm {
        size_mode: None,
        ..default_form()
    })
    .unwrap_err();
    assert_eq!(
        errors.get("size_mode"),
        Some("Choose Ask for approval or Reject.")
    );
}

#[test]
fn a_form_without_the_push_check_fields_leaves_the_policy_alone() {
    assert_eq!(validate(&PushPolicyForm::default()), Ok(None));
}

#[tokio::test]
async fn the_edit_modal_has_a_push_checks_tab_with_the_default_policy() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    let html = text(app.get_htmx("/projects/DF/edit", &cookie).await).await;

    assert!(
        html.contains("x-data=\"tabs(&quot;general&quot;)\""),
        "{html}"
    );
    let general = tag_around(&html, "id=\"tab-general\"");
    assert!(
        general.contains("role=\"tab\"") && general.contains("aria-selected=\"true\""),
        "{general}"
    );
    let push = tag_around(&html, "id=\"tab-push-checks\"");
    assert!(
        push.contains("aria-selected=\"false\"")
            && push.contains("aria-controls=\"panel-push-checks\""),
        "{push}"
    );
    assert!(tag_around(&html, "id=\"panel-push-checks\"").contains("x-cloak"));
    for text in [
        ">General<",
        ">Push checks<",
        "Checked on every push of this project's tasks, before it reaches the forge.",
        "One glob per line. A commit that adds, changes, deletes or renames a matching path is a finding.",
        "Changes to the paths above.",
        "Every added file, binary files included. Matches are masked.",
        "Largest file a commit may add.",
        "Reject merge commits.",
        "Always rejected, whatever the policy: commits whose author or committer is not the instance's bot identity, and changes to .dark-factory/**.",
    ] {
        assert!(html.contains(text), "{text}");
    }
    let textarea = &html[html.find("id=\"protected_globs\"").unwrap()..];
    assert!(
        textarea.contains(&format!(">{DEFAULT_GLOBS}</textarea>")),
        "{textarea}"
    );
    for mode in ["protected_mode", "secret_mode", "size_mode"] {
        let select = &html[html.find(&format!("id=\"{mode}\"")).unwrap()..];
        let select = &select[..select.find("</select>").unwrap()];
        assert!(
            select.contains("<option value=\"approval\" selected>Ask for approval</option>"),
            "{select}"
        );
        assert!(
            select.contains("<option value=\"reject\">Reject</option>"),
            "{select}"
        );
    }
    assert!(tag_around(&html, "id=\"max_blob_mib\"").contains("value=\"5\""));
    assert!(html.contains(">MiB<"));
    let switch = tag_around(&html, "id=\"linear_history\"");
    assert!(
        switch.contains("role=\"switch\"") && !switch.contains("checked"),
        "{switch}"
    );
}

#[tokio::test]
async fn a_saved_policy_is_stored_audited_announced_and_shown_again() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let notifier = Notifier::start(&app.pool).await.unwrap();
    let mut notifications = notifier.subscribe();
    let globs = format!("{DEFAULT_GLOBS}\ndeploy/**");

    let response = app
        .post(
            "/projects/DF",
            &edit_body(&[
                ("protected_globs", &globs),
                ("protected_mode", "approval"),
                ("secret_mode", "reject"),
                ("size_mode", "approval"),
                ("max_blob_mib", "8"),
                ("linear_history", "on"),
            ]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-trigger"), "modal-close");
    let stored = push_policy(&app.pool, seed.project).await.unwrap();
    assert_eq!(
        stored,
        PushPolicy {
            protected_globs: globs.lines().map(str::to_owned).collect(),
            secret_mode: CheckMode::Reject,
            max_blob_bytes: 8 * 1024 * 1024,
            linear_history: true,
            ..PushPolicy::default()
        }
    );
    let audit: Vec<serde_json::Value> =
        sqlx::query_scalar("SELECT details FROM audit_log WHERE action = 'push_policy.create'")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["after"]["protected_globs"][4], "deploy/**");
    tokio::time::timeout(Duration::from_secs(5), async {
        while notifications.recv().await.unwrap() != Notification::EgressPolicy {}
    })
    .await
    .unwrap();

    let html = text(app.get_htmx("/projects/DF/edit", &cookie).await).await;
    assert!(html.contains(&format!(">{globs}</textarea>")), "{html}");
    assert!(tag_around(&html, "id=\"linear_history\"").contains("checked"));

    // The general fields alone (an older client) keep the stored policy.
    let response = app.post("/projects/DF", &edit_body(&[]), &cookie).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(push_policy(&app.pool, seed.project).await.unwrap(), stored);
    // Re-rendered after an error, such a form shows the stored policy, not an empty one.
    let unnamed = edit_body(&[]).replace("name=Dark+Factory", "name=");
    let response = app.post("/projects/DF", &unnamed, &cookie).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("x-data=\"tabs(&quot;general&quot;)\""),
        "{html}"
    );
    assert!(html.contains(&format!(">{globs}</textarea>")), "{html}");
}

#[tokio::test]
async fn the_edit_modal_is_wide_and_its_tabs_follow_the_keyboard() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;

    let html = text(app.get_htmx("/projects/DF/edit", &cookie).await).await;

    assert!(tag_around(&html, "dialog id=\"modal\"").contains("md:max-w-3xl"));
    let tablist = tag_around(&html, "role=\"tablist\"");
    assert!(
        tablist.contains("x-on:keydown=\"move($event)\""),
        "{tablist}"
    );
    let script = crate::assets::script_source("tabs.js");
    assert!(script.contains("Alpine.data('tabs'"));
    for key in [
        "ArrowUp",
        "ArrowDown",
        "ArrowLeft",
        "ArrowRight",
        "'Home'",
        "'End'",
    ] {
        assert!(script.contains(key), "{key}");
    }
    // The new-project form has no tabs.
    let new = text(app.get_htmx("/projects/new", &cookie).await).await;
    assert!(!new.contains("role=\"tablist\"") && !new.contains("protected_globs"));
}

#[tokio::test]
async fn an_invalid_glob_reopens_the_push_checks_tab_with_the_error_on_the_field() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let globs = format!("{DEFAULT_GLOBS}\ndeploy/[");

    let response = app
        .post(
            "/projects/DF",
            &edit_body(&[
                ("protected_globs", &globs),
                ("protected_mode", "approval"),
                ("secret_mode", "approval"),
                ("size_mode", "approval"),
                ("max_blob_mib", "5"),
            ]),
            &cookie,
        )
        .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("x-data=\"tabs(&quot;push-checks&quot;)\""),
        "{html}"
    );
    let textarea = tag_around(&html, "id=\"protected_globs\"");
    assert!(
        textarea.contains("aria-invalid=\"true\"")
            && textarea.contains("aria-describedby=\"protected_globs-error protected_globs-hint\""),
        "{textarea}"
    );
    let error = &html[html.find("id=\"protected_globs-error\"").unwrap()..];
    assert!(
        error.starts_with(
            "id=\"protected_globs-error\" class=\"text-sm text-danger\" role=\"alert\">Line 5: &quot;deploy/[&quot; is not a valid glob (unclosed character class).</p>"
        ),
        "{error}"
    );
    assert!(error.find("protected_globs-hint").is_some());
    assert!(html.contains(&format!(">{globs}</textarea>")));
    assert_eq!(
        push_policy(&app.pool, seed.project).await.unwrap(),
        PushPolicy::default()
    );
}

#[tokio::test]
async fn the_tab_row_scrolls_on_phones_and_keeps_the_open_tab_in_view() {
    let app = TestApp::new().await;
    seed(&app.pool).await;

    let html = text(app.get_htmx("/projects/DF/edit", &app.login().await).await).await;

    let tablist = tag_around(&html, "role=\"tablist\"");
    assert!(
        tablist.contains("overflow-x-auto") && tablist.contains("md:overflow-visible"),
        "{tablist}"
    );
    assert!(tag_around(&html, "role=\"tab\"").contains("shrink-0"));
    let script = crate::assets::script_source("tabs.js");
    assert!(
        script.contains("scrollIntoView({ block: 'nearest', inline: 'nearest' })"),
        "{script}"
    );
    assert!(script.contains("init()") && script.contains("$watch('tab'"));
}
