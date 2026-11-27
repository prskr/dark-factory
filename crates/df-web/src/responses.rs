//! Responses shared by the handlers: htmx redirects, invalid forms, error pages.
//!
//! Error answers go through [`crate::auth::error_page`], directly or through [`not_found`] and
//! [`internal_error`]: for an htmx request, `htmx::error_answers` turns that page into a toast
//! and leaves the targeted region unchanged. An error answer built any other way swaps text or
//! nothing into its target. Toasts of successful actions go through
//! [`crate::components::toast_answer`].

use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use maud::Markup;

/// Response telling htmx to navigate to `location` (used after a successful form post).
pub(crate) fn htmx_redirect(location: &str) -> Response {
    let mut response = StatusCode::OK.into_response();
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert("HX-Redirect", value);
    }
    response
}

/// Response telling htmx to navigate to `location` like a boosted link, without a full page load
/// (a created entity's page after a successful form post).
pub(crate) fn htmx_location(location: &str) -> Response {
    let mut response = StatusCode::OK.into_response();
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert("HX-Location", value);
    }
    response
}

/// An HTML fragment that htmx swaps although the request failed validation (htmx 4 swaps
/// every status except 204 and 304).
pub(crate) fn invalid(fragment: Markup) -> Response {
    (StatusCode::UNPROCESSABLE_ENTITY, fragment).into_response()
}

/// A page-level 404.
pub(crate) fn not_found(what: &str) -> Response {
    crate::auth::error_page(
        StatusCode::NOT_FOUND,
        "Not found",
        &format!("{what} does not exist."),
    )
}

/// Maps an unexpected database error to an error page and logs it.
pub(crate) fn internal_error(error: &dyn std::fmt::Display) -> Response {
    tracing::error!(%error, "request failed");
    crate::auth::error_page(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Error",
        "Something went wrong. Try again.",
    )
}
