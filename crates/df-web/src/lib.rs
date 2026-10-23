//! Dark Factory web UI (`df-web`): server-rendered pages for board members.
//!
//! [`base_router`] (health checks and assets) is always available; [`router`] adds the pages
//! behind OIDC login. Other crates reuse [`layout`] and [`auth::require_board_member`] for their
//! own pages.

pub mod assets;
pub mod auth;
pub mod components;
mod config_pages;
pub mod layout;
mod responses;
mod sse;
mod task_pages;
#[cfg(test)]
mod testkit;
pub mod transcript;

pub use assets::{app_css, get_asset};
pub use layout::{Nav, layout};

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{get, post};
use df_db::{Notifier, PgPool};
use tower_http::compression::CompressionLayer;
use tower_sessions::cookie::SameSite;
use tower_sessions::cookie::time::Duration as CookieDuration;
use tower_sessions::{Expiry, SessionManagerLayer};

/// Name of the session cookie.
const SESSION_COOKIE: &str = "df_session";
/// Sessions end after this long without a request.
const SESSION_INACTIVITY: CookieDuration = CookieDuration::hours(8);
const READY_TIMEOUT: Duration = Duration::from_secs(2);

/// Shared state of all handlers.
#[derive(Clone)]
pub struct WebState {
    pub pool: PgPool,
    pub notifier: Arc<Notifier>,
    pub oidc: Arc<auth::OidcClient>,
}

/// `/healthz`, `/readyz` and `/assets/*`; no login required.
///
/// Both routers compress responses (brotli or gzip, as the browser accepts) with one layer
/// around all routes. Its default predicate leaves SSE uncompressed, so live updates are not
/// held back in compression buffers.
pub fn base_router(pool: PgPool) -> Router {
    base_routes(pool).layer(CompressionLayer::new())
}

fn base_routes(pool: PgPool) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(readyz))
        .with_state(pool)
        .merge(assets::routes())
}

async fn readyz(State(pool): State<PgPool>) -> (StatusCode, &'static str) {
    match tokio::time::timeout(READY_TIMEOUT, sqlx::query("SELECT 1").execute(&pool)).await {
        Ok(Ok(_)) => (StatusCode::OK, "ready"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "database unavailable"),
    }
}

/// The complete UI: base routes, login and all pages.
pub fn router(state: WebState) -> Router {
    let pages = Router::new()
        .merge(task_pages::routes())
        .merge(config_pages::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_board_member,
        ));
    let sessions = SessionManagerLayer::new(auth::PgSessionStore::new(state.pool.clone()))
        .with_name(SESSION_COOKIE)
        .with_secure(true)
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_expiry(Expiry::OnInactivity(SESSION_INACTIVITY));
    let ui = Router::new()
        .route("/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/logout", post(auth::logout))
        .merge(pages)
        .layer(middleware::from_fn(auth::csrf_guard))
        .layer(sessions)
        .with_state(state.clone());
    base_routes(state.pool)
        .merge(ui)
        .layer(CompressionLayer::new())
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, header};
    use tower::ServiceExt;

    use crate::testkit::{TestApp, header_str};

    fn get(uri: &str, cookie: &str) -> Request<Body> {
        Request::get(uri)
            .header(header::COOKIE, cookie)
            .header(header::ACCEPT_ENCODING, "br, gzip")
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn assets_are_compressed_without_login() {
        let pool = sqlx::PgPool::connect_lazy("postgres://unused/db").unwrap();
        let css = crate::base_router(pool)
            .oneshot(get("/assets/app.css", ""))
            .await
            .unwrap();
        assert_eq!(header_str(&css, "content-encoding"), "br");
    }

    // Needs a PostgreSQL container (testcontainers).
    #[tokio::test]
    async fn pages_are_compressed_but_live_streams_are_not() {
        let app = TestApp::new().await;
        let cookie = app.login().await;
        for uri in ["/tasks", "/assets/app.css"] {
            let response = app.send(get(uri, &cookie)).await;
            assert_eq!(header_str(&response, "content-encoding"), "br", "{uri}");
        }
        // SSE must reach the browser event by event, not in compressed blocks.
        let stream = app.send(get("/tasks/events", &cookie)).await;
        assert_eq!(header_str(&stream, "content-type"), "text/event-stream");
        assert!(stream.headers().get(header::CONTENT_ENCODING).is_none());
    }
}
