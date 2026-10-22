//! Human authentication: OIDC login, server-side sessions, role check and CSRF protection
//! (`specs/human-auth/spec.md`).

mod oidc;
mod store;
#[cfg(test)]
mod tests;

pub use oidc::{Identity, OidcClient, OidcError, PendingLogin};
pub use store::PgSessionStore;

use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use df_core::{Principal, UserId};
use maud::{Markup, html};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use crate::WebState;
use crate::layout::{Nav, layout};
use crate::ui::htmx_redirect;

/// Role every page and action requires.
pub const REQUIRED_ROLE: &str = "board-member";
/// Seconds before expiry at which a token counts as expired.
const REFRESH_SKEW: i64 = 30;

const SESSION_USER: &str = "user";
const SESSION_LOGIN: &str = "login";

/// What the session holds about a logged-in user. Tokens never leave the server.
#[derive(Clone, Serialize, Deserialize)]
struct SessionUser {
    user_id: UserId,
    display_name: String,
    roles: Vec<String>,
    id_token: String,
    refresh_token: Option<String>,
    expires_at: i64,
}

impl std::fmt::Debug for SessionUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionUser")
            .field("user_id", &self.user_id)
            .field("display_name", &self.display_name)
            .field("roles", &self.roles)
            .finish_non_exhaustive()
    }
}

/// The authenticated board member of the current request (request extension).
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: UserId,
    pub display_name: String,
    pub roles: Vec<String>,
}

impl CurrentUser {
    pub fn principal(&self) -> Principal {
        Principal::human(self.id, self.display_name.clone())
    }
}

/// Rejects state-changing requests that lack the header htmx sends. Together with
/// `SameSite=Lax` this stops cross-site form posts (ADR-0011).
pub async fn csrf_guard(request: Request, next: Next) -> Response {
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if !safe && !is_htmx(request.headers()) {
        return error_page(
            StatusCode::FORBIDDEN,
            "Forbidden",
            "State-changing requests must come from the UI.",
        );
    }
    next.run(request).await
}

/// Requires a logged-in user with the `board-member` role; refreshes expired tokens.
pub async fn require_board_member(
    State(state): State<WebState>,
    session: Session,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(mut user) = load_user(&session).await else {
        return login_redirect(&request);
    };
    if user.expires_at - REFRESH_SKEW <= now_unix() {
        match refresh_user(&state, &session, user).await {
            Some(refreshed) => user = refreshed,
            None => return login_redirect(&request),
        }
    }
    if !user.roles.iter().any(|r| r == REQUIRED_ROLE) {
        return error_page(
            StatusCode::FORBIDDEN,
            "Forbidden",
            &format!("Your account lacks the role `{REQUIRED_ROLE}`."),
        );
    }
    request.extensions_mut().insert(CurrentUser {
        id: user.user_id,
        display_name: user.display_name,
        roles: user.roles,
    });
    next.run(request).await
}

/// A full page for errors outside the normal flow.
pub fn error_page(status: StatusCode, title: &str, message: &str) -> Response {
    let body: Markup = html! {
        section class="mx-auto max-w-md py-10 text-center" {
            h1 class="text-2xl font-semibold" { (status.as_u16()) " " (title) }
            p class="mt-3 text-slate-300" { (message) }
            div class="mt-6 flex justify-center gap-3" {
                a href="/" class="min-h-11 rounded bg-slate-800 px-4 py-2" { "Home" }
                button type="button" class="min-h-11 rounded bg-slate-800 px-4 py-2" hx-post="/logout" { "Log out" }
            }
        }
    };
    (status, layout(title, Nav::None, body)).into_response()
}

#[derive(Debug, Deserialize)]
pub struct LoginQuery {
    next: Option<String>,
}

/// `GET /login`: redirects to the provider.
pub async fn login(
    State(state): State<WebState>,
    session: Session,
    Query(query): Query<LoginQuery>,
) -> Response {
    let next = sanitize_next(query.next.as_deref());
    if load_user(&session).await.is_some() {
        return Redirect::to(&next).into_response();
    }
    let (url, pending) = match state.oidc.authorize(&next).await {
        Ok(started) => started,
        Err(error) => return login_unavailable(&error),
    };
    if let Err(error) = session.insert(SESSION_LOGIN, &pending).await {
        tracing::error!(%error, "saving the login state failed");
        return error_page(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Error",
            "Login is unavailable.",
        );
    }
    Redirect::to(url.as_str()).into_response()
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// `GET /auth/callback`: checks the state, exchanges the code and creates the session.
pub async fn callback(
    State(state): State<WebState>,
    session: Session,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let pending = session
        .remove::<PendingLogin>(SESSION_LOGIN)
        .await
        .ok()
        .flatten();
    let (Some(pending), Some(code), None) = (pending, query.code, query.error.as_ref()) else {
        return reject_login(
            &session,
            "The login request was not started here or was refused.",
        )
        .await;
    };
    if query.state.as_deref() != Some(pending.state.as_str()) {
        return reject_login(&session, "The login state does not match.").await;
    }
    let identity = match state.oidc.complete_login(code, &pending).await {
        Ok(identity) => identity,
        Err(error) => {
            tracing::warn!(%error, "login failed");
            return reject_login(&session, "The identity provider rejected the login.").await;
        }
    };
    let user_id = match df_db::users::upsert_user(
        &state.pool,
        &identity.issuer,
        &identity.subject,
        &identity.display_name,
    )
    .await
    {
        Ok(id) => id,
        Err(error) => {
            tracing::error!(%error, "saving the user failed");
            return error_page(StatusCode::INTERNAL_SERVER_ERROR, "Error", "Login failed.");
        }
    };
    let user = SessionUser {
        user_id,
        display_name: identity.display_name,
        roles: identity.roles,
        id_token: identity.id_token,
        refresh_token: identity.refresh_token,
        expires_at: identity.expires_at,
    };
    // A new session id after login prevents session fixation.
    if let Err(error) = session.cycle_id().await {
        tracing::error!(%error, "cycling the session id failed");
    }
    if let Err(error) = session.insert(SESSION_USER, &user).await {
        tracing::error!(%error, "saving the session failed");
        return error_page(StatusCode::INTERNAL_SERVER_ERROR, "Error", "Login failed.");
    }
    Redirect::to(&sanitize_next(Some(&pending.next))).into_response()
}

/// `POST /logout`: deletes the session and sends the browser to the provider's end-session
/// endpoint.
pub async fn logout(State(state): State<WebState>, session: Session) -> Response {
    let user = load_user(&session).await;
    if let Err(error) = session.flush().await {
        tracing::warn!(%error, "deleting the session failed");
    }
    let target = match user {
        Some(user) => state
            .oidc
            .end_session_url(&user.id_token)
            .await
            .inspect_err(|error| tracing::warn!(%error, "no end-session URL"))
            .ok()
            .flatten()
            .map_or_else(|| "/".to_string(), |url| url.to_string()),
        None => "/".to_string(),
    };
    htmx_redirect(&target)
}

async fn load_user(session: &Session) -> Option<SessionUser> {
    session
        .get::<SessionUser>(SESSION_USER)
        .await
        .inspect_err(|error| tracing::warn!(%error, "reading the session failed"))
        .ok()
        .flatten()
}

/// Refreshes the tokens and roles; on failure the session is destroyed.
async fn refresh_user(
    state: &WebState,
    session: &Session,
    user: SessionUser,
) -> Option<SessionUser> {
    let refreshed = match &user.refresh_token {
        Some(token) => state.oidc.refresh(token).await,
        None => Err(OidcError::Token("no refresh token".into())),
    };
    match refreshed {
        Ok(identity) => {
            let user = SessionUser {
                roles: identity.roles,
                id_token: identity.id_token,
                refresh_token: identity.refresh_token,
                expires_at: identity.expires_at,
                ..user
            };
            session
                .insert(SESSION_USER, &user)
                .await
                .inspect_err(|error| tracing::warn!(%error, "saving the session failed"))
                .ok()?;
            Some(user)
        }
        Err(error) => {
            tracing::info!(%error, "token refresh failed; ending the session");
            if let Err(error) = session.flush().await {
                tracing::warn!(%error, "deleting the session failed");
            }
            None
        }
    }
}

fn login_redirect(request: &Request) -> Response {
    if is_htmx(request.headers()) {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        response
            .headers_mut()
            .insert("HX-Redirect", HeaderValue::from_static("/login"));
        return response;
    }
    let target = request.uri().path_and_query().map_or("/", |pq| pq.as_str());
    Redirect::to(&format!("/login?next={}", urlencode(target))).into_response()
}

fn is_htmx(headers: &HeaderMap) -> bool {
    headers
        .get("hx-request")
        .is_some_and(|v| v.as_bytes() == b"true")
}

fn urlencode(value: &str) -> String {
    openidconnect::url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Only local paths are valid return targets.
fn sanitize_next(next: Option<&str>) -> String {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.starts_with("/\\") => n.into(),
        _ => "/".into(),
    }
}

fn login_unavailable(error: &OidcError) -> Response {
    tracing::error!(%error, "login failed");
    error_page(
        StatusCode::BAD_GATEWAY,
        "Login unavailable",
        "The identity provider could not be reached.",
    )
}

async fn reject_login(session: &Session, message: &str) -> Response {
    if let Err(error) = session.flush().await {
        tracing::warn!(%error, "deleting the session failed");
    }
    error_page(StatusCode::BAD_REQUEST, "Login failed", message)
}

/// Test support: sessions without going through the provider.
#[cfg(test)]
pub(crate) mod testing {
    use df_core::UserId;
    use sqlx::PgPool;
    use tower_sessions::cookie::time::{Duration, OffsetDateTime};
    use tower_sessions::session::{Id, Record};
    use tower_sessions::session_store::SessionStore;

    use super::{SESSION_USER, SessionUser, now_unix};
    use crate::auth::PgSessionStore;

    /// Creates a logged-in session and returns its `Cookie` header value and the user.
    pub(crate) async fn seed_session(
        pool: &PgPool,
        roles: &[&str],
        expires_in: i64,
        refresh_token: Option<&str>,
    ) -> (String, UserId) {
        let user_id = df_db::users::upsert_user(pool, "https://idp.test", "sub-1", "Peter")
            .await
            .unwrap();
        let user = SessionUser {
            user_id,
            display_name: "Peter".into(),
            roles: roles.iter().map(ToString::to_string).collect(),
            id_token: "id-token".into(),
            refresh_token: refresh_token.map(ToString::to_string),
            expires_at: now_unix() + expires_in,
        };
        let mut record = Record {
            id: Id::default(),
            data: std::collections::HashMap::from([(
                SESSION_USER.to_string(),
                serde_json::to_value(user).unwrap(),
            )]),
            expiry_date: OffsetDateTime::now_utc() + Duration::hours(1),
        };
        PgSessionStore::new(pool.clone())
            .create(&mut record)
            .await
            .unwrap();
        (format!("{}={}", crate::SESSION_COOKIE, record.id), user_id)
    }
}
