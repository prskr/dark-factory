//! Task creation and the composer.

use axum::http::StatusCode;

use crate::testkit::{TestApp, form, header_str, seed, tag_around, text};

fn task_form(seed: &crate::testkit::Seed, description: &str) -> String {
    form(&[
        ("description", description),
        ("project", &seed.project.to_string()),
        ("persona", &seed.persona.to_string()),
        ("issue_number", ""),
    ])
}

#[tokio::test]
async fn creating_a_task_opens_its_page_as_queued_with_a_placeholder_title() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    let response = app
        .post(
            "/tasks",
            &task_form(
                &seed,
                "The nightly backup runs twice. It started last week.",
            ),
            &cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_str(&response, "hx-redirect"), "/tasks/DF-1");

    let page = text(app.get("/tasks/DF-1", &cookie).await).await;
    assert!(page.contains("DF-1") && page.contains("The nightly backup runs twice."));
    assert!(page.contains(">queued<"), "{page}");
    let (created_by, source): (Option<uuid::Uuid>, String) =
        sqlx::query_as("SELECT created_by, title_source FROM tasks")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(created_by.is_some());
    assert_eq!(source, "placeholder");
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'task.create' AND resource_id = 'DF-1'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audit, 1);
}

#[tokio::test]
async fn missing_description_keeps_the_composer_open_with_project_and_assignee() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let response = app.post("/tasks", &task_form(&seed, "  "), &cookie).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let html = text(response).await;
    assert!(
        html.contains("Describe what the agent should do."),
        "{html}"
    );
    assert!(html.contains(&format!("<option value=\"{}\" selected>", seed.project)));
    assert!(html.contains(&format!("<option value=\"{}\" selected>", seed.persona)));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn task_creation_needs_a_known_project_and_persona_and_a_numeric_issue() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let body = form(&[
        ("description", "d"),
        ("project", "nope"),
        ("persona", ""),
        ("issue_number", "x"),
    ]);
    let html = text(app.post("/tasks", &body, &cookie).await).await;
    assert!(html.contains("Choose a project.") && html.contains("Choose an assignee."));
    let seed = seed(&app.pool).await;
    let body = form(&[
        ("description", "d"),
        ("project", &seed.project.to_string()),
        ("persona", &seed.persona.to_string()),
        ("issue_number", "0"),
    ]);
    assert!(
        text(app.post("/tasks", &body, &cookie).await)
            .await
            .contains("Must be a positive number.")
    );
}

#[tokio::test]
async fn linked_issue_is_stored_and_shown_as_a_chip_when_the_composer_reopens() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let body = form(&[
        ("description", ""),
        ("project", &seed.project.to_string()),
        ("persona", &seed.persona.to_string()),
        ("issue_number", "7"),
    ]);
    let html = text(app.post("/tasks", &body, &cookie).await).await;
    assert!(
        html.contains("issue: '7'") && html.contains("x-text=\"issue\""),
        "{html}"
    );

    let body = form(&[
        ("description", "Fix it"),
        ("project", &seed.project.to_string()),
        ("persona", &seed.persona.to_string()),
        ("issue_number", "7"),
    ]);
    app.post("/tasks", &body, &cookie).await;
    let issue: Option<i64> = sqlx::query_scalar("SELECT issue_number FROM tasks")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(issue, Some(7));
}

#[tokio::test]
async fn composer_defaults_to_the_filtered_project_and_offers_the_keyboard_shortcuts() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;

    let html = text(app.get("/tasks/compose?project=DF", &cookie).await).await;
    assert!(html.contains("<dialog id=\"composer\""), "{html}");
    assert!(html.contains(&format!("<option value=\"{}\" selected>", seed.project)));
    assert!(html.contains("data-explicit=\"true\""));
    assert!(
        html.contains("x-on:keydown.ctrl.enter.prevent")
            && html.contains("x-on:keydown.meta.enter.prevent")
    );
    assert!(html.contains("localStorage.getItem('df.composer')"));
    assert!(html.contains("aria-label=\"Close\""));
    // An unknown project key falls back to the first project without marking it explicit.
    let html = text(app.get("/tasks/compose?project=NOPE", &cookie).await).await;
    assert!(!html.contains("data-explicit"), "{html}");
    assert_eq!(
        app.get("/tasks/new", &cookie).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn composer_remove_issue_button_is_the_icon_button() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let html = text(app.get("/tasks/compose", &cookie).await).await;
    let tag = tag_around(&html, "aria-label=\"Remove linked issue\"");
    assert!(
        tag.contains(crate::components::actions::ICON_BUTTON_CLASS),
        "{tag}"
    );
}

#[tokio::test]
async fn composer_is_a_frameless_card_with_a_named_round_submit_button() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let html = text(app.get("/tasks/compose", &cookie).await).await;

    assert!(
        html.contains("bg-transparent"),
        "no solid modal frame: {html}"
    );
    let chips = html.find("name=\"project\"").unwrap();
    let card = html
        .find("rounded-2xl border border-line bg-panel")
        .unwrap();
    let description = html.find("name=\"description\"").unwrap();
    let assignee = html.find("name=\"persona\"").unwrap();
    assert!(chips < card && card < description, "chips above the card");
    assert!(card < assignee, "assignee inside the card");
    let submit = &html[html.find("aria-label=\"Create task\"").unwrap()..];
    assert!(submit.contains("rounded-full") && submit.contains("<svg"));
    assert!(html.find("type=\"submit\"").unwrap() > assignee);
}

#[tokio::test]
async fn the_composer_has_a_link_issue_plus_on_the_left_and_the_assignee_next_to_submit() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let html = text(app.get("/tasks/compose", &cookie).await).await;
    assert!(!html.contains("Options"), "no Options text");
    let plus = html.find("aria-label=\"Link issue\"").unwrap();
    let assignee = html.find("name=\"persona\"").unwrap();
    let submit = html.find("aria-label=\"Create task\"").unwrap();
    let description = html.find("name=\"description\"").unwrap();
    assert!(
        description < plus && plus < assignee && assignee < submit,
        "{html}"
    );
    // Nothing but the end of the picker sits between it and the submit button.
    let between = &html[assignee..submit];
    assert!(
        between.matches("<button").count() == 1 && between.matches("<select").count() == 0,
        "{between}"
    );
    // The button itself: round, 44 px below 1024 px (36 px from there), with its icon inside.
    let start = html[..plus].rfind("<button").unwrap();
    let end = start + html[start..].find("</button>").unwrap();
    let plus_button = &html[start..end];
    assert!(
        plus_button.contains("rounded-full")
            && plus_button.contains("size-11")
            && plus_button.contains("lg:size-9")
            && !plus_button.contains("md:size-9")
            && plus_button.contains("<svg"),
        "{plus_button}"
    );
    assert!(
        html.contains("aria-label=\"Remove linked issue\"")
            && html.contains("aria-label=\"Linked issue\"")
    );
}

#[tokio::test]
async fn the_composer_chips_are_rounded_rectangles_the_persona_has_no_border_and_create_is_an_arrow()
 {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let html = text(app.get("/tasks/compose", &cookie).await).await;
    let project = tag_around(&html, "name=\"project\"");
    assert!(
        project.contains("rounded-[10px]") && !project.contains("rounded-full"),
        "{project}"
    );
    let persona = tag_around(&html, "name=\"persona\"");
    assert!(
        !persona.contains(" border ")
            && !persona.contains("border-line")
            && persona.contains("bg-transparent"),
        "{persona}"
    );
    // The arrow-up of the mockup, not the paper plane.
    let submit = &html[html.find("aria-label=\"Create task\"").unwrap()..];
    let submit = &submit[..submit.find("</button>").unwrap()];
    assert!(
        submit.contains("M12 19V5") && submit.contains("m5 12 7-7 7 7"),
        "{submit}"
    );
    // No accent outline on the text; the card shows focus with a stronger border instead.
    let textarea = tag_around(&html, "name=\"description\"");
    assert!(
        textarea.contains("focus-visible:outline-none"),
        "{textarea}"
    );
    let card = tag_around(&html, "rounded-2xl border border-line bg-panel");
    assert!(card.contains("focus-within:border-line-strong"), "{card}");
    // Neutral: no accent anywhere in the focus treatment (the user asked to drop it).
    assert!(
        !card.contains("accent") && !textarea.contains("accent"),
        "{card} {textarea}"
    );
    let css = crate::app_css().unwrap();
    assert!(
        css.contains("focus-within"),
        "focus-within rule in the stylesheet"
    );
    // The close button is the shared icon button.
    let close = tag_around(&html, "aria-label=\"Close\"");
    assert!(close.contains("hover:bg-sunk"), "{close}");
}

#[tokio::test]
async fn the_link_issue_plus_opens_a_popover_with_the_number_field_and_a_chip_shows_it() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    seed(&app.pool).await;
    let html = text(app.get("/tasks/compose", &cookie).await).await;

    // The plus: named, expanded state bound, active look while open (accent border + soft fill).
    let plus = tag_around(&html, "aria-label=\"Link issue\"");
    assert!(
        plus.contains(":aria-expanded=\"linking\"")
            && plus.contains("x-on:click=\"linking = !linking\""),
        "{plus}"
    );
    assert!(plus.contains("border-accent-text bg-accent-soft"), "{plus}");
    // The popover: anchored below the card on the left, 280 px, panel surface, named.
    let popover = tag_around(&html, "aria-label=\"Linked issue\"");
    for class in [
        "absolute",
        "left-3",
        "top-[calc(100%-8px)]",
        "w-[280px]",
        "rounded-xl",
        "border",
        "bg-panel",
        "p-3",
        "shadow-lg",
    ] {
        assert!(popover.contains(class), "{class}: {popover}");
    }
    // On phones (bottom sheet) it sits inline below the card instead.
    assert!(
        popover.contains("max-md:static") && popover.contains("max-md:w-full"),
        "{popover}"
    );
    assert!(popover.contains("x-show=\"linking\""));
    // Label, mono # prefix, numeric field under the unchanged form field name, help sentence.
    let at = html.find("aria-label=\"Linked issue\"").unwrap();
    let inside = &html[at..];
    assert!(inside.contains(">Linked issue</label>"), "{inside}");
    assert!(inside.contains(">#</span>") && inside.contains("font-mono"));
    let field = tag_around(&html, "id=\"issue_number\"");
    assert!(
        field.contains("name=\"issue_number\"")
            && field.contains("inputmode=\"numeric\"")
            && field.contains("x-model=\"issue\""),
        "{field}"
    );
    assert!(
        inside.contains("An existing issue in ")
            && inside.contains("tracker that this task works on."),
        "{inside}"
    );
    // The chip above the card: "Issue #n" with a remove button that clears the number.
    let chip = &html[html.find("x-show=\"issue\"").unwrap()..];
    let chip = &chip[..chip.find("</span></span>").unwrap_or(400.min(chip.len()))];
    assert!(
        chip.contains("Issue") && chip.contains("x-text=\"issue\"") && chip.contains("font-mono"),
        "{chip}"
    );
    let remove = tag_around(&html, "aria-label=\"Remove linked issue\"");
    assert!(remove.contains("x-on:click=\"issue = ''\""), "{remove}");
    // Escape and a click outside close the popover only.
    let card = tag_around(&html, "rounded-2xl border border-line bg-panel");
    assert!(
        card.contains("relative") && card.contains("x-on:click.outside=\"linking = false\""),
        "{card}"
    );
    assert!(card.contains("x-on:keydown.escape=\"if (linking) { $event.preventDefault(); $event.stopPropagation(); linking = false }\""), "{card}");
}
