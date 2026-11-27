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
use df_core::{RequestId, TaskId, TaskKind};
use df_db::Notification;
use df_db::task_views::StoredEvent;
use df_proto::transcript::v1::event;
use futures::StreamExt;
use maud::{Markup, html};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::WebState;
use crate::artifacts::details_heading;
use crate::auth::CurrentUser;
use crate::task_pages::{
    CancelNote, ListQuery, ResumeNote, composer_action, dependencies_region, details_region,
    head_region, interruptible, load_detail, pending_region, runs_region, subtasks_panel,
    task_rows,
};
use crate::transcript::{Change, Folder, ItemKind, render_group, render_group_head, render_item};

/// Chunks of one message that arrive within this window are sent as one update.
const COALESCE: Duration = Duration::from_millis(150);
/// Safety net against lost notifications.
const POLL: Duration = Duration::from_secs(5);
/// Settling time before the task list is re-rendered after a change.
const LIST_DEBOUNCE: Duration = Duration::from_millis(200);
pub(crate) const BATCH: i64 = 500;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct AfterQuery {
    after: Option<i64>,
}

/// `GET /tasks/{key}/events` and `GET /chats/{key}/events`: the transcript and the regions of the
/// page of a task or chat.
pub(crate) async fn task_events(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    section: crate::task_pages::Section,
    Path(key): Path<String>,
    headers: HeaderMap,
    Query(query): Query<AfterQuery>,
) -> Response {
    let task = match crate::task_pages::task_by_key(&state, section, &key).await {
        Ok(task) => task,
        Err(response) => return *response,
    };
    let start = last_event_id(&headers).or(query.after).unwrap_or(0).max(0);
    let folder = Folder::new(crate::task_pages::persona_label(&task))
        .with_hidden_blocks(state.show_hidden_blocks);
    let task_id = TaskId::from_uuid(task.id);
    let stream = DetailStream {
        task_id,
        kind: section.kind,
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
        regions: Vec::new(),
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
    /// A chat's page has other regions than a task's.
    kind: TaskKind,
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
    /// Re-render the regions at the next opportunity.
    check_regions: bool,
    /// Last sent markup of every region of [`Self::rendered_regions`], in its order.
    regions: Vec<String>,
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
        let mut partials = self.transcript_partials(&touched);
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

    /// The partials for the touched items (design D3). An item outside a tool call group is
    /// appended to the transcript or morphed. The first call of a streak appends the whole group.
    /// A later call goes `beforeend` into the items of its group and a changed member is morphed,
    /// and the head of every touched group is replaced once. The wrapper of a group is never
    /// morphed, so its open state survives.
    fn transcript_partials(&self, touched: &[Change]) -> Vec<Markup> {
        let items = self.folder.items();
        let mut partials = Vec::new();
        let mut created: Vec<&str> = Vec::new();
        let mut dirty: Vec<(&str, usize)> = Vec::new();
        for change in touched {
            let Some(item) = items.get(change.index) else {
                continue;
            };
            let Some(group) = item.group.as_deref() else {
                partials.push(item_partial(item, change.added, "#transcript"));
                continue;
            };
            if created.contains(&group) {
                continue;
            }
            let members = self.folder.group_members(change.index);
            if change.added && members.first().is_some_and(|m| m.dom_id == item.dom_id) {
                created.push(group);
                partials.push(partial("#transcript", "beforeend", &render_group(members)));
                continue;
            }
            partials.push(item_partial(item, change.added, &format!("#{group}-items")));
            if !dirty.iter().any(|(g, _)| *g == group) {
                dirty.push((group, change.index));
            }
        }
        for (group, index) in dirty {
            if !created.contains(&group) {
                let head = render_group_head(self.folder.group_members(index));
                partials.push(partial(&format!("#{group}-head"), "outerMorph", &head));
            }
        }
        partials
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
        // The cards show the title of the task: a rename sends them again (an upsert keeps drafts).
        if task.title != self.task_title {
            self.task_title.clone_from(&task.title);
            self.check_requests = true;
        }
        let mut data = load_detail(&self.state, task).await?;
        let artifacts = df_db::artifacts::list_artifacts(&self.state.pool, self.task_id)
            .await?
            .len();
        // A message the transcript already shows is delivered, whether or not `delivered_at` is
        // committed yet.
        data.pending.retain(|m| {
            let id = m.id.to_string();
            !self.folder.items().iter().any(
                |item| matches!(&item.kind, ItemKind::User { prompt_id, .. } if *prompt_id == id),
            )
        });
        let rendered = self.rendered_regions(&data, artifacts).await?;
        self.regions.resize(rendered.len(), String::new());
        let mut changed = Vec::new();
        for (slot, (id, markup)) in self.regions.iter_mut().zip(rendered) {
            let html = markup.0;
            if *slot != html {
                changed.push(partial(id, "outerMorph", &maud::PreEscaped(html.clone())));
                *slot = html;
            }
        }
        Ok(changed)
    }

    /// The regions of the page as they are now, with their target: head, details,
    /// dependencies, runs, pending list, composer action, the details heading (tabs) and the
    /// Subtasks tab of a task; of a chat the same without dependencies, and with the overview.
    async fn rendered_regions(
        &self,
        data: &crate::task_pages::DetailData,
        artifacts: usize,
    ) -> Result<Vec<(&'static str, Markup)>, df_db::DbError> {
        let task = &data.task;
        let runs = runs_region(&task.key, &data.runs, task.persona_model.as_deref());
        let pending = pending_region(
            &task.key,
            &task.persona_display_name,
            task.status.as_str(),
            &data.pending,
            self.state.show_hidden_blocks,
            None,
        );
        let composer = composer_action(&task.key, interruptible(&data.runs));
        let subtasks = subtasks_panel(&data.subtasks);
        if self.kind == TaskKind::Chat {
            let base = df_db::base_branch(&self.state.pool, self.task_id).await?;
            return Ok(vec![
                ("#chat-head", crate::chat_pages::head_region(task)),
                (
                    "#chat-overview",
                    crate::chat_pages::overview_region(task, &base, None),
                ),
                ("#runs", runs),
                ("#pending", pending),
                ("#composer-action", composer),
                (
                    "#ws-details-heading",
                    crate::artifacts::chat_details_heading(data.subtasks.len(), artifacts, false),
                ),
                ("#panel-subtasks", subtasks),
            ]);
        }
        Ok(vec![
            ("#task-head", head_region(task, None)),
            (
                "#task-details",
                details_region(
                    task,
                    &CancelNote::new(&data.runs, &data.pending),
                    &ResumeNote::new(task, &data.runs, self.state.max_auto_resumes),
                    None,
                ),
            ),
            (
                "#dependencies",
                dependencies_region(&data.dependencies, task.status),
            ),
            ("#runs", runs),
            ("#pending", pending),
            ("#composer-action", composer),
            (
                "#ws-details-heading",
                details_heading(data.subtasks.len(), artifacts, false),
            ),
            ("#panel-subtasks", subtasks),
        ])
    }

    /// The request cards that changed. The first send of a card by this connection removes the
    /// card (the page may show it already) and inserts it by time, as `render_transcript` does:
    /// before the first item it goes before (`Folder::card_anchor`; after the group for a card
    /// inside a tool call group), else at the end. That is idempotent, so a reconnect
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
            if self.requests.insert(card.row.id, html.clone()).as_ref() == Some(&html) {
                continue;
            }
            // An upsert for the first send too: the page may show the card already, and a draft
            // in its form stays (`requestCards.ts`).
            let anchor = self.folder.card_anchor(card.row.created_at);
            changed.push(crate::requests::upsert_partial(
                &markup,
                anchor.as_deref(),
                "#transcript",
            ));
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
pub(crate) async fn task_list_events(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<ListQuery>,
) -> Response {
    let filters = query.filters();
    list_events(
        state,
        &user,
        "#task-rows",
        |n| {
            matches!(
                n,
                Notification::TaskChanged { .. } | Notification::Reconnected
            )
        },
        move |state| {
            let filters = filters.clone();
            async move {
                let items = df_db::task_views::list_task_items(&state.pool, &filters.to_db())
                    .await
                    .inspect_err(|error| tracing::warn!(%error, "task list stream failed"))
                    .ok()?;
                Some(task_rows(&items, None, &filters).into_string())
            }
        },
    )
}

/// An SSE stream of a list: `render` makes the markup of the list on connect and after every
/// notification `relevant` accepts (a burst settles first), and the stream sends it as a morph of
/// `target` when it changed. `render` returns `None` to end the stream.
fn list_events<R, Fut>(
    state: WebState,
    user: &CurrentUser,
    target: &'static str,
    relevant: fn(&Notification) -> bool,
    render: R,
) -> Response
where
    R: Fn(WebState) -> Fut + Send + 'static,
    Fut: Future<Output = Option<String>> + Send,
{
    struct ListStream<R> {
        state: WebState,
        rx: broadcast::Receiver<Notification>,
        render: R,
        last: String,
        first: bool,
    }
    let stream = ListStream {
        rx: state.notifier.subscribe(),
        state,
        render,
        last: String::new(),
        first: true,
    };
    let events = futures::stream::unfold(stream, move |mut s| async move {
        loop {
            if !std::mem::take(&mut s.first) {
                // Wait for a change, then let a burst of changes settle.
                match tokio::time::timeout(POLL, s.rx.recv()).await {
                    Ok(Err(broadcast::error::RecvError::Closed)) => return None,
                    Ok(Ok(notification)) if relevant(&notification) => {
                        tokio::time::sleep(LIST_DEBOUNCE).await;
                    }
                    Ok(Ok(_)) => continue,
                    Ok(Err(broadcast::error::RecvError::Lagged(_))) | Err(_) => {}
                }
            }
            let html = (s.render)(s.state.clone()).await?;
            if html != s.last {
                let body =
                    partial(target, "outerMorph", &maud::PreEscaped(html.clone())).into_string();
                s.last = html;
                return Some((Ok::<_, Infallible>(Event::default().data(body)), s));
            }
        }
    });
    Sse::new(events.take_until(tokio::time::sleep(user.until_refresh())))
        .keep_alive(KeepAlive::default())
        .into_response()
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProjectQuery {
    project: Option<String>,
}

/// `GET /chats/events?project=KEY`: re-renders the chat list of the project when a chat changes,
/// so a new reply, a status or a request that needs you reorders it.
pub(crate) async fn chat_list_events(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<ProjectQuery>,
) -> Response {
    let project = query.project.unwrap_or_default();
    list_events(
        state,
        &user,
        "#chat-rows",
        |n| {
            matches!(
                n,
                Notification::TaskChanged { .. }
                    | Notification::RequestChanged { .. }
                    | Notification::TranscriptEvent { .. }
                    | Notification::Reconnected
            )
        },
        move |state| {
            let project = project.clone();
            async move {
                let (projects, chats) = tokio::try_join!(
                    df_db::chat_views::list_chat_projects(&state.pool),
                    df_db::chat_views::list_chats(&state.pool, &project, false, None)
                )
                .inspect_err(|error| tracing::warn!(%error, "chat list stream failed"))
                .ok()?;
                let project = projects.iter().find(|p| p.key == project)?;
                Some(crate::chat_pages::chat_rows(&chats, project, None).into_string())
            }
        },
    )
}

/// The partial of one item: a new item goes `beforeend` into `container`, a changed one replaces
/// itself in place.
fn item_partial(item: &crate::transcript::Item, added: bool, container: &str) -> Markup {
    let rendered = render_item(item);
    if added {
        partial(container, "beforeend", &rendered)
    } else {
        partial(&format!("#{}", item.dom_id), "outerMorph", &rendered)
    }
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
