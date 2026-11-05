//! `POST /webhooks/{plugin}`: forge webhook deliveries (`iter3-webhooks` design D1). No login,
//! session or CSRF token: the plugin verifies the delivery's signature. Every response body is
//! empty.

use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Meter};

/// Largest accepted body (spec "Webhook endpoint").
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
/// `plugin` label of requests for a plugin that is not loaded; keeps the label set bounded.
const UNKNOWN_PLUGIN: &str = "unknown";

/// Verifies, parses and applies webhook deliveries; implemented over the forge plugins.
#[async_trait]
pub trait WebhookReceiver: Send + Sync {
    /// Whether `plugin` (the name under `plugins`) loaded.
    fn is_loaded(&self, plugin: &str) -> bool;

    /// Whether `plugin` loaded, exports `webhooks` and has a webhook secret.
    fn has_endpoint(&self, plugin: &str) -> bool;

    /// Handles one delivery to `plugin`.
    async fn receive(
        &self,
        plugin: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<(), WebhookRejection>;
}

/// Why a delivery is not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookRejection {
    /// The plugin has no webhook endpoint.
    NotFound,
    /// The signature check failed.
    Unauthorized,
    /// The plugin rejected the payload.
    Invalid,
    /// Storing or applying the events failed; the forge records a failed delivery.
    Failed,
}

/// The receiver and the request counter of the webhook route.
#[derive(Clone)]
pub struct Webhooks {
    receiver: Arc<dyn WebhookReceiver>,
    requests: Counter<u64>,
}

impl Webhooks {
    /// Counts requests as `df.webhook.requests` on `meter`.
    pub fn new(receiver: Arc<dyn WebhookReceiver>, meter: &Meter) -> Self {
        let requests = meter
            .u64_counter("df.webhook.requests")
            .with_description("Webhook requests by plugin and outcome.")
            .build();
        Self { receiver, requests }
    }

    fn count(&self, plugin: &str, outcome: Outcome) -> StatusCode {
        self.requests.add(
            1,
            &[
                KeyValue::new("plugin", plugin.to_string()),
                KeyValue::new("outcome", outcome.label()),
            ],
        );
        outcome.status()
    }
}

/// The webhook route; mounted next to the health checks, outside login.
pub fn routes(webhooks: Webhooks) -> Router {
    Router::new()
        .route("/webhooks/{plugin}", post(receive))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(webhooks)
}

#[derive(Debug, Clone, Copy)]
enum Outcome {
    Accepted,
    Unauthorized,
    NotFound,
    TooLarge,
    Invalid,
    Error,
}

impl Outcome {
    fn label(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Unauthorized => "unauthorized",
            Self::NotFound => "not_found",
            Self::TooLarge => "too_large",
            Self::Invalid => "invalid",
            Self::Error => "error",
        }
    }

    fn status(self) -> StatusCode {
        match self {
            Self::Accepted => StatusCode::ACCEPTED,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Invalid => StatusCode::BAD_REQUEST,
            Self::Error => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<WebhookRejection> for Outcome {
    fn from(rejection: WebhookRejection) -> Self {
        match rejection {
            WebhookRejection::NotFound => Self::NotFound,
            WebhookRejection::Unauthorized => Self::Unauthorized,
            WebhookRejection::Invalid => Self::Invalid,
            WebhookRejection::Failed => Self::Error,
        }
    }
}

/// An unknown plugin is answered before the body size, so a probe learns nothing else.
async fn receive(
    State(webhooks): State<Webhooks>,
    Path(plugin): Path<String>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> StatusCode {
    if !webhooks.receiver.has_endpoint(&plugin) {
        let label = if webhooks.receiver.is_loaded(&plugin) {
            plugin.as_str()
        } else {
            UNKNOWN_PLUGIN
        };
        return webhooks.count(label, Outcome::NotFound);
    }
    let body = match body {
        Ok(body) => body,
        Err(rejection) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return webhooks.count(&plugin, Outcome::TooLarge);
        }
        Err(_unreadable) => return webhooks.count(&plugin, Outcome::Invalid),
    };
    // Signature headers are ASCII; a header that is not is no part of the check.
    let headers: Vec<(String, String)> = headers
        .iter()
        .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_string())))
        .collect();
    let outcome = match webhooks.receiver.receive(&plugin, &headers, &body).await {
        Ok(()) => Outcome::Accepted,
        Err(rejection) => Outcome::from(rejection),
    };
    webhooks.count(&plugin, outcome)
}

#[cfg(test)]
mod tests;
