//! The `http.request` span of the HTTP listeners (`specs/telemetry/spec.md`, "Span coverage").

use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;

/// Middleware that wraps every request in an `http.request` span (method, route, status). The
/// route is the matched pattern, so neither paths nor query strings reach the span.
pub async fn request_span(request: Request, next: Next) -> Response {
    use tracing::Instrument as _;

    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str)
        .to_string();
    let span = tracing::info_span!(
        "http.request",
        http.request.method = %request.method(),
        http.route = %route,
        http.response.status_code = tracing::field::Empty,
    );
    let response = next.run(request).instrument(span.clone()).await;
    span.record("http.response.status_code", response.status().as_u16());
    response
}
