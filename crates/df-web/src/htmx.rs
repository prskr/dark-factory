//! What the pages and htmx share: the element IDs several files must keep the same ([`Region`]),
//! the `<hx-partial>` wrapper ([`partial`]), request detection ([`is_htmx`]) and the middleware
//! that turns error pages into toasts for htmx requests ([`error_answers`]).

use axum::body::Body;
use axum::extract::Request;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use maud::{Markup, Render, html};

use crate::components::{error_toast, toast_answer};

/// Declares [`Region`]: each element ID once, with its literal. The enum, `ALL` and the two
/// `const fn`s come from the same list, so none can miss a variant.
macro_rules! regions {
    ($($variant:ident => $id:literal),+ $(,)?) => {
        /// An element ID that markup in one file sets and a selector in another file (or a
        /// script) names. Page-local IDs stay literals next to their `id=`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(crate) enum Region {
            $($variant),+
        }

        impl Region {
            /// Every region.
            #[cfg(test)]
            pub(crate) const ALL: &[Region] = &[$(Region::$variant),+];

            /// The value of the `id` attribute.
            pub(crate) const fn id(self) -> &'static str {
                match self {
                    $(Region::$variant => $id),+
                }
            }

            /// The CSS selector of the element: `#` and the ID.
            pub(crate) const fn selector(self) -> &'static str {
                match self {
                    $(Region::$variant => concat!("#", $id)),+
                }
            }
        }
    };
}

regions! {
    Toasts => "toasts",
    NetworkErrorToast => "network-error-toast",
    // The workspace of a task or chat: what live updates and actions replace.
    WsDetail => "ws-detail",
    WsDetailsHeading => "ws-details-heading",
    // The task list column; its opening and scoping controls swap it whole.
    TaskList => "task-list",
    TaskRows => "task-rows",
    TaskHead => "task-head",
    TaskDetails => "task-details",
    ChatRows => "chat-rows",
    ChatHead => "chat-head",
    ChatOverview => "chat-overview",
    Dependencies => "dependencies",
    PanelSubtasks => "panel-subtasks",
    Runs => "runs",
    Pending => "pending",
    ComposerAction => "composer-action",
    ComposerSlot => "composer-slot",
    Transcript => "transcript",
    // Settings pages.
    ConfigList => "config-list",
    Form => "form",
    ProjectEgress => "project-egress",
    ArchiveBody => "archive-body",
    // The traffic page, which its stream updates.
    CountAll => "count-all",
    CountBlocked => "count-blocked",
    CountAllowed => "count-allowed",
    EventCount => "event-count",
    Flows => "flows",
    FlowsEmpty => "flows-empty",
    Graph => "graph",
}

/// Wraps `inner` in a `<hx-partial>` that htmx 4 swaps into `target` with `swap`.
pub(crate) fn partial(target: &str, swap: &str, inner: impl Render) -> Markup {
    html! { hx-partial hx-target=(target) hx-swap=(swap) { (inner) } }
}

/// Whether the request comes from htmx (`HX-Request: true`).
pub(crate) fn is_htmx(headers: &HeaderMap) -> bool {
    headers
        .get("hx-request")
        .is_some_and(|v| v.as_bytes() == b"true")
}

/// Marks a response of [`crate::auth::error_page`]: the title and message that
/// [`error_answers`] shows as a toast to an htmx request.
#[derive(Debug, Clone)]
pub(crate) struct ErrorAnswer {
    pub(crate) title: String,
    pub(crate) message: String,
}

/// Middleware: answers an htmx request that got an error page with a toast instead. The status
/// and the headers of the inner response stay (a refreshed session cookie among them), only the
/// body changes. Full page loads, history restores and responses without [`ErrorAnswer`]
/// (streams, invalid forms, conflicts) pass through unchanged.
pub(crate) async fn error_answers(request: Request, next: Next) -> Response {
    let htmx = is_htmx(request.headers()) && !is_history_restore(request.headers());
    let response = next.run(request).await;
    if !htmx {
        return response;
    }
    let Some(answer) = response.extensions().get::<ErrorAnswer>().cloned() else {
        return response;
    };
    let (mut parts, _page) = response.into_parts();
    let title = format!("{} {}", parts.status.as_u16(), answer.title);
    let (toast, body) =
        toast_answer(parts.status, error_toast(&title, &answer.message)).into_parts();
    parts.headers.remove(CONTENT_LENGTH);
    parts.headers.extend(toast.headers);
    parts.headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    parts
        .headers
        .insert("HX-Push-Url", HeaderValue::from_static("false"));
    Response::from_parts(parts, Body::new(body))
}

/// A history restore (back, forward) fetches the page with `HX-Request` and swaps the whole body.
fn is_history_restore(headers: &HeaderMap) -> bool {
    headers
        .get("hx-history-restore-request")
        .is_some_and(|v| v.as_bytes() == b"true")
}

#[cfg(test)]
mod app_tests;
#[cfg(test)]
mod tests;
