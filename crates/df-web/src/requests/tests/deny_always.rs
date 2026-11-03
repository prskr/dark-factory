//! "Deny always" on `network-access` cards.

use axum::http::StatusCode;
use df_core::RequestId;
use df_db::EntryKind;
use df_db::requests::{Raised, RequestState, get_request, raise_approval};
use serde_json::json;

use super::{Fixture, fixture};
use crate::testkit::{TestApp, form};

const DATADOG: &str = "http-intake.logs.us5.datadoghq.com";

impl Fixture {
    async fn network_access(&self, host: &str) -> RequestId {
        let agent = self.agent();
        let raised = raise_approval(
            &self.app.pool,
            &TestApp::approval_types(),
            self.from(&agent),
            "network-access",
            json!({ "host": host, "port": 443 }),
            "telemetry",
        )
        .await
        .unwrap();
        let Raised::Request { request, .. } = raised else {
            panic!("not stored")
        };
        request.id
    }
}

#[tokio::test]
async fn deny_always_denies_the_request_and_creates_an_exact_host_rule() {
    let f = fixture().await;
    let id = f.network_access(DATADOG).await;
    let other = f.network_access("other.datadoghq.com").await;

    let (status, card) = f
        .post(
            &format!("/requests/{id}/deny-always"),
            &form(&[("comment", "telemetry")]),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(card.contains("Denied by Peter: telemetry"), "{card}");
    let rules = df_db::list_entries(&f.app.pool).await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(
        rules[0].kind,
        EntryKind::HttpDeny {
            host: DATADOG.into()
        }
    );
    let decided = get_request(&f.app.pool, id).await.unwrap().unwrap();
    assert_eq!(decided.state, RequestState::Denied);
    let messages: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM task_messages WHERE author->>'reason' = 'request.decided'",
    )
    .fetch_one(&f.app.pool)
    .await
    .unwrap();
    assert_eq!(messages, 1);
    let untouched = get_request(&f.app.pool, other).await.unwrap().unwrap();
    assert_eq!(
        untouched.state,
        RequestState::Pending,
        "not *.datadoghq.com"
    );
}

#[tokio::test]
async fn only_network_access_cards_offer_deny_always() {
    let f = fixture().await;
    let network = f.network_access(DATADOG).await;
    let fake = f.approval("needs it").await;

    let page = f.page("/requests").await;

    assert!(
        page.contains(&format!("hx-post=\"/requests/{network}/deny-always\"")),
        "{page}"
    );
    assert!(page.contains(&format!("hx-target=\"#request-{network}\"")));
    assert!(
        !page.contains(&format!("/requests/{fake}/deny-always")),
        "{page}"
    );
    assert_eq!(page.matches(">Deny always<").count(), 1);
}

#[tokio::test]
async fn deny_always_on_another_type_or_a_decided_request_is_rejected() {
    let f = fixture().await;
    let fake = f.approval("needs it").await;
    let network = f.network_access(DATADOG).await;

    let (status, _) = f.post(&format!("/requests/{fake}/deny-always"), "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    f.post(&format!("/requests/{network}/deny"), "").await;
    let (status, _) = f
        .post(&format!("/requests/{network}/deny-always"), "")
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    assert!(df_db::list_entries(&f.app.pool).await.unwrap().is_empty());
    let still = get_request(&f.app.pool, fake).await.unwrap().unwrap();
    assert_eq!(still.state, RequestState::Pending);
}

#[tokio::test]
async fn deny_always_needs_the_board_member_role() {
    let f = fixture().await;
    let id = f.network_access(DATADOG).await;
    let (cookie, _) = crate::auth::testing::seed_session(&f.app.pool, &[], 3600, Some("r")).await;

    let response = f
        .app
        .post(&format!("/requests/{id}/deny-always"), "", &cookie)
        .await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(df_db::list_entries(&f.app.pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn the_phone_layout_offers_deny_always_in_the_same_card() {
    // Scenario "Phone": the card has no separate phone markup, so the button is in it at any width.
    let f = fixture().await;
    f.network_access(DATADOG).await;

    let page = f.page("/requests").await;

    let card = &page[page.find("<article").unwrap()..];
    let actions = &card[card.find(">Approve<").unwrap()..];
    assert!(actions.contains(">Deny always<"), "{actions}");
    assert!(
        page.contains("flex flex-wrap gap-2"),
        "the buttons wrap on narrow screens"
    );
}
