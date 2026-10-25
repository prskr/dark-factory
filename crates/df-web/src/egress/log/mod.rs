//! `/egress/log`: the live view of every connection attempt through the egress proxy, as a
//! connection graph and an events table with filters and a details panel.

use std::convert::Infallible;

use axum::Extension;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::layout::{Nav, layout};

mod details;
mod filter;
mod graph;
mod page;
mod rows;
mod viewer;

pub(super) use filter::Filter;
#[cfg(test)]
pub(super) use filter::VerdictFilter;
#[cfg(test)]
pub(super) use viewer::MAX_OPEN_ROWS;
pub(super) use viewer::Viewer;

/// Events sent to the browser in one message while a backlog is being replayed.
const CHUNK: usize = 200;

pub async fn page(State(state): State<WebState>, Query(filter): Query<Filter>) -> Response {
    if state.flows.is_none() {
        return crate::auth::error_page(
            StatusCode::SERVICE_UNAVAILABLE,
            "Network traffic unavailable",
            "The gateway pods cannot be discovered: set `gateway.headlessService`.",
        );
    }
    // The persona filter is a convenience: without the list it offers only "All personas".
    let personas = df_db::catalog::list_personas(&state.pool)
        .await
        .inspect_err(|error| tracing::warn!(%error, "listing personas for the traffic filter"))
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.id.to_string(), p.name))
        .collect::<Vec<_>>();
    layout(
        "Network traffic",
        Nav::EgressLog,
        page::page_body(&filter, &personas),
    )
    .into_response()
}

/// `GET /egress/log/events`: the viewer's rows, graph and counts as `<hx-partial>` messages.
pub async fn events(
    State(state): State<WebState>,
    Extension(user): Extension<CurrentUser>,
    Query(filter): Query<Filter>,
) -> Response {
    let Some(flows) = &state.flows else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    // The subscription is not filtered: the graph and the counts need every event.
    let mut source = flows.subscribe();
    // The source sends the backlog, a `backlog_end` marker, then live events. The backlog is the
    // first message and replaces the table body: after a reconnect the page already shows rows.
    let stream = futures::stream::once(async move {
        let mut backlog = Vec::new();
        while let Some(event) = source.next().await {
            if event.backlog_end {
                break;
            }
            backlog.push(event);
        }
        let mut viewer = Viewer::new(filter);
        let first = viewer.backlog_message(&backlog);
        let live = source.ready_chunks(CHUNK).filter_map(move |batch| {
            let body = viewer.live_message(&batch);
            async move { body.map(|body| Ok::<_, Infallible>(Event::default().data(body))) }
        });
        futures::stream::once(std::future::ready(Ok::<_, Infallible>(
            Event::default().data(first),
        )))
        .chain(live)
    })
    .flatten();
    // Ends when the session's token is due for refresh, like the other streams: the reconnect
    // passes the auth middleware again.
    Sse::new(stream.take_until(tokio::time::sleep(user.until_refresh())))
        .keep_alive(KeepAlive::default())
        .into_response()
}
