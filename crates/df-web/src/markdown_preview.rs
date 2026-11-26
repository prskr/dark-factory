//! `POST /markdown/preview`: the server-rendered preview of a Markdown field
//! (`markdown-editor` spec, "Preview endpoint").
//!
//! The route is a pure function of its input: it stores nothing and never logs the text.

use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Form, Router};
use maud::{Markup, html};
use serde::Deserialize;

use crate::WebState;

/// The longest text the preview accepts, in UTF-8 bytes after decoding (the skill size limit).
pub(crate) const MAX_SOURCE_BYTES: usize = 256 * 1024;
/// The body limit: URL encoding makes a text up to three times larger (`#` becomes `%23`), so
/// every valid text fits; [`MAX_SOURCE_BYTES`] is checked on the decoded text.
const MAX_BODY_BYTES: usize = 1024 * 1024;

pub(crate) fn routes() -> Router<WebState> {
    Router::new().route(
        "/markdown/preview",
        post(preview).layer(DefaultBodyLimit::max(MAX_BODY_BYTES)),
    )
}

/// The preview of `source`: [`crate::markdown::render`] in the `md-body` wrapper, or "Nothing to
/// preview" for an empty text. The route and the first preview of a split field render the same.
pub(crate) fn fragment(source: &str) -> Markup {
    if source.trim().is_empty() {
        return html! { p class="text-sm text-faint" { "Nothing to preview" } };
    }
    html! { div class="md-body" { (crate::markdown::render(source)) } }
}

#[derive(Deserialize)]
struct PreviewForm {
    source: String,
}

async fn preview(Form(form): Form<PreviewForm>) -> Response {
    if form.source.len() > MAX_SOURCE_BYTES {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    fragment(&form.source).into_response()
}

// Needs a PostgreSQL container (testcontainers).
#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::MAX_SOURCE_BYTES;
    use crate::testkit::{TestApp, form, post_request, text};

    async fn preview(app: &TestApp, cookie: &str, source: &str) -> (StatusCode, String) {
        let response = app
            .post("/markdown/preview", &form(&[("source", source)]), cookie)
            .await;
        (response.status(), text(response).await)
    }

    #[tokio::test]
    async fn preview_is_the_server_rendering_in_a_wrapper() {
        let app = TestApp::new().await;
        let cookie = app.login().await;
        let (status, body) = preview(&app, &cookie, "## Rules\n\n- one\n- two").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.starts_with("<div class=\"md-body\">"), "{body}");
        assert!(body.contains("<h2>Rules</h2>"), "{body}");
        assert!(body.contains("<li>two</li>"), "{body}");
    }

    #[tokio::test]
    async fn preview_escapes_scripts_and_marks_links() {
        let app = TestApp::new().await;
        let cookie = app.login().await;
        let (_, body) = preview(
            &app,
            &cookie,
            "<script>alert(1)</script>\n\n[docs](https://example.org)",
        )
        .await;
        assert!(!body.contains("<script>"), "{body}");
        assert!(body.contains("&lt;script&gt;"), "{body}");
        assert!(
            body.contains("rel=\"noopener noreferrer nofollow\" href=\"https://example.org\""),
            "{body}"
        );
    }

    #[tokio::test]
    async fn preview_returns_a_mermaid_fence_as_diagram_component() {
        let app = TestApp::new().await;
        let cookie = app.login().await;
        let (_, body) = preview(&app, &cookie, "```mermaid\nflowchart LR; A-->B\n```").await;
        assert!(body.contains("x-data=\"mermaidDiagram\""), "{body}");
    }

    #[tokio::test]
    async fn preview_of_an_empty_text_says_so() {
        let app = TestApp::new().await;
        let cookie = app.login().await;
        let (status, body) = preview(&app, &cookie, "").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Nothing to preview"), "{body}");
    }

    #[tokio::test]
    async fn preview_without_the_htmx_header_is_forbidden() {
        let app = TestApp::new().await;
        let cookie = app.login().await;
        let request = post_request(
            "/markdown/preview",
            &form(&[("source", "# x")]),
            &cookie,
            false,
        );
        assert_eq!(app.send(request).await.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn preview_without_a_session_renders_nothing() {
        let app = TestApp::new().await;
        let (status, body) = preview(&app, "", "# Secret heading").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(!body.contains("Secret heading"), "{body}");
    }

    #[tokio::test]
    async fn preview_of_a_text_over_the_limit_is_too_large() {
        let app = TestApp::new().await;
        let cookie = app.login().await;
        let (status, _) = preview(&app, &cookie, &"a".repeat(MAX_SOURCE_BYTES + 1)).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        let (status, _) = preview(&app, &cookie, &"a".repeat(MAX_SOURCE_BYTES)).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn preview_accepts_a_text_that_grows_in_the_form_encoding() {
        let app = TestApp::new().await;
        let cookie = app.login().await;
        let source = "#".repeat(200 * 1024);
        assert!(form(&[("source", &source)]).len() > 512 * 1024);
        let (status, html) = preview(&app, &cookie, &source).await;
        assert_eq!(status, StatusCode::OK);
        assert!(html.contains("md-body"));
    }

    #[tokio::test]
    async fn preview_of_a_body_over_one_mib_is_too_large() {
        let app = TestApp::new().await;
        let cookie = app.login().await;
        let (status, _) = preview(&app, &cookie, &"#".repeat(400 * 1024)).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn preview_leaves_no_trace_of_the_text_in_the_logs() {
        let app = TestApp::new().await;
        let cookie = app.login().await;
        let (logs, _guard) = df_telemetry::testing::capture("trace");
        let (status, _) = preview(&app, &cookie, "marker-7f3a-text").await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = preview(&app, &cookie, &"marker-7f3a-text ".repeat(20_000)).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(!logs.text().contains("marker-7f3a"), "{}", logs.text());
    }
}
