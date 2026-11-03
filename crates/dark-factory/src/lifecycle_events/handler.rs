//! The `lifecycle_event.deliver` outbox handler (`lifecycle-events` design D1, D5): signs one
//! event and POSTs it to one receiver.
//!
//! Every attempt carries the event ID as `webhook-id` and its own `webhook-timestamp` and
//! signature. A project subscription records the status or error of each attempt.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use df_db::lifecycle_events::{self, DELIVER_KIND, DeliverJob, Target};
use df_db::outbox::{JobError, OutboxHandler, RetryPolicy};
use df_db::{DbError, PgPool};
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Meter};
use reqwest::header::{HeaderMap, HeaderValue, InvalidHeaderValue};
use reqwest::{StatusCode, Url};
use serde_json::Value;

use super::SigningKey;
use super::client::{Clients, PostError, Trust};

/// Retries of a delivery: 30 s doubling up to 1 h; the dispatcher gives up a job claimed a day
/// after the event. The lease covers the 10 s request timeout and the database writes around it.
pub(crate) const RETRY_POLICY: RetryPolicy = RetryPolicy {
    base: Duration::from_secs(30),
    cap: Duration::from_hours(1),
    give_up_after: Duration::from_hours(24),
    lease: Duration::from_secs(30),
};

/// Sends one attempt; [`Clients`] in production.
#[async_trait]
pub(crate) trait Post: Send + Sync {
    /// POSTs `body` as JSON with `headers` and returns the response status, whatever it is.
    async fn post(
        &self,
        trust: Trust,
        url: &Url,
        headers: HeaderMap,
        body: Vec<u8>,
    ) -> Result<StatusCode, PostError>;
}

#[async_trait]
impl Post for Clients {
    async fn post(
        &self,
        trust: Trust,
        url: &Url,
        headers: HeaderMap,
        body: Vec<u8>,
    ) -> Result<StatusCode, PostError> {
        Self::post(self, trust, url, headers, body).await
    }
}

/// Runs [`DELIVER_KIND`] jobs.
pub(crate) struct DeliverHandler {
    pool: PgPool,
    key: Arc<SigningKey>,
    sender: Arc<dyn Post>,
    deliveries: Counter<u64>,
}

impl DeliverHandler {
    pub(crate) fn new(
        pool: PgPool,
        key: Arc<SigningKey>,
        sender: Arc<dyn Post>,
        meter: &Meter,
    ) -> Self {
        Self {
            pool,
            key,
            sender,
            deliveries: meter
                .u64_counter("df.lifecycle_event.deliveries")
                .with_description("Lifecycle event delivery attempts by result")
                .build(),
        }
    }

    fn count(&self, result: DeliveryResult) {
        self.deliveries
            .add(1, &[KeyValue::new("result", result.as_str())]);
    }
}

/// The `result` of one attempt in `df.lifecycle_event.deliveries`. Give-ups are counted by
/// `df.outbox.jobs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryResult {
    Delivered,
    Failed,
    Refused,
}

impl DeliveryResult {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Failed => "failed",
            Self::Refused => "refused",
        }
    }
}

#[async_trait]
impl OutboxHandler for DeliverHandler {
    fn kind(&self) -> &'static str {
        DELIVER_KIND
    }

    fn retry_policy(&self) -> RetryPolicy {
        RETRY_POLICY
    }

    async fn handle(&self, payload: &Value) -> Result<(), JobError> {
        let job: DeliverJob = serde_json::from_value(payload.clone())
            .map_err(|e| JobError::GiveUp(format!("invalid delivery job: {e}")))?;
        let receiver = receiver(&job.target);
        let delivery = lifecycle_events::load_delivery(&self.pool, &job)
            .await
            .map_err(|e| JobError::Retry(format!("{receiver}: {e}")))?
            .ok_or_else(|| {
                JobError::GiveUp(format!(
                    "{receiver}: the event or subscription no longer exists"
                ))
            })?;
        let url = Url::parse(&delivery.url)
            .map_err(|e| JobError::GiveUp(format!("{receiver}: invalid receiver URL: {e}")))?;
        let body = serde_json::to_vec(&delivery.payload)
            .map_err(|e| JobError::GiveUp(format!("cannot serialize the event: {e}")))?;
        let trust = match job.target {
            Target::Subscription { .. } => Trust::Project,
            Target::Instance { .. } => Trust::Instance,
        };

        let event_id = format!("evt_{}", job.event_id);
        let timestamp = Utc::now().timestamp();
        let signature = self.key.sign(&event_id, timestamp, &body);
        let headers = headers(&event_id, timestamp, &signature)
            .map_err(|e| JobError::GiveUp(format!("invalid header value: {e}")))?;
        let result = self.sender.post(trust, &url, headers, body).await;

        let (status, error) = match &result {
            Ok(status) if status.is_success() => (Some(*status), None),
            Ok(status) => (Some(*status), Some(format!("HTTP {status}"))),
            Err(e) => (None, Some(describe(e))),
        };
        if let Target::Subscription { subscription_id } = job.target {
            let recorded = lifecycle_events::record_attempt(
                &self.pool,
                subscription_id,
                status.map(|status| status.as_u16()),
                error.as_deref().filter(|_| status.is_none()),
            )
            .await;
            if let Err(e) = recorded {
                tracing::warn!(error = %e, %subscription_id, "cannot record a delivery attempt");
            }
        }
        let Some(error) = error else {
            self.count(DeliveryResult::Delivered);
            return Ok(());
        };
        self.count(match result {
            Err(PostError::Refused(_)) => DeliveryResult::Refused,
            _ => DeliveryResult::Failed,
        });
        match job.target {
            Target::Instance { .. } => tracing::warn!(
                %event_id, %receiver, %error, "a lifecycle event delivery attempt failed"
            ),
            Target::Subscription { .. } => tracing::debug!(
                %event_id, %receiver, %error, "a lifecycle event delivery attempt failed"
            ),
        }
        Err(JobError::Retry(format!("{receiver}: {error}")))
    }

    async fn cleanup(&self, pool: &PgPool) -> Result<(), DbError> {
        let deleted = lifecycle_events::delete_unreferenced(pool).await?;
        tracing::debug!(deleted, "deleted delivered lifecycle events");
        Ok(())
    }
}

/// Names the receiver in logs and job errors: a project subscription by its ID, an instance
/// receiver by the host of its URL (see [`host`]).
fn receiver(target: &Target) -> String {
    match target {
        Target::Subscription { subscription_id } => format!("subscription {subscription_id}"),
        Target::Instance { instance_url } => {
            let host =
                Url::parse(instance_url).map_or_else(|_| "<invalid URL>".into(), |url| host(&url));
            format!("instance receiver {host}")
        }
    }
}

/// Host and port of `url`; leaves out user info, path and query, which may carry a token.
fn host(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.into(),
    }
}

/// The Standard Webhooks headers of one attempt.
fn headers(
    event_id: &str,
    timestamp: i64,
    signature: &str,
) -> Result<HeaderMap, InvalidHeaderValue> {
    let mut headers = HeaderMap::new();
    headers.insert("webhook-id", HeaderValue::from_str(event_id)?);
    headers.insert("webhook-timestamp", HeaderValue::from(timestamp));
    headers.insert("webhook-signature", HeaderValue::from_str(signature)?);
    Ok(headers)
}

/// The error shown for an attempt without a response: the refusal, a timeout, or the causes.
/// Leaves out the URL, which may carry a token.
fn describe(err: &PostError) -> String {
    match err {
        PostError::Refused(_) => err.to_string(),
        PostError::Transport(e) if e.is_timeout() => "timed out".into(),
        PostError::Transport(e) => {
            let mut message = String::from(if e.is_connect() {
                "connection failed"
            } else {
                "request failed"
            });
            let mut source = std::error::Error::source(e);
            while let Some(cause) = source {
                message = format!("{message}: {cause}");
                source = cause.source();
            }
            message
        }
    }
}

#[cfg(test)]
mod tests;
