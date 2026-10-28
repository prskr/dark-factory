//! Agent requests in the UI: the `/requests` inbox, request cards (shared with the task
//! timeline), decisions and the live pending count (`specs/request-inbox/spec.md`).
//!
//! Everything an agent wrote (question text, reasons, payload values) is untrusted: it is
//! escaped, Markdown renders without raw HTML.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Form, Router};
use chrono::{DateTime, Utc};
use df_core::{RequestId, TaskId};
use df_db::Notification;
use df_db::requests::{
    ApprovalTypes, Decision, RequestError, RequestKind, RequestRow, RequestState, Scope,
};
use futures::StreamExt;
use maud::{Markup, html};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::{ButtonKind, EmptyPanel, EmptySize, action_class, page_header, tone_badge};
use crate::components::{Tone, chip};
use crate::layout::{Nav, layout};
use crate::responses::{internal_error, not_found};
use crate::transcript::markdown;

/// Safety net against lost notifications.
const POLL: Duration = Duration::from_secs(5);
/// Settling time before the page is re-rendered after a change.
const DEBOUNCE: Duration = Duration::from_millis(100);

pub fn routes() -> Router<WebState> {
    Router::new()
        .route("/requests", get(inbox))
        .route("/requests/events", get(events))
        .route("/requests/{id}/answer", post(answer))
        .route("/requests/{id}/approve", post(approve))
        .route("/requests/{id}/deny", post(deny))
}

/// A request with what its card shows besides the row.
#[derive(Debug, Clone)]
pub struct CardData {
    pub row: RequestRow,
    pub task_key: String,
    pub task_title: String,
    /// What an approval offers, narrowest first; empty for questions.
    pub scopes: Vec<Scope>,
    /// What the approval type says the request is about.
    pub summary: String,
}

impl CardData {
    pub fn new(
        types: &ApprovalTypes,
        row: RequestRow,
        task_key: String,
        task_title: String,
    ) -> Self {
        let approval_type = row
            .approval_type
            .as_deref()
            .and_then(|name| types.get(name));
        Self {
            scopes: approval_type
                .map(|t| t.scopes().to_vec())
                .unwrap_or_default(),
            summary: approval_type.map(|t| t.describe(&row)).unwrap_or_default(),
            row,
            task_key,
            task_title,
        }
    }
}

/// Cards of the requests of a task (the timeline).
pub async fn task_cards(
    state: &WebState,
    task_id: TaskId,
    key: &str,
    title: &str,
) -> Result<Vec<CardData>, df_db::DbError> {
    Ok(df_db::requests::list_for_task(&state.pool, task_id)
        .await?
        .into_iter()
        .map(|row| {
            CardData::new(
                &state.approval_types,
                row,
                key.to_string(),
                title.to_string(),
            )
        })
        .collect())
}

/// The card's DOM id.
pub fn card_id(id: RequestId) -> String {
    format!("request-{id}")
}

/// The pending-count badge of the navigation entry `id`; empty at zero.
pub fn count_badge(id: &str, count: i64) -> Markup {
    html! {
        span id=(id) aria-live="polite"
            class={"absolute -right-1 -top-1 min-w-5 rounded-full bg-accent px-1 text-center text-xs font-semibold text-on-accent" @if count == 0 { " hidden" }} {
            @if count > 0 { (count) span class="sr-only" { " pending requests" } }
        }
    }
}

/// The pending requests, or the empty state.
pub fn request_list(cards: &[CardData]) -> Markup {
    html! {
        div id="request-list" class="mx-auto max-w-3xl space-y-3" {
            @if cards.is_empty() {
                (EmptyPanel {
                    size: EmptySize::Page,
                    icon: None,
                    title: "Nothing waits for you",
                    text: "Questions and approval requests of the agents show up here.",
                    phone_hint: None,
                    action: html! {},
                }.render())
            }
            @for card in cards { (request_card(card, None)) }
        }
    }
}

/// One request: what it asks, its state and, while pending, the ways to decide it. `error`
/// is the reason the last decision failed.
pub fn request_card(card: &CardData, error: Option<&str>) -> Markup {
    let row = &card.row;
    let id = card_id(row.id);
    let target = format!("#{id}");
    let pending = row.state == RequestState::Pending;
    let kind_label = match row.kind {
        RequestKind::Question => "Question".to_string(),
        RequestKind::Approval => row.approval_type.clone().unwrap_or_default(),
    };
    html! {
        article id=(id) data-state=(row.state.as_str())
            class="min-w-0 space-y-3 rounded-xl border border-line bg-panel p-4" {
            div class="flex flex-wrap items-center gap-2 text-xs text-faint" {
                (chip(&kind_label))
                @if row.blocking && pending { (tone_badge(Tone::Waiting, "Blocking")) }
                @if !pending { (tone_badge(state_tone(row.state), row.state.as_str())) }
                a href={"/tasks/" (card.task_key)} class="font-medium text-accent-text hover:underline" { (card.task_key) }
                span class="min-w-0 truncate" { (card.task_title) }
            }
            div class="min-w-0 break-words text-sm [&_p]:my-1" {
                @match row.kind {
                    RequestKind::Question => (markdown(row.question_text())),
                    RequestKind::Approval => {
                        @if !card.summary.is_empty() { p class="font-medium" { (card.summary) } }
                        @if !row.reason.is_empty() { (markdown(&row.reason)) }
                    }
                }
            }
            @if pending {
                (actions(card, &target))
            } @else {
                p class="text-sm text-muted" { (outcome(row)) }
            }
            @if let Some(error) = error {
                p role="alert" class="text-sm text-danger" { (error) }
            }
            time class="block text-[11px] text-faint" datetime=(row.created_at.to_rfc3339()) { (clock(row.created_at)) }
        }
    }
}

async fn pending_cards(state: &WebState) -> Result<Vec<CardData>, df_db::DbError> {
    Ok(df_db::requests::list_pending(&state.pool)
        .await?
        .into_iter()
        .map(|(row, key, title)| CardData::new(&state.approval_types, row, key, title))
        .collect())
}

async fn inbox(State(state): State<WebState>) -> Response {
    let cards = match pending_cards(&state).await {
        Ok(cards) => cards,
        Err(e) => return internal_error(&e),
    };
    layout(
        "Requests",
        Nav::Requests,
        html! {
            (page_header("Requests", None, html! {}))
            (request_list(&cards))
        },
    )
    .into_response()
}

const fn state_tone(state: RequestState) -> Tone {
    match state {
        RequestState::Answered | RequestState::Approved => Tone::Completed,
        RequestState::Denied => Tone::Failed,
        RequestState::Closed | RequestState::Pending => Tone::Queued,
    }
}

const fn scope_label(scope: Scope) -> &'static str {
    match scope {
        Scope::Run => "This run",
        Scope::Task => "This task",
        Scope::Project => "This project",
        Scope::Instance => "Everywhere",
    }
}

fn actions(card: &CardData, target: &str) -> Markup {
    let row = &card.row;
    let post = |action: &str| format!("/requests/{}/{action}", row.id);
    let button_class = action_class(ButtonKind::Secondary);
    match row.kind {
        RequestKind::Question => html! {
            @let suggested = row.suggested_answers();
            @if !suggested.is_empty() {
                div class="flex flex-wrap gap-2" {
                    @for answer in suggested {
                        button type="button" hx-post=(post("answer")) hx-target=(target) hx-swap="outerHTML"
                            hx-vals=(serde_json::json!({ "answer": answer }).to_string())
                            class=(button_class) { (answer) }
                    }
                }
            }
            form hx-post=(post("answer")) hx-target=(target) hx-swap="outerHTML" class="flex gap-2" {
                input type="text" name="answer" required maxlength="4000" autocomplete="off"
                    aria-label="Your answer" placeholder="Your answer"
                    class="min-h-11 min-w-0 flex-1 rounded-lg border border-line-strong bg-panel px-3 text-sm lg:min-h-9";
                button type="submit" class=(action_class(ButtonKind::Primary)) { "Answer" }
            }
        },
        RequestKind::Approval => html! {
            form class="space-y-3" {
                @if !card.scopes.is_empty() {
                    fieldset class="flex flex-wrap gap-x-4 gap-y-1" {
                        legend class="sr-only" { "Scope" }
                        @for (i, scope) in card.scopes.iter().enumerate() {
                            label class="flex min-h-11 items-center gap-2 text-sm lg:min-h-9" {
                                input type="radio" name="scope" value=(scope.as_str()) checked[i == 0];
                                (scope_label(*scope))
                            }
                        }
                    }
                }
                input type="text" name="comment" maxlength="4000" autocomplete="off"
                    aria-label="Comment (optional)" placeholder="Comment (optional)"
                    class="min-h-11 w-full rounded-lg border border-line-strong bg-panel px-3 text-sm lg:min-h-9";
                div class="flex flex-wrap gap-2" {
                    // htmx 4 does not inherit target and swap from the form.
                    button type="submit" hx-post=(post("approve")) hx-target=(target) hx-swap="outerHTML"
                        class=(action_class(ButtonKind::Primary)) { "Approve" }
                    button type="submit" hx-post=(post("deny")) hx-target=(target) hx-swap="outerHTML"
                        class=(button_class) { "Deny" }
                }
            }
        },
    }
}

/// What was decided, by whom and when.
fn outcome(row: &RequestRow) -> String {
    let by = match &row.decided_by {
        Some(df_core::Principal::Human { display_name, .. }) => format!(" by {display_name}"),
        _ => String::new(),
    };
    match row.state {
        RequestState::Answered => format!("Answered{by}: {}", row.answer().unwrap_or_default()),
        RequestState::Approved => format!(
            "Approved{by} for scope {}",
            row.scope().map_or("-", |s| s.as_str())
        ),
        RequestState::Denied => match row.comment() {
            Some(comment) => format!("Denied{by}: {comment}"),
            None => format!("Denied{by}"),
        },
        RequestState::Closed => format!("Closed: {}", row.close_reason.as_deref().unwrap_or("-")),
        RequestState::Pending => String::new(),
    }
}

fn clock(at: DateTime<Utc>) -> String {
    at.format("%b %d %H:%M").to_string()
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AnswerForm {
    answer: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ApproveForm {
    scope: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DenyForm {
    comment: String,
}

#[derive(Debug, Default, Deserialize)]
struct EventsQuery {
    /// Also stream the list of the inbox page.
    list: Option<String>,
}

async fn answer(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    Path(id): Path<String>,
    Form(form): Form<AnswerForm>,
) -> Response {
    decide(state.0, user.0, &id, Ok(Decision::Answer(form.answer))).await
}

async fn approve(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    Path(id): Path<String>,
    Form(form): Form<ApproveForm>,
) -> Response {
    let scope = form.scope.parse::<Scope>().map(Decision::Approve);
    decide(state.0, user.0, &id, scope).await
}

async fn deny(
    state: State<WebState>,
    user: Extension<CurrentUser>,
    Path(id): Path<String>,
    Form(form): Form<DenyForm>,
) -> Response {
    let comment = Some(form.comment).filter(|c| !c.trim().is_empty());
    decide(state.0, user.0, &id, Ok(Decision::Deny(comment))).await
}

/// Stores the decision and answers with the card as it is now. A failed decision answers with
/// the card and the reason (htmx swaps 409 and 422 too).
async fn decide(
    state: WebState,
    user: CurrentUser,
    id: &str,
    decision: Result<Decision, RequestError>,
) -> Response {
    let Ok(request_id) = id.parse::<RequestId>() else {
        return not_found("The request");
    };
    let result = match decision {
        Ok(decision) => {
            df_db::requests::decide(
                &state.pool,
                &state.approval_types,
                request_id,
                &user.principal(),
                &user.roles,
                decision,
            )
            .await
        }
        Err(e) => Err(e),
    };
    let (status, error) = match &result {
        Ok(_) => (StatusCode::OK, None),
        Err(RequestError::NotFound) => return not_found("The request"),
        Err(RequestError::AlreadyDecided) => (
            StatusCode::CONFLICT,
            Some(RequestError::AlreadyDecided.to_string()),
        ),
        Err(
            e @ (RequestError::Invalid(_)
            | RequestError::ScopeNotOffered(_)
            | RequestError::ApplyFailed(_)
            | RequestError::UnknownType(_)
            | RequestError::TooManyPending),
        ) => (StatusCode::UNPROCESSABLE_ENTITY, Some(e.to_string())),
        Err(RequestError::Db(e)) => return internal_error(e),
    };
    let row = match result {
        Ok(row) => row,
        Err(_) => match df_db::requests::get_request(&state.pool, request_id).await {
            Ok(Some(row)) => row,
            Ok(None) => return not_found("The request"),
            Err(e) => return internal_error(&e),
        },
    };
    let task = match df_db::get_task(&state.pool, row.task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => return not_found("The task"),
        Err(e) => return internal_error(&e),
    };
    let card = CardData::new(&state.approval_types, row, task.key.to_string(), task.title);
    (status, request_card(&card, error.as_deref())).into_response()
}

/// `GET /requests/events`: the pending count of the navigation, and with `?list=1` the inbox
/// list, whenever a request changes.
async fn events(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<EventsQuery>,
) -> Response {
    struct Stream {
        state: WebState,
        rx: broadcast::Receiver<Notification>,
        list: bool,
        first: bool,
        last_count: Option<i64>,
        last_list: String,
    }
    let stream = Stream {
        rx: state.notifier.subscribe(),
        state,
        list: query.list.is_some(),
        first: true,
        last_count: None,
        last_list: String::new(),
    };
    let events = futures::stream::unfold(stream, |mut s| async move {
        loop {
            if !std::mem::take(&mut s.first) {
                match tokio::time::timeout(POLL, s.rx.recv()).await {
                    Ok(Err(broadcast::error::RecvError::Closed)) => return None,
                    Ok(Ok(Notification::RequestChanged { .. } | Notification::Reconnected)) => {
                        tokio::time::sleep(DEBOUNCE).await;
                    }
                    Ok(Ok(_)) => continue,
                    Ok(Err(broadcast::error::RecvError::Lagged(_))) | Err(_) => {}
                }
            }
            let (cards, count) = match load(&s.state, s.list).await {
                Ok(loaded) => loaded,
                Err(error) => {
                    tracing::warn!(%error, "request event stream failed");
                    return None;
                }
            };
            let mut partials = Vec::new();
            if s.last_count != Some(count) {
                s.last_count = Some(count);
                for id in ["request-count-rail", "request-count-bar"] {
                    partials.push(partial(&format!("#{id}"), &count_badge(id, count)));
                }
            }
            if s.list {
                let list = request_list(&cards).into_string();
                if list != s.last_list {
                    partials.push(partial("#request-list", &maud::PreEscaped(list.clone())));
                    s.last_list = list;
                }
            }
            if !partials.is_empty() {
                let body = html! { @for p in &partials { (p) } }.into_string();
                return Some((Ok::<_, Infallible>(Event::default().data(body)), s));
            }
        }
    });
    Sse::new(events.take_until(tokio::time::sleep(user.until_refresh())))
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn load(state: &WebState, list: bool) -> Result<(Vec<CardData>, i64), df_db::DbError> {
    if list {
        let cards = pending_cards(state).await?;
        let count = i64::try_from(cards.len()).unwrap_or(i64::MAX);
        Ok((cards, count))
    } else {
        Ok((
            Vec::new(),
            df_db::requests::pending_count(&state.pool).await?,
        ))
    }
}

fn partial(target: &str, inner: &Markup) -> Markup {
    html! { hx-partial hx-target=(target) hx-swap="outerMorph" { (inner) } }
}

#[cfg(test)]
mod tests;
