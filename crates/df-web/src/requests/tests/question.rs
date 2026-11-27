//! The question card (`request-inbox` spec: "One question card", "Answer input by kind", "Answer
//! errors on the card", "Answered question in history", "Choice targets on phones").

use axum::http::StatusCode;
use df_core::{QuestionAnswer, QuestionSpec, RequestId};
use df_db::requests::{RequestState, get_request, raise_question};
use serde_json::{Value, json};

use super::{Fixture, fixture};
use crate::testkit::{form, tag_around};

fn spec(value: Value) -> QuestionSpec {
    QuestionSpec::parse(serde_json::from_value(value).unwrap()).unwrap()
}

fn single() -> QuestionSpec {
    spec(json!({
        "text": "Which PostgreSQL version?", "kind": "single_choice",
        "options": [
            {"id": "pg16", "label": "PostgreSQL 16", "description": "Supported until 2028"},
            {"id": "pg17", "label": "PostgreSQL 17"},
        ],
    }))
}

fn multi() -> QuestionSpec {
    spec(json!({
        "text": "Which parts?", "kind": "multi_choice", "min_selected": 1, "max_selected": 3,
        "options": [
            {"id": "api", "label": "API"}, {"id": "ui", "label": "UI"},
            {"id": "cli", "label": "CLI"}, {"id": "docs", "label": "Docs"},
        ],
    }))
}

fn text() -> QuestionSpec {
    QuestionSpec::text_question("Why did the build fail?")
}

impl Fixture {
    async fn ask_spec(&self, question: QuestionSpec) -> RequestId {
        let agent = self.agent();
        raise_question(&self.app.pool, self.from(&agent), question)
            .await
            .unwrap()
            .0
            .id
    }

    async fn answer(&self, id: RequestId, pairs: &[(&str, &str)]) -> (StatusCode, String) {
        self.post(&format!("/requests/{id}/answer"), &form(pairs))
            .await
    }
}

/// `first` appears before `second` in `html`.
fn in_order(html: &str, first: &str, second: &str) -> bool {
    matches!((html.find(first), html.find(second)), (Some(a), Some(b)) if a < b)
}

#[tokio::test]
async fn a_question_card_has_its_parts_in_the_fixed_order_for_every_kind() {
    let f = fixture().await;
    f.ask_spec(single()).await;
    f.ask_spec(multi()).await;
    f.ask_spec(text()).await;
    let page = f.page("/requests").await;

    for (chip, question, input) in [
        (
            "Question · Single choice",
            "Which PostgreSQL version?",
            "<fieldset",
        ),
        (
            "Question · Multiple choice",
            "Which parts?",
            "Choose 1 to 3",
        ),
        ("Question · Text", "Why did the build fail?", "Your answer"),
    ] {
        let card = &page[page.find(chip).unwrap_or_else(|| panic!("{chip}"))..];
        let card = &card[..card.find("</article>").unwrap()];
        let parts = [question, input, ">Answer<", "<time"];
        let mut at = 0;
        for part in parts {
            let found = card[at..]
                .find(part)
                .unwrap_or_else(|| panic!("{part} in {card}"));
            at += found;
        }
    }
}

#[tokio::test]
async fn radios_and_checkboxes_post_selected_ids_and_nothing_is_preselected() {
    let f = fixture().await;
    f.ask_spec(single()).await;
    let page = f.page("/requests").await;

    for (id, label) in [("pg16", "PostgreSQL 16"), ("pg17", "PostgreSQL 17")] {
        let tag = tag_around(&page, &format!("value=\"{id}\""));
        assert!(
            tag.contains("type=\"radio\"") && tag.contains("name=\"selected\""),
            "{tag}"
        );
        assert!(page.contains(label), "{page}");
    }
    let other = tag_around(&page, "value=\"_other\"");
    assert!(
        other.contains("type=\"radio\"") && other.contains("name=\"selected\""),
        "{other}"
    );
    assert!(!page.contains("checked"), "{page}");
    assert!(page.contains("Supported until 2028"), "{page}");

    f.ask_spec(multi()).await;
    let page = f.page("/requests").await;
    let tag = tag_around(&page, "value=\"api\"");
    assert!(
        tag.contains("type=\"checkbox\"") && tag.contains("name=\"selected\""),
        "{tag}"
    );
}

#[tokio::test]
async fn labels_and_descriptions_are_shown_as_plain_text() {
    let f = fixture().await;
    f.ask_spec(spec(json!({
        "text": "Which?", "kind": "single_choice",
        "options": [
            {"id": "a", "label": "**bold** <b>x</b>", "description": "<i>why</i> `code`"},
            {"id": "b", "label": "B"},
        ],
    })))
    .await;
    let page = f.page("/requests").await;

    assert!(page.contains("**bold** &lt;b&gt;x&lt;/b&gt;"), "{page}");
    assert!(page.contains("&lt;i&gt;why&lt;/i&gt; `code`"), "{page}");
    assert!(
        !page.contains("<b>x</b>") && !page.contains("<strong>"),
        "{page}"
    );
}

#[tokio::test]
async fn a_multiple_choice_shows_its_limit_hint_and_the_other_input_only_when_allowed() {
    let f = fixture().await;
    f.ask_spec(multi()).await;
    f.ask_spec(spec(json!({
        "text": "Strict?", "kind": "single_choice", "allow_other": false,
        "options": [{"id": "x", "label": "X"}, {"id": "y", "label": "Y"}],
    })))
    .await;
    let page = f.page("/requests").await;

    let multi_card = &page[page.find("Which parts?").unwrap()..];
    let multi_card = &multi_card[..multi_card.find("</article>").unwrap()];
    assert!(multi_card.contains("Choose 1 to 3"), "{multi_card}");
    let hint = tag_around(multi_card, "Choose 1 to 3");
    let hint_id = hint
        .split("id=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(
        multi_card.contains(&format!("aria-describedby=\"{hint_id}")),
        "{multi_card}"
    );
    assert!(
        multi_card.contains("placeholder=\"Your Other answer\""),
        "{multi_card}"
    );
    assert!(multi_card.contains("maxlength=\"1000\""), "{multi_card}");

    let strict = &page[page.find("Strict?").unwrap()..];
    let strict = &strict[..strict.find("</article>").unwrap()];
    assert!(
        !strict.contains("value=\"_other\"") && !strict.contains("Your Other answer"),
        "{strict}"
    );
}

#[tokio::test]
async fn a_text_question_offers_the_plain_tabs_markdown_field_named_text() {
    let f = fixture().await;
    f.ask_spec(text()).await;
    f.ask_spec(QuestionSpec::text_question("Second text question?"))
        .await;
    let page = f.page("/requests").await;

    let area = tag_around(&page, "name=\"text\"");
    assert!(
        area.contains("<textarea") && area.contains("rows=\"4\""),
        "{area}"
    );
    assert!(
        page.contains(">Your answer<") && page.contains(">Write<") && page.contains(">Preview<"),
        "{page}"
    );
    assert!(
        page.contains("markdownField({&quot;plain&quot;:true})"),
        "{page}"
    );
    assert!(
        !page.contains("Highlighting") && !page.contains("role=\"radiogroup\""),
        "{page}"
    );
    // Two cards on one page keep unique ids.
    let ids: Vec<_> = page
        .match_indices("<textarea id=\"")
        .map(|(at, _)| &page[at..at + 60])
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

#[tokio::test]
async fn option_rows_are_44_px_below_1024_px() {
    let f = fixture().await;
    f.ask_spec(spec(json!({
        "text": "Which?", "kind": "single_choice",
        "options": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}],
    })))
    .await;
    let page = f.page("/requests").await;

    let label = page[..page.find("value=\"a\"").unwrap()]
        .rfind("<label")
        .unwrap();
    let row = &page[label..page[label..].find('>').unwrap() + label];
    assert!(
        row.contains("min-h-11") && row.contains("lg:min-h-9"),
        "{row}"
    );
    let answer = tag_around(&page, ">Answer<");
    assert!(
        answer.contains("min-h-11") && answer.contains("w-full"),
        "{answer}"
    );
}

#[tokio::test]
async fn the_answer_of_a_multiple_choice_is_read_from_repeated_selected_values() {
    let f = fixture().await;
    let id = f.ask_spec(multi()).await;

    let (status, card) = f
        .answer(
            id,
            &[
                ("selected", "api"),
                ("selected", "ui"),
                ("selected", "_other"),
                ("other", "also the CLI"),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{card}");
    let row = get_request(&f.app.pool, id).await.unwrap().unwrap();
    assert_eq!(row.state, RequestState::Answered);
    assert_eq!(
        row.answer(),
        Some(QuestionAnswer::Choice {
            selected: vec!["api".into(), "ui".into()],
            other: Some("also the CLI".into())
        })
    );
}

#[tokio::test]
async fn an_other_text_without_other_chosen_is_ignored() {
    let f = fixture().await;
    let id = f.ask_spec(single()).await;

    let (status, _) = f
        .answer(id, &[("selected", "pg16"), ("other", "maybe 15")])
        .await;

    assert_eq!(status, StatusCode::OK);
    let row = get_request(&f.app.pool, id).await.unwrap().unwrap();
    assert_eq!(
        row.answer(),
        Some(QuestionAnswer::Choice {
            selected: vec!["pg16".into()],
            other: None
        })
    );
}

#[tokio::test]
async fn a_rejected_answer_returns_the_card_with_the_values_and_the_message_under_the_input() {
    let f = fixture().await;
    let id = f.ask_spec(multi()).await;

    let (status, card) = f
        .answer(
            id,
            &[
                ("selected", "api"),
                ("selected", "ui"),
                ("selected", "cli"),
                ("selected", "docs"),
            ],
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(card.contains("Choose at most 3 options."), "{card}");
    assert_eq!(card.matches(" checked").count(), 4, "{card}");
    let fieldset = tag_around(&card, "aria-labelledby");
    assert!(fieldset.contains("aria-invalid=\"true\""), "{fieldset}");
    let error = tag_around(&card, "Choose at most 3 options.");
    assert!(
        error.contains("role=\"alert\"") && error.contains("text-danger"),
        "{error}"
    );
    let error_id = error
        .split("id=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(fieldset.contains(error_id), "{fieldset}");
    assert!(
        in_order(&card, "Choose at most 3 options.", ">Answer<"),
        "{card}"
    );
    assert!(
        in_order(&card, "value=\"_other\"", "Choose at most 3 options."),
        "{card}"
    );
    let row = get_request(&f.app.pool, id).await.unwrap().unwrap();
    assert_eq!(row.state, RequestState::Pending);
}

#[tokio::test]
async fn each_rejected_answer_names_its_field_and_keeps_the_submitted_text() {
    let f = fixture().await;
    let single_id = f.ask_spec(single()).await;
    let text_id = f.ask_spec(text()).await;

    let (status, card) = f.answer(single_id, &[]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(card.contains("Choose one option."), "{card}");

    let (status, card) = f
        .answer(single_id, &[("selected", "_other"), ("other", "  ")])
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(card.contains("Write your Other answer."), "{card}");
    let other = tag_around(&card, "name=\"other\"");
    assert!(other.contains("aria-invalid=\"true\""), "{other}");
    assert!(
        tag_around(&card, "value=\"_other\"").contains("checked"),
        "{card}"
    );

    let (status, card) = f.answer(text_id, &[("text", "   ")]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(card.contains("Write an answer."), "{card}");
    assert!(
        tag_around(&card, "name=\"text\"").contains("aria-invalid=\"true\""),
        "{card}"
    );
    assert!(in_order(&card, "Write an answer.", ">Answer<"), "{card}");
}

#[tokio::test]
async fn other_chosen_on_a_question_without_other_shows_its_message_below_the_options() {
    let f = fixture().await;
    let id = f
        .ask_spec(spec(json!({
            "text": "Strict?", "kind": "single_choice", "allow_other": false,
            "options": [{"id": "x", "label": "X"}, {"id": "y", "label": "Y"}],
        })))
        .await;

    let (status, card) = f
        .answer(id, &[("selected", "_other"), ("other", "maybe")])
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        card.contains("This question has no Other answer."),
        "{card}"
    );
    assert!(
        in_order(&card, "value=\"y\"", "This question has no Other answer."),
        "{card}"
    );
    assert!(
        in_order(&card, "This question has no Other answer.", ">Answer<"),
        "{card}"
    );
    assert!(
        tag_around(&card, "aria-labelledby").contains("aria-invalid=\"true\""),
        "{card}"
    );
    let error = tag_around(&card, "This question has no Other answer.");
    assert!(error.contains("role=\"alert\""), "{error}");
    let error_id = error
        .split("id=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert!(
        tag_around(&card, "aria-labelledby").contains(error_id),
        "{card}"
    );
    assert_eq!(
        get_request(&f.app.pool, id).await.unwrap().unwrap().state,
        RequestState::Pending
    );
}

#[tokio::test]
async fn several_text_questions_on_one_page_have_their_own_tabs_with_unique_ids() {
    let f = fixture().await;
    for text in ["First?", "Second?", "Third?"] {
        f.ask_spec(QuestionSpec::text_question(text)).await;
    }
    let page = f.page("/requests").await;

    assert_eq!(page.matches("role=\"tablist\"").count(), 3, "{page}");
    assert_eq!(page.matches(">Write<").count(), 3, "{page}");
    let ids: Vec<&str> = page
        .split(" id=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(
        unique.len(),
        ids.len(),
        "duplicate ids on the page: {ids:?}"
    );
    // The cards never name the editor bundle (the page body may: the component never loads it).
    let cards = &page[page.find("<article").unwrap()..];
    assert!(
        !cards.contains("editor.js") && !cards.contains("Highlighting"),
        "{cards}"
    );
}

#[tokio::test]
async fn an_answered_choice_lists_every_option_with_the_chosen_ones_marked() {
    let f = fixture().await;
    let id = f.ask_spec(multi()).await;
    f.answer(
        id,
        &[
            ("selected", "api"),
            ("selected", "_other"),
            ("other", "also the CLI"),
        ],
    )
    .await;

    let page = f.page("/tasks/DF-1").await;
    let card = &page[page.find(&format!("id=\"request-{id}\"")).unwrap()..];
    let card = &card[..card.find("</article>").unwrap()];

    for label in ["API", "UI", "CLI", "Docs"] {
        assert!(card.contains(label), "{label}: {card}");
    }
    assert!(
        !card.contains("<input") && !card.contains(">Answer<"),
        "{card}"
    );
    assert_eq!(card.matches(">chosen<").count(), 2, "{card}");
    assert!(card.contains("Other: also the CLI"), "{card}");
    assert!(card.contains("Answered by Peter"), "{card}");
    assert!(in_order(card, ">API<", "Other: also the CLI"), "{card}");
    let api = card[..card.find("API").unwrap()].rfind("<li").unwrap();
    assert!(
        card[api..].starts_with("<li")
            && card[api..card[api..].find("</li>").unwrap() + api].contains("chosen"),
        "{card}"
    );
}

#[tokio::test]
async fn an_answered_text_question_shows_the_answer_rendered_as_markdown() {
    let f = fixture().await;
    let id = f.ask_spec(text()).await;
    f.answer(
        id,
        &[("text", "Use the **old** endpoint <script>x</script>")],
    )
    .await;

    let page = f.page("/tasks/DF-1").await;
    assert!(page.contains("<strong>old</strong>"), "{page}");
    assert!(!page.contains("<script>x</script>"), "{page}");
    assert!(page.contains("Answered by Peter"), "{page}");
}

#[tokio::test]
async fn a_migrated_question_with_a_single_option_is_shown_and_answerable() {
    let f = fixture().await;
    let id = f.ask_spec(single()).await;
    sqlx::query("UPDATE agent_requests SET payload = jsonb_set(payload, '{options}', '[{\"id\":\"a1\",\"label\":\"Only\"}]') WHERE id = $1")
        .bind(id.as_uuid())
        .execute(&f.app.pool)
        .await
        .unwrap();

    let page = f.page("/requests").await;
    assert!(
        page.contains("Only") && page.contains("value=\"a1\""),
        "{page}"
    );
    let (status, _) = f.answer(id, &[("selected", "a1")]).await;
    assert_eq!(status, StatusCode::OK);
}

fn question_key(html: &str) -> String {
    let tag = tag_around(html, "data-question=");
    tag.split("data-question=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn a_pending_question_form_carries_the_request_id_and_a_hash_that_stays_the_same() {
    let f = fixture().await;
    let id = f.ask_spec(single()).await;

    let first = question_key(&f.page("/requests").await);
    let second = question_key(&f.page("/tasks/DF-1").await);

    assert!(first.starts_with(&format!("{id}:")), "{first}");
    assert_eq!(first, second);
}

#[tokio::test]
async fn the_task_stream_sends_the_card_again_with_the_new_title_after_a_rename() {
    let f = fixture().await;
    f.ask_spec(single()).await;
    let page = f.page("/tasks/DF-1").await;
    let url = super::task_stream_url(&page);
    let mut sse = f.events(&url).await;
    let _ = sse.next().await;

    f.post("/tasks/DF-1/title", &form(&[("title", "Renamed backups")]))
        .await;

    let mut frame = sse.next_within(super::LIVE).await;
    while !frame.data.contains("<template data-request-card") {
        frame = sse.next_within(super::LIVE).await;
    }
    assert!(frame.data.contains("Renamed backups"), "{}", frame.data);
}

fn upserts(data: &str) -> usize {
    data.matches("<template data-request-card").count()
}

#[tokio::test]
async fn the_inbox_stream_sends_one_upsert_for_a_new_request_and_nothing_for_unchanged_cards() {
    let f = fixture().await;
    f.ask("first?", &[]).await;
    let mut sse = f.events("/requests/events?list=1").await;
    let first = sse.next().await;
    assert_eq!(upserts(&first.data), 1, "{}", first.data);

    f.ask("second?", &[]).await;
    let update = sse.next_within(super::LIVE).await;

    assert_eq!(upserts(&update.data), 1, "{}", update.data);
    assert!(
        update.data.contains("second?") && !update.data.contains("first?"),
        "{}",
        update.data
    );
    assert!(
        update
            .data
            .contains("hx-target=\"#request-updates\" hx-swap=\"beforeend\""),
        "{}",
        update.data
    );
}

#[tokio::test]
async fn the_inbox_stream_deletes_a_decided_card_and_sends_the_empty_state_with_the_last() {
    let f = fixture().await;
    let first = f.ask("first?", &[]).await;
    let second = f.ask("second?", &[]).await;
    let mut sse = f.events("/requests/events?list=1").await;
    let _ = sse.next().await;

    f.answer(first, &[("text", "a")]).await;
    let update = sse.next_within(super::LIVE).await;
    assert!(
        update
            .data
            .contains(&format!("data-request-delete=\"request-{first}\"")),
        "{}",
        update.data
    );
    assert_eq!(upserts(&update.data), 0, "{}", update.data);

    f.answer(second, &[("text", "b")]).await;
    let update = sse.next_within(super::LIVE).await;
    assert!(
        update.data.contains("Nothing waits for you"),
        "{}",
        update.data
    );
}

#[tokio::test]
async fn the_inbox_stream_upserts_a_changed_card() {
    let f = fixture().await;
    let id = f.ask("first?", &[]).await;
    let mut sse = f.events("/requests/events?list=1").await;
    let _ = sse.next().await;

    sqlx::query("UPDATE tasks SET title = 'Renamed' WHERE id = $1")
        .bind(f.task.as_uuid())
        .execute(&f.app.pool)
        .await
        .unwrap();
    df_db::publish(
        &f.app.pool,
        &df_db::Notification::RequestChanged {
            task_id: f.task,
            request_id: id,
        },
    )
    .await
    .unwrap();
    let update = sse.next_within(super::LIVE).await;

    assert_eq!(upserts(&update.data), 1, "{}", update.data);
    assert!(update.data.contains("Renamed"), "{}", update.data);
}
