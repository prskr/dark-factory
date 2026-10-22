//! Embedded static assets: vendored htmx/Alpine, compiled Tailwind stylesheet.
//!
//! URLs carry a content hash (`?v=<hash>`), so responses are cached for a year.

use std::borrow::Cow;

use axum::Router;
use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use rust_embed::{EmbeddedFile, RustEmbed};

/// Embedded static assets and compiled `app.css` stylesheet.
#[derive(RustEmbed)]
#[folder = "$OUT_DIR/assets/"]
pub struct Assets;

const CACHE_CONTROL: &str = "public, max-age=31536000, immutable";

/// Returns a bundled static asset by relative path.
pub fn get_asset(path: &str) -> Option<EmbeddedFile> {
    Assets::get(path)
}

/// Returns the compiled `app.css` stylesheet embedded at build time.
pub fn app_css() -> Option<Cow<'static, str>> {
    let file = Assets::get("app.css")?;
    match file.data {
        Cow::Borrowed(bytes) => std::str::from_utf8(bytes).ok().map(Cow::Borrowed),
        Cow::Owned(bytes) => String::from_utf8(bytes).ok().map(Cow::Owned),
    }
}

/// URL of an asset including its content hash, `/assets/app.css?v=1a2b3c4d`.
pub fn asset_url(path: &str) -> String {
    match Assets::get(path) {
        Some(file) => format!(
            "/assets/{path}?v={}",
            hex::encode(&file.metadata.sha256_hash()[..4])
        ),
        None => format!("/assets/{path}"),
    }
}

/// Route `GET /assets/{*path}`.
pub fn routes() -> Router {
    Router::new().route("/assets/{*path}", get(serve_asset))
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

async fn serve_asset(Path(path): Path<String>) -> Response {
    let Some(file) = Assets::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let etag = format!("\"{}\"", hex::encode(file.metadata.sha256_hash()));
    let mut response = file.data.into_owned().into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&path)),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_CONTROL),
    );
    if let Ok(etag) = HeaderValue::from_str(&etag) {
        headers.insert(header::ETAG, etag);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn get_path(path: &str) -> Response {
        routes()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    #[test]
    fn embeds_stylesheet_and_vendored_scripts() {
        assert!(!app_css().expect("app.css is embedded").is_empty());
        for path in [
            "vendor/htmx.min.js",
            "vendor/hx-sse.min.js",
            "vendor/alpine.min.js",
        ] {
            assert!(get_asset(path).is_some(), "{path}");
        }
    }

    #[test]
    fn stylesheet_covers_the_classes_the_pages_use() {
        let css = app_css().expect("app.css is embedded");
        for rule in [
            ".min-h-11",
            ".inline-flex",
            ".hidden",
            "md\\:flex",
            "md\\:hidden",
            ".md-body",
            ".overflow-x-auto",
        ] {
            assert!(
                css.contains(rule),
                "app.css lacks `{rule}`; regenerate assets/app.css with tailwindcss"
            );
        }
    }

    #[tokio::test]
    async fn assets_are_served_with_long_cache_headers() {
        let response = get_path("/assets/vendor/htmx.min.js").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], CACHE_CONTROL);
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("text/javascript")
        );
        assert!(response.headers().contains_key(header::ETAG));
    }

    #[tokio::test]
    async fn unknown_asset_is_404() {
        assert_eq!(
            get_path("/assets/nope.js").await.status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn asset_url_carries_a_content_hash() {
        let url = asset_url("app.css");
        assert!(url.starts_with("/assets/app.css?v="), "{url}");
        assert_eq!(url.len(), "/assets/app.css?v=".len() + 8);
    }
}
