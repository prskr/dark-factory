use std::collections::HashSet;

use axum::Router;
use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, SET_COOKIE};
use axum::http::{Request, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use maud::html;
use tower::ServiceExt;

use super::{Region, error_answers, partial};
use crate::assets::script_source;
use crate::layout::COMPONENT_SCRIPTS;
use crate::responses::{internal_error, invalid, not_found};
use crate::testkit::assert_toast;

#[test]
fn region_ids_are_unique_and_valid_html_ids() {
    let mut seen = HashSet::new();
    for region in Region::ALL {
        let id = region.id();
        assert!(!id.is_empty(), "{region:?}");
        assert!(
            !id.chars().any(|c| c.is_whitespace() || c == '#'),
            "{region:?}: {id:?}"
        );
        assert!(seen.insert(id), "{region:?}: duplicate {id:?}");
        assert_eq!(region.selector(), format!("#{id}"));
    }
}

#[test]
fn a_partial_wraps_its_content_for_the_target() {
    let markup = partial("#x", "innerHTML", html! { b { "hi" } }).into_string();

    assert_eq!(
        markup,
        "<hx-partial hx-target=\"#x\" hx-swap=\"innerHTML\"><b>hi</b></hx-partial>"
    );
}

fn app() -> Router {
    Router::new()
        .route("/not-found", get(|| async { not_found("The task") }))
        .route(
            "/internal",
            get(|| async { internal_error(&"database down") }),
        )
        .route(
            "/cookie",
            get(|| async {
                let mut response = not_found("The task");
                response
                    .headers_mut()
                    .append(SET_COOKIE, "df_session=refreshed".parse().unwrap());
                response
            }),
        )
        .route("/invalid", get(|| async { invalid(html! { "bad form" }) }))
        .route(
            "/conflict",
            get(|| async { (StatusCode::CONFLICT, "already exists").into_response() }),
        )
        .route(
            "/stream",
            get(|| async {
                (
                    [(CONTENT_TYPE, "text/event-stream")],
                    "data: <hx-partial></hx-partial>\n\n",
                )
                    .into_response()
            }),
        )
        .layer(middleware::from_fn(error_answers))
}

async fn send(uri: &str, headers: &[(&str, &str)]) -> (Response, String) {
    let mut request = Request::get(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let body = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    (
        Response::from_parts(parts, Body::empty()),
        String::from_utf8(body.to_vec()).unwrap(),
    )
}

#[tokio::test]
async fn an_htmx_request_gets_the_error_as_a_toast() {
    for (uri, status, title, message) in [
        (
            "/not-found",
            StatusCode::NOT_FOUND,
            "404 Not found",
            "The task does not exist.",
        ),
        (
            "/internal",
            StatusCode::INTERNAL_SERVER_ERROR,
            "500 Error",
            "Something went wrong. Try again.",
        ),
    ] {
        let (response, body) = send(uri, &[("hx-request", "true")]).await;

        assert_eq!(response.status(), status, "{uri}");
        assert_eq!(response.headers()["hx-reswap"], "none", "{uri}");
        assert_eq!(response.headers()["hx-push-url"], "false", "{uri}");
        assert_eq!(
            response.headers()[CONTENT_TYPE],
            "text/html; charset=utf-8",
            "{uri}"
        );
        // The router sets the length from the new body; the page's length must not survive.
        assert_eq!(
            response.headers()["content-length"],
            body.len().to_string().as_str(),
            "{uri}"
        );
        assert!(body.starts_with("<hx-partial"), "{uri}: {body}");
        assert_toast(&body);
        assert!(
            body.contains(title) && body.contains(message),
            "{uri}: {body}"
        );
        assert!(!body.contains("<html"), "{uri}: {body}");
    }
}

#[tokio::test]
async fn the_toast_answer_keeps_headers_the_inner_handler_set() {
    let (response, _) = send("/cookie", &[("hx-request", "true")]).await;

    assert_eq!(response.headers()[SET_COOKIE], "df_session=refreshed");
}

#[tokio::test]
async fn page_loads_and_history_restores_get_the_full_error_page() {
    for headers in [
        vec![],
        vec![
            ("hx-request", "true"),
            ("hx-history-restore-request", "true"),
        ],
    ] {
        let (response, body) = send("/not-found", &headers).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(response.headers().get("hx-reswap").is_none());
        assert!(body.contains("<html"), "{body}");
        assert!(!body.contains("<hx-partial"), "{body}");
    }
}

#[tokio::test]
async fn invalid_forms_conflicts_and_streams_pass_through() {
    let htmx = [("hx-request", "true")];

    let (response, body) = send("/invalid", &htmx).await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body, "bad form");
    assert!(response.headers().get("hx-reswap").is_none());

    let (response, body) = send("/conflict", &htmx).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(body, "already exists");

    let (response, body) = send("/stream", &htmx).await;
    assert_eq!(response.headers()[CONTENT_TYPE], "text/event-stream");
    assert_eq!(body, "data: <hx-partial></hx-partial>\n\n");
}

#[test]
fn the_network_error_script_acts_only_on_requests_without_an_answer() {
    let js = script_source("htmxErrors.ts");

    assert!(js.contains("'htmx:error'"), "{js}");
    assert!(js.contains("!ctx || ctx.response"), "{js}");
    assert!(
        js.contains("data-toasts") && js.contains("data-network-error"),
        "{js}"
    );
    assert!(
        !js.contains("'#") && !js.contains("\"#") && !js.contains("`#"),
        "no element ID is written in the script: {js}"
    );
    assert!(COMPONENT_SCRIPTS.contains(&"htmxErrors.js"));
}

#[test]
fn focus_moves_after_a_swap_only_when_the_request_succeeded() {
    let js = script_source("focusAfterSwap.ts");

    assert!(js.contains("'htmx:after:swap'"), "{js}");
    assert!(js.contains("data-focus-after-swap"), "{js}");
    assert!(js.contains("status < 400"), "{js}");
    assert!(COMPONENT_SCRIPTS.contains(&"focusAfterSwap.js"));
}
