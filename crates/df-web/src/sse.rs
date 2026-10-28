//! Server-sent events for the task list and the task detail page (design D4).
//!
//! Each message is a bundle of `<hx-partial>` elements that htmx 4 swaps into the page. Transcript
//! messages carry the global id of the last event they contain; on reconnect the client sends it as
//! `Last-Event-ID` (the spike in `README.md` shows htmx 4 does), and the first connect of a page
//! passes the id it was rendered with as `?after=`.
//!
//! Streams end when the session's token is due for refresh: the reconnect passes the auth
//! middleware again, so a removed role or an ended session also ends live updates.

use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;

use axum::Extension;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use df_core::{RequestId, TaskId};
use df_db::Notification;
use df_db::task_views::StoredEvent;
use df_proto::transcript::v1::event;
use futures::StreamExt;
use maud::{Markup, html};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::responses::{internal_error, not_found};
use crate::task_pages::{
    Filters, ListQuery, details_region, head_region, load_detail, pending_region, runs_region,
    task_rows,
};
use crate::transcript::{Change, Folder, ItemKind, card_goes_before, render_item};

/// Chunks of one message that arrive within this window are sent as one update.
const COALESCE: Duration = Duration::from_millis(150);
/// Safety net against lost notifications.
const POLL: Duration = Duration::from_secs(5);
/// Settling time before the task list is re-rendered after a change.
const LIST_DEBOUNCE: Duration = Duration::from_millis(200);
pub(crate) const BATCH: i64 = 500;

#[derive(Debug, Default, Deserialize)]
pub struct AfterQuery {
    after: Option<i64>,
}

/// `GET /tasks/{key}/events`.
pub async fn task_events(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Path(key): Path<String>,
    headers: HeaderMap,
    Query(query): Query<AfterQuery>,
) -> Response {
    let task = match df_db::task_views::get_task_detail_by_key(&state.pool, &key).await {
        Ok(Some(task)) => task,
        Ok(None) => return not_found("The task"),
        Err(e) => return internal_error(&e),
    };
    let start = last_event_id(&headers).or(query.after).unwrap_or(0).max(0);
    let folder =
        Folder::new(task.persona_name.clone()).with_hidden_blocks(state.show_hidden_blocks);
    let task_id = TaskId::from_uuid(task.id);
    let stream = DetailStream {
        task_id,
        task_key: task.key.clone(),
        task_title: task.title.clone(),
        requests: HashMap::new(),
        check_requests: true,
        rx: state.notifier.subscribe(),
        state,
        start,
        cursor: 0,
        folder,
        check_regions: true,
        regions: [String::new(), String::new(), String::new(), String::new()],
        settle: None,
    };
    let events = futures::stream::unfold(stream, |mut s| async move {
        let event = s.next_event().await?;
        Some((Ok::<_, Infallible>(event), s))
    });
    Sse::new(events.take_until(tokio::time::sleep(user.until_refresh())))
        .keep_alive(KeepAlive::default())
        .into_response()
}

struct DetailStream {
    state: WebState,
    task_id: TaskId,
    task_key: String,
    task_title: String,
    /// Last sent markup of every request card of the task; absent until the connection sent it.
    requests: HashMap<RequestId, String>,
    /// Re-render the request cards at the next opportunity.
    check_requests: bool,
    rx: broadcast::Receiver<Notification>,
    /// Events up to this id are folded silently (the client has them already).
    start: i64,
    /// Id of the last transcript row read.
    cursor: i64,
    folder: Folder,
    /// Re-render head, runs and pending list at the next opportunity.
    check_regions: bool,
    /// Last sent markup of head, details, runs and pending list.
    regions: [String; 4],
    /// Pause before the next read to coalesce chunks.
    settle: Option<Duration>,
}

impl DetailStream {
    /// The next message; `None` ends the stream.
    async fn next_event(&mut self) -> Option<Event> {
        loop {
            if let Some(pause) = self.settle.take() {
                tokio::time::sleep(pause).await;
            }
            match self.step().await {
                Ok(Some(event)) => return Some(event),
                Ok(None) => {
                    if !self.wait_for_change().await {
                        return None;
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, task = %self.task_id, "task event stream failed");
                    return None;
                }
            }
        }
    }

    /// Reads new rows and regions; `Some` if there is something to send.
    async fn step(&mut self) -> Result<Option<Event>, df_db::DbError> {
        let rows =
            df_db::task_views::transcript_after(&self.state.pool, self.task_id, self.cursor, BATCH)
                .await?;
        let full = i64::try_from(rows.len()).is_ok_and(|n| n >= BATCH);
        let (touched, streamed) = self.fold(&rows);
        let mut partials = Vec::new();
        for change in &touched {
            let Some(item) = self.folder.items().get(change.index) else {
                continue;
            };
            let rendered = render_item(item);
            partials.push(if change.added {
                partial("#transcript", "beforeend", &rendered)
            } else {
                partial(&format!("#{}", item.dom_id), "outerMorph", &rendered)
            });
        }
        if self.check_regions {
            self.check_regions = false;
            partials.extend(self.changed_regions().await?);
        }
        // Cards are placed among the items, so wait until the fold has caught up.
        if self.check_requests && !full {
            self.check_requests = false;
            partials.extend(self.changed_requests().await?);
        }
        if partials.is_empty() {
            return if full {
                Box::pin(self.step()).await
            } else {
                Ok(None)
            };
        }
        if streamed {
            self.settle = Some(COALESCE);
        }
        let mut event = Event::default().data(html! { @for p in &partials { (p) } }.into_string());
        if !touched.is_empty() {
            event = event.id(self.cursor.to_string());
        }
        Ok(Some(event))
    }

    /// Folds `rows`; returns the changes to send (deduplicated) and whether agent text streamed.
    fn fold(&mut self, rows: &[StoredEvent]) -> (Vec<Change>, bool) {
        let mut touched: Vec<Change> = Vec::new();
        let mut streamed = false;
        for row in rows {
            self.cursor = self.cursor.max(row.id);
            let changes = self.folder.apply(row);
            if row.id <= self.start {
                continue;
            }
            // The run's token usage changes with a turn end, which adds no transcript item.
            if matches!(
                row.event.as_ref().and_then(|e| e.kind.as_ref()),
                Some(event::Kind::TurnEnded(_))
            ) {
                self.check_regions = true;
            }
            for change in changes {
                match touched.iter_mut().find(|t| t.index == change.index) {
                    // Added in this batch: still added, now with its latest content.
                    Some(_) => {}
                    None => touched.push(change),
                }
                match self.folder.items().get(change.index).map(|item| &item.kind) {
                    Some(ItemKind::Agent {
                        complete: false, ..
                    }) => streamed = true,
                    Some(ItemKind::User { .. }) => {
                        self.check_regions = true;
                    }
                    _ => {}
                }
            }
        }
        (touched, streamed)
    }

    async fn changed_regions(&mut self) -> Result<Vec<Markup>, df_db::DbError> {
        let Some(task) = df_db::task_views::get_task_detail(&self.state.pool, self.task_id).await?
        else {
            return Ok(Vec::new());
        };
        let mut data = load_detail(&self.state, task).await?;
        // A message the transcript already shows is delivered, whether or not `delivered_at` is
        // committed yet.
        data.pending.retain(|m| {
            let id = m.id.to_string();
            !self.folder.items().iter().any(
                |item| matches!(&item.kind, ItemKind::User { prompt_id, .. } if *prompt_id == id),
            )
        });
        let rendered = [
            head_region(&data.task, None),
            details_region(&data.task),
            runs_region(&data.runs, data.task.persona_model.as_deref()),
            pending_region(
                &data.task.key,
                &data.task.persona_name,
                data.task.status.as_str(),
                &data.pending,
                self.state.show_hidden_blocks,
                None,
            ),
        ];
        let mut changed = Vec::new();
        for (slot, (id, markup)) in self.regions.iter_mut().zip(
            ["#task-head", "#task-details", "#runs", "#pending"]
                .into_iter()
                .zip(rendered),
        ) {
            let html = markup.0;
            if *slot != html {
                changed.push(partial(id, "outerMorph", &maud::PreEscaped(html.clone())));
                *slot = html;
            }
        }
        Ok(changed)
    }

    /// The request cards that changed. The first send of a card by this connection removes the
    /// card (the page may show it already) and inserts it by time, as `render_transcript` does:
    /// before the first item it goes before ([`card_goes_before`]), else at the end. That is idempotent, so a reconnect
    /// or a card the page raced with cannot duplicate or misplace it. It runs once the fold has
    /// caught up with the transcript (events up to `start` are folded silently), so every
    /// neighbour is known. Later changes morph the card in place.
    async fn changed_requests(&mut self) -> Result<Vec<Markup>, df_db::DbError> {
        let cards = crate::requests::task_cards(
            &self.state,
            self.task_id,
            &self.task_key,
            &self.task_title,
        )
        .await?;
        let mut changed = Vec::new();
        for card in &cards {
            let markup = crate::requests::request_card(card, None);
            let html = markup.0.clone();
            let target = format!("#{}", crate::requests::card_id(card.row.id));
            match self.requests.insert(card.row.id, html.clone()) {
                Some(sent) if sent == html => {}
                Some(_) => changed.push(partial(&target, "outerMorph", &markup)),
                None => {
                    changed.push(partial(&target, "delete", &Markup::default()));
                    changed.push(
                        match self
                            .folder
                            .items()
                            .iter()
                            .find(|item| card_goes_before(card.row.created_at, item.at))
                        {
                            Some(next) => {
                                partial(&format!("#{}", next.dom_id), "beforebegin", &markup)
                            }
                            None => partial("#transcript", "beforeend", &markup),
                        },
                    );
                }
            }
        }
        Ok(changed)
    }

    /// Waits for a notification about this task, the poll interval, or a lost connection.
    /// Returns `false` if the notifier is gone.
    async fn wait_for_change(&mut self) -> bool {
        loop {
            match tokio::time::timeout(POLL, self.rx.recv()).await {
                Err(_) => {
                    // Safety net: a lost task notification must not leave the regions stale.
                    self.check_regions = true;
                    self.check_requests = true;
                    return true;
                }
                Ok(Ok(notification)) => match notification {
                    Notification::TranscriptEvent { task_id, .. } if task_id == self.task_id => {
                        return true;
                    }
                    Notification::TaskChanged { task_id }
                    | Notification::RunChanged { task_id, .. }
                    | Notification::TaskMessageQueued { task_id }
                    | Notification::TaskMessageChanged { task_id }
                        if task_id == self.task_id =>
                    {
                        self.check_regions = true;
                        return true;
                    }
                    Notification::RequestChanged { task_id, .. } if task_id == self.task_id => {
                        self.check_requests = true;
                        return true;
                    }
                    Notification::Reconnected => {
                        self.check_regions = true;
                        self.check_requests = true;
                        return true;
                    }
                    _ => {}
                },
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                    self.check_regions = true;
                    self.check_requests = true;
                    return true;
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => return false,
            }
        }
    }
}

/// `GET /tasks/events?status=..&project=..`: re-renders the rows of the filtered list when a task
/// changes, so a task that no longer matches the filter leaves it.
pub async fn task_list_events(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<ListQuery>,
) -> Response {
    struct ListStream {
        state: WebState,
        rx: broadcast::Receiver<Notification>,
        filters: Filters,
        last: String,
        first: bool,
    }
    let stream = ListStream {
        rx: state.notifier.subscribe(),
        state,
        filters: query.filters(),
        last: String::new(),
        first: true,
    };
    let events = futures::stream::unfold(stream, |mut s| async move {
        loop {
            if !std::mem::take(&mut s.first) {
                // Wait for a change, then let a burst of changes settle.
                match tokio::time::timeout(POLL, s.rx.recv()).await {
                    Ok(Err(broadcast::error::RecvError::Closed)) => return None,
                    Ok(Ok(Notification::TaskChanged { .. } | Notification::Reconnected)) => {
                        tokio::time::sleep(LIST_DEBOUNCE).await;
                    }
                    Ok(Ok(_)) => continue,
                    Ok(Err(broadcast::error::RecvError::Lagged(_))) | Err(_) => {}
                }
            }
            let items =
                match df_db::task_views::list_task_items(&s.state.pool, &s.filters.to_db()).await {
                    Ok(items) => items,
                    Err(error) => {
                        tracing::warn!(%error, "task list stream failed");
                        return None;
                    }
                };
            let html = task_rows(&items, None, &s.filters).into_string();
            if html != s.last {
                let body = partial("#task-rows", "outerMorph", &maud::PreEscaped(html.clone()))
                    .into_string();
                s.last = html;
                return Some((Ok::<_, Infallible>(Event::default().data(body)), s));
            }
        }
    });
    Sse::new(events.take_until(tokio::time::sleep(user.until_refresh())))
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// Wraps `inner` in a partial that htmx swaps into `target`.
fn partial(target: &str, swap: &str, inner: &Markup) -> Markup {
    html! { hx-partial hx-target=(target) hx-swap=(swap) { (inner) } }
}

fn last_event_id(headers: &HeaderMap) -> Option<i64> {
    headers
        .get("last-event-id")?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests;
