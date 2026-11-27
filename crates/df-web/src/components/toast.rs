//! Toasts: the frame, the error toast and the answer that puts one into the page's toast region
//! (`Region::Toasts`, rendered by `layout`). The region is the live region; a toast has no live
//! role of its own, so screen readers announce it once.

use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use maud::{Markup, html};

use super::{ICON_BUTTON_CLASS, Icon, icon};
use crate::htmx::{Region, partial};

/// What a toast reports; it picks the icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastTone {
    Success,
    Error,
}

impl ToastTone {
    const fn icon(self) -> (Icon, &'static str) {
        match self {
            Self::Success => (Icon::Check, "text-status-completed-fg"),
            Self::Error => (Icon::CircleAlert, "text-danger"),
        }
    }
}

/// The toast frame around `body`: tone icon, the content and a "Dismiss" button.
pub fn toast(tone: ToastTone, body: Markup) -> Markup {
    let (glyph, color) = tone.icon();
    html! {
        div x-data="{ open: true }" x-show="open" class="flex w-auto items-start gap-3 rounded-xl border border-line bg-panel p-4 text-sm shadow-lg md:w-80" {
            span aria-hidden="true" class={"mt-0.5 " (color)} { (icon(glyph)) }
            div class="min-w-0 flex-1" { (body) }
            button type="button" aria-label="Dismiss" "x-on:click"="open = false" class=(ICON_BUTTON_CLASS) { (icon(Icon::Close)) }
        }
    }
}

/// A toast of the error tone: `title` (for example "404 Not found") and what `message` says.
pub fn error_toast(title: &str, message: &str) -> Markup {
    toast(
        ToastTone::Error,
        html! {
            p class="font-semibold" { (title) }
            p class="mt-0.5 text-muted" { (message) }
        },
    )
}

/// The answer that shows `body` (a [`toast`]) in the toast region and changes nothing else: one
/// `<hx-partial>` for `Region::Toasts`, and `HX-Reswap: none` so the request's own target stays.
pub fn toast_answer(status: StatusCode, body: Markup) -> Response {
    let partial = partial(Region::Toasts.selector(), "innerHTML", body);
    let mut response = (status, partial).into_response();
    response
        .headers_mut()
        .insert("HX-Reswap", HeaderValue::from_static("none"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toast_has_text_tone_icon_and_a_dismiss_button() {
        let html = error_toast("404 Not found", "The task does not exist.").into_string();

        assert!(html.contains("404 Not found"), "{html}");
        assert!(html.contains("The task does not exist."), "{html}");
        assert!(html.contains("text-danger"), "{html}");
        assert!(html.contains("aria-label=\"Dismiss\""), "{html}");
        assert!(!html.contains("role="), "no live role on the toast: {html}");
        let success = toast(ToastTone::Success, html! { "done" }).into_string();
        assert!(success.contains("text-status-completed-fg"), "{success}");
        assert!(success.contains("aria-label=\"Dismiss\""), "{success}");
    }

    #[tokio::test]
    async fn a_toast_answer_is_one_partial_for_the_toast_region_that_reswaps_nothing() {
        let response = toast_answer(StatusCode::NOT_FOUND, error_toast("404 Not found", "Gone."));

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()["hx-reswap"], "none");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        crate::testkit::assert_toast(&body);
    }
}
