//! Responses shared by the handlers: htmx redirects, invalid forms, error pages.

use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use maud::Markup;

/// Response telling htmx to navigate to `location` (used after a successful form post).
pub fn htmx_redirect(location: &str) -> Response {
    let mut response = StatusCode::OK.into_response();
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert("HX-Redirect", value);
    }
    response
}

/// An HTML fragment that htmx swaps although the request failed validation (htmx 4 swaps
/// every status except 204 and 304).
pub fn invalid(fragment: Markup) -> Response {
    (StatusCode::UNPROCESSABLE_ENTITY, fragment).into_response()
}

/// A page-level 404.
pub fn not_found(what: &str) -> Response {
    crate::auth::error_page(
        StatusCode::NOT_FOUND,
        "Not found",
        &format!("{what} does not exist."),
    )
}

/// Maps an unexpected database error to an error page and logs it.
pub fn internal_error(error: &dyn std::fmt::Display) -> Response {
    tracing::error!(%error, "request failed");
    crate::auth::error_page(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Error",
        "Something went wrong. Try again.",
    )
}
