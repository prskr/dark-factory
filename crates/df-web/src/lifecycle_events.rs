//! `GET /lifecycle-events/public-key`: the public key of the lifecycle event signatures, without
//! login, so receivers can fetch it. `404` while the feature is off.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;

/// The route; `public_key` is `whpk_<base64>`, `None` without a signing key.
pub(crate) fn routes(public_key: Option<Arc<str>>) -> Router {
    Router::new()
        .route("/lifecycle-events/public-key", get(public_key_text))
        .with_state(public_key)
}

async fn public_key_text(State(public_key): State<Option<Arc<str>>>) -> Result<String, StatusCode> {
    public_key
        .map(|key| key.to_string())
        .ok_or(StatusCode::NOT_FOUND)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, header};
    use tower::ServiceExt;

    use super::*;

    async fn get_key(public_key: Option<&str>) -> axum::response::Response {
        let pool = sqlx::PgPool::connect_lazy("postgres://unused/db").unwrap();
        crate::base_router(pool, public_key.map(Arc::from))
            .oneshot(
                Request::get("/lifecycle-events/public-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn public_key_is_served_without_login() {
        let response = get_key(Some("whpk_c2lnbmluZw==")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response.headers()[header::CONTENT_TYPE].to_str().unwrap();
        assert!(content_type.starts_with("text/plain"), "{content_type}");
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&body[..], b"whpk_c2lnbmluZw==");
    }

    #[tokio::test]
    async fn public_key_is_not_found_with_the_feature_off() {
        assert_eq!(get_key(None).await.status(), StatusCode::NOT_FOUND);
    }
}
