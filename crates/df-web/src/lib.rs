//! Dark Factory web UI (`df-web`): server-rendered pages for board members.
//!
//! [`base_router`] (health checks and assets) is always available; [`router`] adds the pages
//! behind OIDC login. Other crates reuse [`layout`] and [`auth::require_board_member`] for their
//! own pages.

pub mod assets;
pub mod auth;
mod config_pages;
pub mod layout;
mod sse;
mod task_pages;
#[cfg(test)]
mod testkit;
pub mod transcript;
mod ui;

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
pub fn base_router(pool: PgPool) -> Router {
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
    base_router(state.pool).merge(ui)
}
