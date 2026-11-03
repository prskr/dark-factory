//! What the allow and deny actions of the traffic page answer with: a toast in the page's
//! `#allow-toast` that links to the rule, and the link itself.

use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};

use crate::components::{ICON_BUTTON_CLASS, Icon, icon};

/// Link to the rules page with the rows of `target` (`host:port`, `host:any` for a deny rule)
/// highlighted.
pub(super) fn highlight_href(target: &str) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("highlight", target);
    format!("/egress/allowlist?{}", query.finish())
}

/// `body` as the content of the page's toast.
pub(super) fn into_toast(status: StatusCode, body: Markup) -> Response {
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    headers.insert("HX-Retarget", HeaderValue::from_static("#allow-toast"));
    headers.insert("HX-Reswap", HeaderValue::from_static("innerHTML"));
    response
}

/// The toast of a created rule: `title`, what it means, and a link to the rule.
pub(super) fn created(title: &str, detail: Markup, rule_href: &str) -> Markup {
    html! {
        div x-data="{ open: true }" x-show="open" class="flex w-80 items-start gap-3 rounded-xl border border-line bg-panel p-4 text-sm shadow-lg" {
            span aria-hidden="true" class="mt-0.5 text-status-completed-fg" { (icon(Icon::Check)) }
            div class="min-w-0 flex-1" {
                p class="font-semibold" { (title) }
                p class="mt-0.5 text-muted" { (detail) }
                a href=(rule_href) class="mt-1 inline-block text-accent-text hover:underline" { "View rule" }
            }
            button type="button" aria-label="Dismiss" "x-on:click"="open = false" class=(ICON_BUTTON_CLASS) { (icon(Icon::Close)) }
        }
    }
}
