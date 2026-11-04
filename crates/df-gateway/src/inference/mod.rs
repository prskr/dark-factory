//! The inference endpoint (iter3-inference-gateway): model traffic of a sandbox, identified by its
//! pod IP, forwarded to the run's model provider with the provider credential, streamed back and
//! measured for usage.
//!
//! It shares the Git port, its TLS and its upstream client (design D1, D2).

mod forward;
pub mod headers;
pub mod report;
pub mod route;
#[cfg(test)]
pub(crate) mod testing;
pub mod usage;

use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use bytes::Bytes;
use df_core::model_provider::WireApi;
use df_proto::gateway::v1::InferenceRoute;
use http_body_util::BodyExt;
use hyper::header::HOST;
use hyper::{Request, StatusCode, Uri};

use self::forward::{Outcome, relay};
use self::headers::upstream_headers;
use self::report::UsageSink;
use crate::convert;
use crate::metrics::Metrics;
use crate::policy::Policy;
use crate::upstream::{
    Body, BodyLimit, BoxError, LimitedBody, UpstreamConnector, UpstreamUrl, text,
};

type Response = hyper::Response<Body>;

/// Time a model provider has to send the response head. A model may think for minutes before its
/// first byte.
pub(super) const RESPONSE_HEAD_TIMEOUT: Duration = Duration::from_mins(10);
/// Longest pause between two frames of the request body from the sandbox; a stalled upload ends.
pub(super) const UPLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest pause between two frames of the response body; streams send keep-alive events, so a
/// longer silence means the provider stalled.
pub(super) const RESPONSE_IDLE_TIMEOUT: Duration = Duration::from_mins(5);

const NO_SNAPSHOT: &str = "df: no policy snapshot yet";
const UNKNOWN_SOURCE: &str = "df: unknown source";
const NO_PROVIDER: &str = "df: no model provider for this run";
const NOT_FOUND: &str = "df: not found";
const CREDENTIAL_UNAVAILABLE: &str = "df: model provider credential unavailable";
const INVALID_URL: &str = "df: invalid model provider URL";
const UNREACHABLE: &str = "df: model provider unreachable";
const UPLOAD_TIMED_OUT: &str = "df: request body timed out";
const TIMED_OUT: &str = "df: model provider did not answer in time";

/// The inference endpoint of a gateway pod.
pub struct Inference {
    connector: UpstreamConnector,
    usage: Arc<dyn UsageSink>,
    metrics: Arc<Metrics>,
}

impl Inference {
    /// Forwards with `connector` (TLS roots, plain HTTP only where the instance allows it) and
    /// hands the usage of every measured request to `usage`.
    pub fn new(
        connector: UpstreamConnector,
        usage: Arc<dyn UsageSink>,
        metrics: Arc<Metrics>,
    ) -> Self {
        Self {
            connector,
            usage,
            metrics,
        }
    }

    /// Answers one request of the sandbox at `source`; `request.uri().path()` starts with
    /// [`route::PREFIX`].
    pub async fn handle<B>(
        &self,
        policy: Option<&Policy>,
        source: IpAddr,
        request: Request<B>,
    ) -> Response
    where
        B: hyper::body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        let Some(policy) = policy else {
            return text(StatusCode::SERVICE_UNAVAILABLE, NO_SNAPSHOT);
        };
        let Some(sandbox) = policy.sandbox(source) else {
            tracing::warn!(%source, "inference request denied: unknown source");
            return text(StatusCode::FORBIDDEN, UNKNOWN_SOURCE);
        };
        let Some(route) = policy.inference_route(source) else {
            tracing::warn!(run_id = %sandbox.run_id, "inference request denied: the run has no model provider");
            return text(StatusCode::FORBIDDEN, NO_PROVIDER);
        };
        let (parts, body) = request.into_parts();
        let path = parts
            .uri
            .path()
            .strip_prefix(route::PREFIX)
            .unwrap_or_default();
        let chosen =
            convert::wire_api_from_proto(route.wire_api).unwrap_or(WireApi::AnthropicMessages);
        let Some(target) = route::resolve(&parts.method, path, chosen) else {
            tracing::debug!(run_id = %sandbox.run_id, method = %parts.method, path, "inference path not served");
            return text(StatusCode::NOT_FOUND, NOT_FOUND);
        };
        let Some(base) = upstream_url(route, target.wire_api) else {
            tracing::debug!(run_id = %sandbox.run_id, wire_api = %target.wire_api, "wire API not offered by the provider");
            return text(StatusCode::NOT_FOUND, NOT_FOUND);
        };
        let Some(credential) = policy.provider_credential(&route.credential_ref) else {
            tracing::warn!(run_id = %sandbox.run_id, "provider credential missing from the snapshot");
            return text(StatusCode::BAD_GATEWAY, CREDENTIAL_UNAVAILABLE);
        };
        let Ok(headers) = upstream_headers(&parts.headers, route, &credential.value) else {
            tracing::warn!(run_id = %sandbox.run_id, "provider credential cannot be sent as header");
            return text(StatusCode::BAD_GATEWAY, CREDENTIAL_UNAVAILABLE);
        };
        let Some(url) = self.connector.url(base) else {
            tracing::warn!(run_id = %sandbox.run_id, "provider upstream URL is not usable");
            return text(StatusCode::BAD_GATEWAY, INVALID_URL);
        };
        let query = parts
            .uri
            .query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default();
        let target_uri = format!("{}{path}{query}", url.uri().path().trim_end_matches('/'));
        let Ok(target_uri) = target_uri.parse::<Uri>() else {
            return text(StatusCode::NOT_FOUND, NOT_FOUND);
        };

        let outcome = Outcome {
            sink: self.usage.clone(),
            metrics: self.metrics.clone(),
            run_id: sandbox.run_id.clone(),
            wire_api: target.wire_api,
            started: Instant::now(),
            measures_usage: target.measures_usage,
        };
        let upload = LimitedBody::new(
            body.map_err(Into::into).boxed_unsync(),
            u64::MAX,
            UPLOAD_IDLE_TIMEOUT,
        );
        let upload_limit = upload.limit();
        let mut outgoing = Request::new(upload.boxed_unsync());
        *outgoing.method_mut() = parts.method;
        *outgoing.uri_mut() = target_uri;
        *outgoing.headers_mut() = headers;
        if let Ok(host) = hyper::header::HeaderValue::from_str(url.authority()) {
            outgoing.headers_mut().insert(HOST, host);
        }
        self.send(&url, outgoing, upload_limit, outcome).await
    }

    /// Connects to the upstream, sends the request and relays the answer.
    async fn send(
        &self,
        url: &UpstreamUrl,
        request: Request<Body>,
        upload_limit: Arc<OnceLock<BodyLimit>>,
        outcome: Outcome,
    ) -> Response {
        let (mut sender, task) = match self.connector.open::<Body>(url).await {
            Ok(pair) => pair,
            Err(error) => return unreachable(url, outcome, &error.to_string()),
        };
        match tokio::time::timeout(RESPONSE_HEAD_TIMEOUT, sender.send_request(request)).await {
            Ok(Ok(response)) => relay(response, task, outcome),
            Ok(Err(_)) if upload_limit.get() == Some(&BodyLimit::TimedOut) => {
                // The sandbox's own upload stalled; the provider did nothing wrong.
                tracing::info!(run_id = %outcome.run_id, "request body from the sandbox stalled; upload ended");
                outcome.failed();
                text(StatusCode::REQUEST_TIMEOUT, UPLOAD_TIMED_OUT)
            }
            Ok(Err(error)) => unreachable(url, outcome, &error.to_string()),
            Err(_elapsed) => {
                tracing::warn!(
                    host = url.host(),
                    port = url.port(),
                    "model provider sent no response head in time"
                );
                outcome.failed();
                text(StatusCode::GATEWAY_TIMEOUT, TIMED_OUT)
            }
        }
    }
}

/// The upstream base URL of `wire_api` in `route`.
fn upstream_url(route: &InferenceRoute, wire_api: WireApi) -> Option<&str> {
    route
        .upstreams
        .iter()
        .find(|upstream| convert::wire_api_from_proto(upstream.wire_api) == Some(wire_api))
        .map(|upstream| upstream.upstream_url.as_str())
}

fn unreachable(url: &UpstreamUrl, outcome: Outcome, cause: &str) -> Response {
    tracing::warn!(
        host = url.host(),
        port = url.port(),
        cause,
        "model provider unreachable"
    );
    outcome.failed();
    text(StatusCode::BAD_GATEWAY, UNREACHABLE)
}

#[cfg(test)]
mod tests;
