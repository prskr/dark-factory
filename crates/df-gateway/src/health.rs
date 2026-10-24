//! Health and readiness endpoints of the gateway pod.

use axum::Router;
use axum::http::StatusCode;
use axum::routing::get;

use crate::proxy::PolicyReceiver;

/// `/healthz` is always up; `/readyz` only once the first policy snapshot arrived, so that the
/// Service sends no sandbox traffic to a pod that would deny it.
pub fn router(policy: PolicyReceiver) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/readyz",
            get(move || async move {
                if policy.borrow().is_some() {
                    (StatusCode::OK, "ready")
                } else {
                    (StatusCode::SERVICE_UNAVAILABLE, "no policy snapshot yet")
                }
            }),
        )
}
