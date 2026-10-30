use std::time::Duration;

use axum::http::StatusCode;
use df_core::{Principal, RequestId, TaskId};
use df_db::requests::{raise_approval, raise_question};
use serde_json::json;

use crate::testkit::{SseReader, TestApp, add_event, form, insert_run, new_task, seed, text};

const LIVE: Duration = Duration::from_secs(1);
const _: () = assert!(LIVE.as_millis() * 2 <= super::POLL.as_millis());

struct Fixture {
    app: TestApp,
    cookie: String,
    task: TaskId,
}

async fn fixture() -> Fixture {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let seed = seed(&app.pool).await;
    let task = new_task(&app.pool, &seed, "Fix backups").await.id;
    Fixture { app, cookie, task }
}

impl Fixture {
    fn agent(&self) -> Principal {
        Principal::agent(df_core::PersonaId::new(), "Linus", None, self.task)
    }

    fn from<'a>(&self, actor: &'a Principal) -> df_db::requests::Raise<'a> {
        df_db::requests::Raise {
            task_id: self.task,
            run_id: None,
            actor,
            blocking: false,
        }
    }

    async fn ask(&self, question: &str, suggested: &[&str]) -> RequestId {
        let agent = self.agent();
        let suggested: Vec<String> = suggested.iter().map(ToString::to_string).collect();
        raise_question(&self.app.pool, self.from(&agent), question, &suggested)
            .await
            .unwrap()
            .0
            .id
    }

    async fn approval(&self, reason: &str) -> RequestId {
        let agent = self.agent();
        raise_approval(
            &self.app.pool,
            &TestApp::approval_types(),
            self.from(&agent),
            "fake-approval",
            json!({"what": reason}),
            reason,
        )
        .await
        .unwrap()
        .0
        .id
    }

    async fn page(&self, uri: &str) -> String {
        text(self.app.get(uri, &self.cookie).await).await
    }

    async fn post(&self, uri: &str, body: &str) -> (StatusCode, String) {
        let response = self.app.post(uri, body, &self.cookie).await;
        (response.status(), text(response).await)
    }

    async fn events(&self, uri: &str) -> SseReader {
        SseReader::new(self.app.get(uri, &self.cookie).await)
    }
}

#[tokio::test]
async fn inbox_lists_pending_requests_with_their_task() {
    let f = fixture().await;
    f.ask("Use Postgres 16 or 17?", &["16", "17"]).await;
    let page = f.page("/requests").await;
    assert!(
        page.contains("Use Postgres 16 or 17?")
            && page.contains("DF-1")
            && page.contains("Fix backups"),
        "{page}"
    );
    assert!(page.contains("hx-post=\"/requests/"), "{page}");
}

#[tokio::test]
async fn the_time_closes_the_card_below_the_actions() {
    let f = fixture().await;
    f.ask("Use Postgres 16 or 17?", &["16", "17"]).await;
    let page = f.page("/requests").await;
    let actions = page.find("hx-post=\"/requests/").expect("answer actions");
    let time = page.find("<time").expect("raised time");
    let card_end = page[time..].find("</article>").expect("card end") + time;
    assert!(actions < time, "{page}");
    assert!(
        page[time..card_end].trim_end().ends_with("</time>"),
        "{page}"
    );
}

#[tokio::test]
async fn empty_inbox_says_nothing_waits() {
    let f = fixture().await;
    assert!(f.page("/requests").await.contains("Nothing waits for you"));
}

#[tokio::test]
async fn agent_text_is_shown_as_text() {
    let f = fixture().await;
    f.approval("<script>alert(1)</script>").await;
    f.ask("<img src=x onerror=alert(1)>", &[]).await;
    let page = f.page("/requests").await;
    assert!(
        !page.contains("<script>alert") && !page.contains("<img src=x"),
        "{page}"
    );
    assert!(
        page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
        "{page}"
    );
}

#[tokio::test]
async fn a_new_request_appears_in_the_open_inbox_without_reload() {
    let f = fixture().await;
    let mut sse = f.events("/requests/events?list=1").await;
    let first = sse.next().await;
    assert!(
        first.data.contains("Nothing waits for you"),
        "{}",
        first.data
    );
    f.ask("Should I also update the docs?", &[]).await;
    let update = sse.next_within(LIVE).await;
    assert!(
        update.data.contains("Should I also update the docs?"),
        "{}",
        update.data
    );
}

#[tokio::test]
async fn the_navigation_count_follows_the_decisions() {
    let f = fixture().await;
    let first = f.ask("one?", &[]).await;
    f.ask("two?", &[]).await;
    f.ask("three?", &[]).await;
    let mut sse = f.events("/requests/events").await;
    let initial = sse.next().await;
    assert!(initial.data.contains(">3<span"), "{}", initial.data);
    assert!(
        initial.data.contains("request-count-rail") && initial.data.contains("request-count-bar")
    );
    let (status, _) = f
        .post(
            &format!("/requests/{first}/answer"),
            &form(&[("answer", "yes")]),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let update = sse.next_within(LIVE).await;
    assert!(update.data.contains(">2<span"), "{}", update.data);
}

#[tokio::test]
async fn one_tap_on_a_suggested_answer_answers_the_question() {
    let f = fixture().await;
    let id = f.ask("Postgres?", &["16", "17"]).await;
    let page = f.page("/requests").await;
    // The button carries the answer; tapping it posts it without another step.
    assert!(
        page.contains("hx-vals=\"{&quot;answer&quot;:&quot;17&quot;}\""),
        "{page}"
    );
    let (status, card) = f
        .post(
            &format!("/requests/{id}/answer"),
            &form(&[("answer", "17")]),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(card.contains("Answered by Peter: 17"), "{card}");
    assert!(
        !card.contains("Your answer"),
        "no actions after the decision: {card}"
    );
}

#[tokio::test]
async fn card_inputs_use_the_shared_input_class() {
    let f = fixture().await;
    f.ask("Postgres?", &[]).await;
    f.approval("needs it").await;
    let page = f.page("/requests").await;
    for name in ["name=\"answer\"", "name=\"comment\""] {
        let tag = crate::testkit::tag_around(&page, name);
        assert!(tag.contains(crate::components::forms::INPUT_CLASS), "{tag}");
    }
}

#[tokio::test]
async fn approving_offers_the_scopes_narrowest_first_and_rejects_others() {
    let f = fixture().await;
    let id = f.approval("needs it").await;
    let page = f.page("/requests").await;
    let task = page
        .find("value=\"task\" checked")
        .expect("narrowest preselected");
    assert!(task < page.find("value=\"project\"").unwrap(), "{page}");
    assert!(!page.contains("value=\"instance\""));

    let (status, card) = f
        .post(
            &format!("/requests/{id}/approve"),
            &form(&[("scope", "instance")]),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        card.contains("does not offer scope") && card.contains("Approve"),
        "{card}"
    );

    let (status, card) = f
        .post(
            &format!("/requests/{id}/approve"),
            &form(&[("scope", "project")]),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        card.contains("Approved by Peter for scope project"),
        "{card}"
    );

    let (status, card) = f.post(&format!("/requests/{id}/deny"), &form(&[])).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(card.contains("already decided"), "{card}");
}

#[tokio::test]
async fn denying_keeps_the_comment() {
    let f = fixture().await;
    let id = f.approval("needs it").await;
    let (status, card) = f
        .post(
            &format!("/requests/{id}/deny"),
            &form(&[("comment", "not now")]),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(card.contains("Denied by Peter: not now"), "{card}");
}

/// htmx 4 does not inherit `hx-target`/`hx-swap` from the form, so each button names its own.
#[tokio::test]
async fn each_approval_button_replaces_its_card() {
    let f = fixture().await;
    let id = f.approval("needs it").await;
    let page = f.page("/requests").await;
    for action in ["approve", "deny"] {
        assert!(
            page.contains(&format!(
                "hx-post=\"/requests/{id}/{action}\" hx-target=\"#request-{id}\" hx-swap=\"outerHTML\""
            )),
            "{action}: {page}"
        );
    }
}

#[tokio::test]
async fn unknown_request_is_not_found() {
    let f = fixture().await;
    let (status, _) = f
        .post(
            &format!("/requests/{}/answer", RequestId::new()),
            &form(&[("answer", "x")]),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn decided_request_stays_in_the_task_timeline() {
    let f = fixture().await;
    let id = f.ask("Postgres?", &["16"]).await;
    f.post(
        &format!("/requests/{id}/answer"),
        &form(&[("answer", "16")]),
    )
    .await;
    let page = f.page("/tasks/DF-1").await;
    assert!(page.contains(&format!("id=\"request-{id}\"")), "{page}");
    assert!(
        page.contains("Postgres?") && page.contains("Answered by Peter: 16"),
        "{page}"
    );
}

#[tokio::test]
async fn a_new_card_is_appended_after_the_events_streamed_before_it() {
    let f = fixture().await;
    let run = insert_run(&f.app.pool, f.task, 1, "running").await;
    let page = f.page("/tasks/DF-1").await;
    let mut sse = f.events(&task_stream_url(&page)).await;
    let _regions = sse.next().await;
    let event = add_event(
        &f.app.pool,
        f.task,
        run,
        1,
        "t1",
        json!({"userMessage": {"promptId": "p", "content": [{"text": {"text": "go"}}]}}),
    )
    .await;
    let item = sse.next_within(LIVE).await;
    assert!(item.data.contains(&format!("ev-{event}")), "{}", item.data);
    let id = f.ask("Postgres?", &["16"]).await;
    let mut card = sse.next_within(LIVE).await;
    while !card.data.contains(&format!("request-{id}")) {
        card = sse.next_within(LIVE).await;
    }
    let delete = card
        .data
        .find(&format!("hx-target=\"#request-{id}\" hx-swap=\"delete\""))
        .expect("delete partial");
    let append = card
        .data
        .find(r##"hx-target="#transcript" hx-swap="beforeend""##)
        .expect("append partial");
    let markup = card
        .data
        .find(&format!("id=\"request-{id}\""))
        .expect("card markup");
    assert!(delete < append && append < markup, "{}", card.data);
    f.post(
        &format!("/requests/{id}/answer"),
        &form(&[("answer", "16")]),
    )
    .await;
    let mut changed = sse.next_within(LIVE).await;
    while !changed.data.contains("Answered by") {
        changed = sse.next_within(LIVE).await;
    }
    // Sent once, a card changes in place: it does not jump to the end.
    assert!(
        changed.data.contains(&format!(
            "hx-target=\"#request-{id}\" hx-swap=\"outerMorph\""
        )),
        "{}",
        changed.data
    );
    assert!(
        !changed.data.contains("hx-swap=\"delete\""),
        "{}",
        changed.data
    );
}

/// A transcript longer than one batch with a question raised between two events. Returns the
/// question, the first event after it and the last event.
async fn long_transcript_with_a_card(f: &Fixture) -> (RequestId, i64, i64) {
    let run = insert_run(&f.app.pool, f.task, 1, "running").await;
    let card_seq = crate::sse::BATCH + 10;
    let (mut id, mut first_after_card, mut last) = (None, 0, 0);
    for seq in 1..=crate::sse::BATCH + 20 {
        if seq == card_seq {
            id = Some(f.ask("Early question?", &[]).await);
        }
        last = add_event(
            &f.app.pool,
            f.task,
            run,
            seq,
            "t1",
            json!({"userMessage": {"promptId": format!("p{seq}"), "content": [{"text": {"text": "go"}}]}}),
        )
        .await;
        if seq == card_seq {
            first_after_card = last;
        }
    }
    (id.unwrap(), first_after_card, last)
}

/// Reads frames until the card arrives and asserts it goes before `next_event`.
async fn assert_card_placed_before(sse: &mut SseReader, id: RequestId, next_event: i64) {
    let mut frame = sse.next().await;
    while !frame.data.contains(&format!("request-{id}")) {
        frame = sse.next().await;
    }
    assert!(
        frame.data.contains(&format!(
            "hx-target=\"#ev-{next_event}\" hx-swap=\"beforebegin\""
        )),
        "{}",
        frame.data
    );
}

#[tokio::test]
async fn a_card_is_placed_by_time_in_a_transcript_longer_than_one_batch() {
    let f = fixture().await;
    let (id, first_after_card, last) = long_transcript_with_a_card(&f).await;
    let mut sse = f.events(&format!("/tasks/DF-1/events?after={last}")).await;
    assert_card_placed_before(&mut sse, id, first_after_card).await;
}

#[tokio::test]
async fn a_first_connect_places_a_card_in_a_transcript_longer_than_one_batch() {
    let f = fixture().await;
    let (id, first_after_card, _) = long_transcript_with_a_card(&f).await;
    let mut sse = f.events("/tasks/DF-1/events?after=0").await;
    assert_card_placed_before(&mut sse, id, first_after_card).await;
}

#[tokio::test]
async fn a_card_is_placed_by_time_among_the_items_when_the_stream_starts() {
    let f = fixture().await;
    let run = insert_run(&f.app.pool, f.task, 1, "running").await;
    let say = |seq: i64| {
        add_event(
            &f.app.pool,
            f.task,
            run,
            seq,
            "t1",
            json!({"userMessage": {"promptId": format!("p{seq}"), "content": [{"text": {"text": "go"}}]}}),
        )
    };
    let before = say(1).await;
    let id = f.ask("Postgres?", &[]).await;
    let after = say(2).await;
    // A connection that resumes after both events (a reconnect, or a page rendered right after
    // the raise) puts the card back between them.
    let mut sse = f.events(&format!("/tasks/DF-1/events?after={after}")).await;
    let mut frame = sse.next().await;
    while !frame.data.contains(&format!("request-{id}")) {
        frame = sse.next().await;
    }
    assert!(
        frame.data.contains(&format!(
            "hx-target=\"#ev-{after}\" hx-swap=\"beforebegin\""
        )),
        "{}",
        frame.data
    );
    assert!(
        !frame.data.contains(&format!("ev-{before}")),
        "{}",
        frame.data
    );
    assert!(
        !frame.data.contains("hx-swap=\"beforeend\""),
        "{}",
        frame.data
    );
}

#[tokio::test]
async fn a_reconnect_shows_a_card_raised_and_decided_meanwhile_exactly_once() {
    let f = fixture().await;
    let page = f.page("/tasks/DF-1").await;
    let url = task_stream_url(&page);
    let mut sse = f.events(&url).await;
    let _regions = sse.next().await;
    let id = f.ask("Postgres?", &["16"]).await;
    let first = sse.next_within(LIVE).await;
    assert!(first.data.contains("Your answer"), "{}", first.data);
    drop(sse);
    f.post(
        &format!("/requests/{id}/answer"),
        &form(&[("answer", "16")]),
    )
    .await;
    let mut sse = f.events(&url).await;
    let frame = sse.next().await;
    assert_eq!(
        frame.data.matches(&format!("id=\"request-{id}\"")).count(),
        1,
        "{}",
        frame.data
    );
    assert!(frame.data.contains("Answered by"), "{}", frame.data);
    assert!(!frame.data.contains("Your answer"), "{}", frame.data);
    assert!(
        frame
            .data
            .contains(&format!("hx-target=\"#request-{id}\" hx-swap=\"delete\"")),
        "{}",
        frame.data
    );
}

#[tokio::test]
async fn a_card_decided_after_the_page_was_rendered_is_sent_when_the_stream_starts() {
    let f = fixture().await;
    let id = f.ask("Postgres?", &["16"]).await;
    // The page shows the pending card.
    let page = f.page("/tasks/DF-1").await;
    let url = task_stream_url(&page);
    assert!(page.contains("Your answer"));
    f.post(
        &format!("/requests/{id}/answer"),
        &form(&[("answer", "16")]),
    )
    .await;
    let mut sse = f.events(&url).await;
    let frame = sse.next().await;
    assert!(
        frame.data.contains(&format!("#request-{id}")) && frame.data.contains("Answered by"),
        "{}",
        frame.data
    );
    assert!(!frame.data.contains("Your answer"), "{}", frame.data);
}

#[tokio::test]
async fn a_card_raised_after_the_page_was_rendered_appears_when_the_stream_starts() {
    let f = fixture().await;
    let page = f.page("/tasks/DF-1").await;
    let url = task_stream_url(&page);
    let id = f.ask("Raised in between?", &[]).await;
    let mut sse = f.events(&url).await;
    let frame = sse.next().await;
    assert!(
        frame.data.contains("hx-swap=\"beforeend\"")
            && frame.data.contains(&format!("request-{id}")),
        "{}",
        frame.data
    );
}

#[tokio::test]
async fn images_in_agent_text_are_not_rendered() {
    let f = fixture().await;
    f.ask(
        "See ![x](https://evil.test/p.png) and [a link](https://evil.test/)",
        &[],
    )
    .await;
    let page = f.page("/requests").await;
    assert!(!page.contains("<img"), "{page}");
    assert!(
        page.contains("rel=\"noopener noreferrer nofollow\""),
        "{page}"
    );
}

#[test]
fn cards_merge_into_the_transcript_by_time() {
    use chrono::{TimeZone, Utc};
    let at = |m| Utc.with_ymd_and_hms(2026, 1, 1, 10, m, 0).unwrap();
    let item = |id: &str, m| crate::transcript::Item::test_agent(id, at(m));
    let items = [item("a", 0), item("b", 10)];
    let cards = [
        (at(5), maud::html! { div id="card" {} }),
        (at(30), maud::html! { div id="late" {} }),
    ];
    let html = crate::transcript::render_transcript(&items, &cards).into_string();
    let pos = |id: &str| html.find(&format!("id=\"{id}\"")).unwrap();
    assert!(
        pos("a") < pos("card") && pos("card") < pos("b") && pos("b") < pos("late"),
        "{html}"
    );
}

/// The task stream URL of a rendered task page.
fn task_stream_url(page: &str) -> String {
    page.split("hx-sse:connect=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .find(|url| url.contains("/events?after="))
        .expect("task stream url")
        .replace("&amp;", "&")
}
