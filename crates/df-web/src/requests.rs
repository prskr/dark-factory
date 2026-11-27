//! Agent requests in the UI: the `/requests` inbox, request cards (shared with the task
//! timeline), decisions and the live pending count (`specs/request-inbox/spec.md`).
//!
//! Everything an agent wrote (question text, reasons, payload values) is untrusted: it is
//! escaped, Markdown renders without raw HTML.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Form, Router};
use df_core::{RequestId, TaskId};
use df_db::Notification;
use df_db::create_issue::CREATE_ISSUE;
use df_db::network_access::{NETWORK_ACCESS, deny_always as deny_host_always};
use df_db::push_checks::{ExceptionFinding, PUSH_CHECK_EXCEPTION, PushCheck, request_findings};
use df_db::requests::{
    ApprovalTypes, Decision, RequestError, RequestKind, RequestRow, RequestState, Scope,
};
use futures::StreamExt;
use maud::{Markup, html};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::forms::INPUT_CLASS;
use crate::components::short_time;
use crate::components::{ButtonKind, EmptyPanel, EmptySize, action_class, page_header, tone_badge};
use crate::components::{Tone, chip};
use crate::layout::{Nav, Shell, layout};
use crate::markdown;
use crate::responses::{internal_error, not_found};

mod create_issue;
mod question;

use question::{Rejection, Submitted};

/// Safety net against lost notifications.
const POLL: Duration = Duration::from_secs(5);
/// Settling time before the page is re-rendered after a change.
const DEBOUNCE: Duration = Duration::from_millis(100);

pub(crate) fn routes() -> Router<WebState> {
    Router::new()
        .route("/requests", get(inbox))
        .route("/requests/events", get(events))
        .route("/requests/{id}/answer", post(answer))
        .route("/requests/{id}/approve", post(approve))
        .route("/requests/{id}/deny", post(deny))
        .route("/requests/{id}/deny-always", post(deny_always))
}

/// A request with what its card shows besides the row.
#[derive(Debug, Clone)]
pub(crate) struct CardData {
    pub row: RequestRow,
    pub task_key: String,
    pub task_title: String,
    /// What an approval offers, narrowest first; empty for questions.
    pub scopes: Vec<Scope>,
    /// What the approval type says the request is about.
    pub summary: String,
}

impl CardData {
    pub(crate) fn new(
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
            scopes: approval_type.map_or_default(|t| t.scopes().to_vec()),
            summary: approval_type.map_or_default(|t| t.describe(&row)),
            row,
            task_key,
            task_title,
        }
    }
}

/// Cards of the requests of a task (the timeline).
pub(crate) async fn task_cards(
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
pub(crate) fn card_id(id: RequestId) -> String {
    format!("request-{id}")
}

/// The pending-count badge of the navigation entry `id`; empty at zero.
pub(crate) fn count_badge(id: &str, count: i64) -> Markup {
    html! {
        span id=(id) aria-live="polite"
            class={"absolute -right-1 -top-1 min-w-5 rounded-full bg-accent px-1 text-center text-xs font-semibold text-on-accent" @if count == 0 { " hidden" }} {
            @if count > 0 { (count) span class="sr-only" { " pending requests" } }
        }
    }
}

/// The pending requests, or the empty state.
pub(crate) fn request_list(cards: &[CardData]) -> Markup {
    html! {
        div id="request-list" class="mx-auto max-w-3xl space-y-3" {
            @if cards.is_empty() {
                div data-empty-state { (EmptyPanel {
                    size: EmptySize::Page,
                    icon: None,
                    title: "Nothing waits for you",
                    text: "Questions and approval requests of the agents show up here.",
                    phone_hint: None,
                    action: html! {},
                }.render()) }
            }
            @for card in cards { (request_card(card, None)) }
        }
    }
}

/// One request: what it asks, its state and, while pending, the ways to decide it. `error`
/// is the reason the last decision failed.
pub(crate) fn request_card(card: &CardData, error: Option<&str>) -> Markup {
    request_card_with(card, error, None)
}

/// [`request_card`] for a question whose last answer the server refused: the form shows
/// `rejection` again.
fn request_card_with(
    card: &CardData,
    error: Option<&str>,
    rejection: Option<&Rejection>,
) -> Markup {
    let row = &card.row;
    let id = card_id(row.id);
    let target = format!("#{id}");
    let pending = row.state.is_pending();
    let is_issue = row.approval_type.as_deref() == Some(CREATE_ISSUE);
    let kind_label = match row.kind {
        RequestKind::Question => format!("Question · {}", row.question().kind.label()),
        RequestKind::Approval => row.approval_type.clone().unwrap_or_default(),
    };
    html! {
        article id=(id) data-state=(row.state.as_str())
            class="min-w-0 space-y-3 rounded-xl border border-line-strong bg-sunk p-4" {
            div class="flex flex-wrap items-center gap-2 text-xs text-faint" {
                (chip(&kind_label))
                @if row.blocking && pending { (tone_badge(Tone::Waiting, "Blocking")) }
                @if !pending { (tone_badge(state_tone(row.state), row.state.as_str())) }
                a href=(crate::layout::item_path(&card.task_key)) class="font-medium text-accent-text hover:underline" { (card.task_key) }
                span class="min-w-0 truncate" { (card.task_title) }
            }
            @if is_issue {
                (create_issue::body(card, &id))
            } @else {
                div id={(id) "-question"} class="md-body md-request min-w-0 break-words text-sm [&_p]:my-1" {
                    @match row.kind {
                        RequestKind::Question => (markdown::render(row.question_text())),
                        RequestKind::Approval => {
                            @if !card.summary.is_empty() { p class="font-medium" { (card.summary) } }
                            @if !row.reason.is_empty() { (markdown::render(&row.reason)) }
                        }
                    }
                }
            }
            @if row.approval_type.as_deref() == Some(PUSH_CHECK_EXCEPTION) {
                (finding_list(&request_findings(&row.payload)))
            }
            @if pending && row.kind == RequestKind::Question {
                (question::input(row, &id, rejection))
            } @else if pending {
                (actions(card, &target))
            } @else if is_issue {
                p class="text-sm text-muted" { (create_issue::outcome(row)) }
            } @else if row.state == RequestState::Answered {
                (question::answered(row))
            } @else {
                p class="text-sm text-muted" { (outcome(row)) }
            }
            @if let Some(error) = error {
                p role="alert" class="text-sm text-danger" { (error) }
            }
            time class="block text-[11px] text-faint" datetime=(row.created_at.to_string()) { (short_time(row.created_at)) }
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

async fn inbox(State(state): State<WebState>, shell: Shell) -> Response {
    let cards = match pending_cards(&state).await {
        Ok(cards) => cards,
        Err(e) => return internal_error(&e),
    };
    layout(
        "Requests",
        Nav::Requests,
        &shell,
        html! {
            (page_header("Requests", None, html! {}))
            (updates_sink())
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

/// The approve button: it names the scope when the type offers only one.
const fn approve_label(scopes: &[Scope]) -> &'static str {
    match scopes {
        [Scope::Run] => "Approve for this run",
        [Scope::Task] => "Approve for this task",
        [Scope::Project] => "Approve for this project",
        [Scope::Instance] => "Approve everywhere",
        _ => "Approve",
    }
}

const fn check_label(check: PushCheck) -> &'static str {
    match check {
        PushCheck::SecretScan => "Secret scan",
        PushCheck::Identity => "Identity",
        PushCheck::ProtectedPath => "Protected path",
        PushCheck::BlobTooLarge => "Size limit",
        PushCheck::MergeCommit => "Merge commit",
        PushCheck::HiddenPath => "Hidden path",
        PushCheck::Signature => "Signature",
    }
}

/// The findings of a `push-check-exception` request, one row each: check, path and line, the
/// rule of a secret and the masked match or blob size. Desktop: one line per row; phone: check
/// and rule above path and preview.
fn finding_list(findings: &[ExceptionFinding]) -> Markup {
    html! {
        ul class="divide-y divide-line rounded-lg border border-line bg-panel" {
            @for finding in findings {
                li class="flex flex-col items-start gap-1 px-3 py-2 lg:flex-row lg:items-center lg:gap-2" {
                    div class="flex items-center gap-2 lg:contents" {
                        span class="shrink-0 rounded-md bg-sunk px-2 py-px text-xs text-muted" { (check_label(finding.check)) }
                        @if let Some(rule) = &finding.rule {
                            span class="text-xs text-faint lg:order-1" { (rule) }
                        }
                    }
                    div class="flex min-w-0 max-w-full items-center gap-2 lg:contents" {
                        span class="min-w-0 break-all font-mono text-[13px] text-ink" {
                            (finding.path)
                            @if let Some(line) = finding.line { ":" (line) }
                        }
                        @if !finding.preview.is_empty() {
                            span class="shrink-0 font-mono text-xs text-muted lg:order-2 lg:ml-auto" { (finding.preview) }
                        }
                    }
                }
            }
        }
    }
}

fn actions(card: &CardData, target: &str) -> Markup {
    let row = &card.row;
    let post = |action: &str| format!("/requests/{}/{action}", row.id);
    let button_class = action_class(ButtonKind::Secondary);
    match row.kind {
        // A question has its own form (`question::input`).
        RequestKind::Question => html! {},
        RequestKind::Approval => html! {
            form class="space-y-3" {
                @if let [only] = card.scopes.as_slice() {
                    input type="hidden" name="scope" value=(only.as_str());
                } @else if !card.scopes.is_empty() {
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
                @let placeholder = if row.approval_type.as_deref() == Some(CREATE_ISSUE) {
                    create_issue::COMMENT_PLACEHOLDER
                } else {
                    "Comment (optional)"
                };
                input type="text" name="comment" maxlength="4000" autocomplete="off"
                    aria-label=(placeholder) placeholder=(placeholder)
                    class=(INPUT_CLASS);
                div class="flex flex-wrap gap-2" {
                    // htmx 4 does not inherit target and swap from the form.
                    button type="submit" hx-post=(post("approve")) hx-target=(target) hx-swap="outerHTML"
                        class=(action_class(ButtonKind::Primary)) { (approve_label(&card.scopes)) }
                    button type="submit" hx-post=(post("deny")) hx-target=(target) hx-swap="outerHTML"
                        class=(button_class) { "Deny" }
                    // Denies the request and the exact host for good (a deny rule).
                    @if row.approval_type.as_deref() == Some(NETWORK_ACCESS) {
                        button type="submit" hx-post=(post("deny-always")) hx-target=(target) hx-swap="outerHTML"
                            class=(button_class) { "Deny always" }
                    }
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
        // An answered question has its own view (`question::answered`).
        RequestState::Answered => format!("Answered{by}"),
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
    // `selected` repeats: only a list of pairs reads it.
    Form(pairs): Form<Vec<(String, String)>>,
) -> Response {
    let Ok(request_id) = id.parse::<RequestId>() else {
        return not_found("The request");
    };
    let row = match df_db::requests::get_request(&state.pool, request_id).await {
        Ok(Some(row)) => row,
        Ok(None) => return not_found("The request"),
        Err(e) => return internal_error(&e),
    };
    let submitted = Submitted::from_form(&pairs);
    let answer = submitted.answer(row.question().kind);
    let result = df_db::requests::decide(
        &state.pool,
        &state.approval_types,
        request_id,
        &user.principal(),
        &user.roles,
        Decision::Answer(answer),
    )
    .await;
    let rejection = match &result {
        Err(RequestError::InvalidAnswer(error)) => Some(Rejection {
            submitted,
            error: error.clone(),
        }),
        _ => None,
    };
    respond(&state, request_id, result, rejection.as_ref()).await
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

/// "Deny always": denies a `network-access` request and creates a deny rule for its exact host.
async fn deny_always(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<String>,
    Form(form): Form<DenyForm>,
) -> Response {
    let Ok(request_id) = id.parse::<RequestId>() else {
        return not_found("The request");
    };
    let comment = Some(form.comment.as_str()).filter(|c| !c.trim().is_empty());
    let result = deny_host_always(
        &state.pool,
        request_id,
        &user.principal(),
        &user.roles,
        comment,
    )
    .await;
    respond(&state, request_id, result, None).await
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
    respond(&state, request_id, result, None).await
}

/// The card after a decision attempt. A failed one answers with the card and the reason; a
/// refused answer shows `rejection` under its input instead of the card-wide line.
async fn respond(
    state: &WebState,
    request_id: RequestId,
    result: Result<RequestRow, RequestError>,
    rejection: Option<&Rejection>,
) -> Response {
    let (status, error) = match &result {
        Ok(_) => (StatusCode::OK, None),
        Err(RequestError::InvalidAnswer(_)) if rejection.is_some() => {
            (StatusCode::UNPROCESSABLE_ENTITY, None)
        }
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
            | RequestError::Refused(_)
            | RequestError::InvalidAnswer(_)
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
    (
        status,
        request_card_with(&card, error.as_deref(), rejection),
    )
        .into_response()
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
        /// The markup of every card sent by this connection.
        sent: BTreeMap<RequestId, String>,
        first_list: bool,
    }
    let stream = Stream {
        rx: state.notifier.subscribe(),
        state,
        list: query.list.is_some(),
        first: true,
        last_count: None,
        sent: BTreeMap::new(),
        first_list: true,
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
                partials.extend(list_updates(&mut s.sent, &cards, s.first_list));
                s.first_list = false;
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

/// The hidden sink the live card updates are appended to; `requestCards.ts` applies them.
pub(crate) fn updates_sink() -> Markup {
    html! { div id="request-updates" hidden x-data="requestCards" {} }
}

/// A card for the sink: replaces the card with its id, else goes before `before` (a DOM id), else
/// to the end of `into`.
pub(crate) fn upsert_partial(card: &Markup, before: Option<&str>, into: &str) -> Markup {
    html! {
        hx-partial hx-target="#request-updates" hx-swap="beforeend" {
            template data-request-card data-before=[before] data-into=(into) { (card) }
        }
    }
}

/// The removal of the card `card_id` for the sink.
fn delete_partial(card_id: &str) -> Markup {
    html! {
        hx-partial hx-target="#request-updates" hx-swap="beforeend" {
            template data-request-delete=(card_id) {}
        }
    }
}

/// The pending cards of the inbox list for the sink: it removes every other card of the list.
fn keep_partial(cards: &[CardData]) -> Markup {
    let ids = cards
        .iter()
        .map(|c| card_id(c.row.id))
        .collect::<Vec<_>>()
        .join(" ");
    html! {
        hx-partial hx-target="#request-updates" hx-swap="beforeend" {
            template data-request-keep=(ids) data-into="#request-list" {}
        }
    }
}

/// What the inbox list needs to match `cards` after `sent` (the cards this connection sent): a
/// delete for each card that left, an upsert for each new or changed card, and the whole list
/// when the last card left or the first send finds none (the empty state). The first send also
/// names the pending cards, so cards decided while the page was offline leave. Unchanged cards
/// send nothing, so a draft in the form of a card is never touched.
fn list_updates(
    sent: &mut BTreeMap<RequestId, String>,
    cards: &[CardData],
    first: bool,
) -> Vec<Markup> {
    if cards.is_empty() {
        let had_cards = !sent.is_empty();
        sent.clear();
        return if had_cards || first {
            vec![partial("#request-list", &request_list(cards))]
        } else {
            Vec::new()
        };
    }
    let mut updates: Vec<Markup> = sent
        .keys()
        .filter(|id| cards.iter().all(|c| c.row.id != **id))
        .map(|id| delete_partial(&card_id(*id)))
        .collect();
    if first {
        // A reconnect: the page may still show cards decided while it was offline.
        updates.push(keep_partial(cards));
    }
    sent.retain(|id, _| cards.iter().any(|c| c.row.id == *id));
    // Last first: each card's anchor is then on the page or sent before it.
    for (i, card) in cards.iter().enumerate().rev() {
        let markup = request_card(card, None);
        if sent.get(&card.row.id) == Some(&markup.0) {
            continue;
        }
        sent.insert(card.row.id, markup.0.clone());
        let before = cards.get(i + 1).map(|next| card_id(next.row.id));
        updates.push(upsert_partial(&markup, before.as_deref(), "#request-list"));
    }
    updates
}

#[cfg(test)]
mod tests;
