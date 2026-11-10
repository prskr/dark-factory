//! Dark Factory web UI (`df-web`): server-rendered pages for board members.
//!
//! [`base_router`] (health checks and assets) is always available; [`router`] adds the pages
//! behind OIDC login. Other crates reuse [`layout`] and [`auth::require_board_member`] for their
//! own pages.

mod artifacts;
pub mod assets;
pub mod auth;
// The asset staging of `build.rs`, compiled here for its tests.
#[cfg(test)]
#[path = "../build_assets.rs"]
mod build_assets;
pub mod components;
mod config_pages;
pub mod egress;
pub mod layout;
mod lifecycle_events;
pub mod markdown;
mod project_subscriptions;
mod project_webhook;
mod requests;
mod responses;
mod sse;
mod task_pages;
#[cfg(test)]
mod testkit;
pub mod transcript;
pub mod webhooks;

pub use assets::{app_css, get_asset};
pub use layout::{Nav, Shell, layout};

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
    /// The forge routing table with each plugin's availability; projects show their forge by icon
    /// and name.
    pub forges: df_core::routing::ForgeRoutes,
    /// The forge client; marking a task done uses it to mark the PR ready for review.
    pub forge: Arc<dyn df_core::ForgeClient>,
    /// The sandbox provider; resetting a task's workspace deletes its volume.
    pub provider: Arc<dyn df_core::SandboxProvider>,
    /// Flow events of the egress gateways; without it the egress log page is unavailable.
    pub flows: Option<Arc<dyn egress::FlowSource>>,
    /// Checks `cluster-service` entries against the cluster; without it they cannot be added.
    pub services: Option<Arc<dyn egress::ServiceLookup>>,
    /// Show transcript content blocks marked `hidden` (`web.showHiddenBlocks`).
    pub show_hidden_blocks: bool,
    /// The stylesheet of highlighted code (`web.codeTheme`), generated at startup.
    pub code_theme: markdown::CodeTheme,
    /// Accept `http://` repository URLs in the project form (`network.allowPlainHttp`).
    pub allow_plain_http: bool,
    /// `runs.maxAutoResumes`, the denominator of the resume count on the task detail.
    pub max_auto_resumes: u32,
    /// The approval types of this instance; they define what a request asks and does.
    pub approval_types: df_db::requests::ApprovalTypes,
    /// The release version shown in the profile menu (`dev` for unstamped builds).
    pub version: String,
    /// The clock the session check compares token expiry against ([`auth::SystemClock`]).
    pub clock: Arc<dyn auth::Clock>,
    /// Receivers of lifecycle events; resetting a workspace emits `volume.destroyed`.
    pub events: df_db::lifecycle_events::EventSinks,
    /// The `whpk_` public key of the lifecycle event signatures; `None` while the feature is off.
    pub lifecycle_public_key: Option<Arc<str>>,
    /// Which targets project subscriptions may use; [`ProjectTargets::Unrestricted`] only in a
    /// development build started with `--unrestricted-project-subscriptions`.
    ///
    /// [`ProjectTargets::Unrestricted`]: df_core::public_address::ProjectTargets::Unrestricted
    pub project_targets: df_core::public_address::ProjectTargets,
    /// `publicUrl`: the base of absolute URLs shown in the UI, such as the webhook URL.
    pub public_url: Option<url::Url>,
    /// The webhook endpoints of the forge plugins: the settings page shows their state and
    /// reveals the secret to admins.
    pub webhooks: Arc<dyn webhooks::WebhookReceiver>,
}

/// `/healthz`, `/readyz`, `/assets/*` and `/lifecycle-events/public-key`; no login required.
///
/// Both routers compress responses (brotli or gzip, as the browser accepts) with one layer
/// around all routes. Its default predicate leaves SSE uncompressed, so live updates are not
/// held back in compression buffers.
pub fn base_router(pool: PgPool, lifecycle_public_key: Option<Arc<str>>) -> Router {
    base_routes(pool, lifecycle_public_key).layer(CompressionLayer::new())
}

fn base_routes(pool: PgPool, lifecycle_public_key: Option<Arc<str>>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(readyz))
        .with_state(pool)
        .merge(assets::routes())
        .merge(lifecycle_events::routes(lifecycle_public_key))
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
        .merge(requests::routes())
        .merge(config_pages::routes())
        .merge(project_subscriptions::routes())
        .merge(project_webhook::routes())
        .merge(egress::routes())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_board_member,
        ));
    let sessions = SessionManagerLayer::new(auth::PgSessionStore::new(state.pool.clone()))
        .with_name(SESSION_COOKIE)
        .with_secure(state.oidc.secure_cookies())
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
    base_routes(state.pool, state.lifecycle_public_key)
        .merge(state.code_theme.routes())
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
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
    async fn assets_are_compressed_without_login() {
        let pool = sqlx::PgPool::connect_lazy("postgres://unused/db").unwrap();
        let css = crate::base_router(pool, None)
            .oneshot(get("/assets/app.css", ""))
            .await
            .unwrap();
        assert_eq!(header_str(&css, "content-encoding"), "br");
    }

    // Needs a PostgreSQL container (testcontainers).
    #[tokio::test]
    async fn pages_link_the_hashed_code_theme_stylesheet() {
        let app = TestApp::new().await;
        let cookie = app.login().await;
        let plain = Request::get("/tasks")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap();
        let page = app.send(plain).await;
        let page = String::from_utf8(
            axum::body::to_bytes(page.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let url = crate::markdown::CodeTheme::default().url().to_owned();
        assert!(page.contains(&format!("href=\"{url}\"")), "{page}");
    }

    // Needs a PostgreSQL container (testcontainers).
    #[tokio::test]
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
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
