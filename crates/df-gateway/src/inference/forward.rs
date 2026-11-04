//! The upstream request and the relay of its answer (iter3-inference-gateway design D2, D3).

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use bytes::Bytes;
use df_core::model_provider::WireApi;
use df_proto::gateway::v1 as pb;
use http_body_util::BodyExt;
use hyper::Response;
use hyper::body::{Frame, Incoming};
use hyper::header::{CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap};

use super::RESPONSE_IDLE_TIMEOUT;
use super::headers::is_hop_by_hop;
use super::report::UsageSink;
use super::usage::{Measured, UsageParser};
use crate::convert::wire_api_to_proto;
use crate::metrics::Metrics;
use crate::upstream::{Body, BoxError, ConnectionTask, Guarded, LimitedBody};

/// What is needed to report a request when its response ended.
pub(super) struct Outcome {
    pub(super) sink: Arc<dyn UsageSink>,
    pub(super) metrics: Arc<Metrics>,
    pub(super) run_id: String,
    pub(super) wire_api: WireApi,
    pub(super) started: Instant,
    /// Whether the request is one with usage (`/v1/messages`, chat completions, responses).
    pub(super) measures_usage: bool,
}

impl Outcome {
    /// The request ended with `status` (0 if there was no answer) and `measured`.
    fn complete(self, status: u16, measured: Measured) {
        self.metrics.inference_finished(
            self.wire_api.as_str(),
            status_class(status),
            self.started.elapsed().as_secs_f64(),
        );
        if !self.measures_usage {
            return;
        }
        let usage = measured.usage.unwrap_or_default();
        self.sink.record(pb::UsageRecord {
            request_id: uuid::Uuid::now_v7().to_string(),
            run_id: self.run_id,
            at: Some(crate::flow::timestamp_now()),
            model: measured.model.unwrap_or_default(),
            wire_api: wire_api_to_proto(self.wire_api).into(),
            upstream_status: u32::from(status),
            usage_known: measured.usage.is_some(),
            input_tokens: usage.input,
            output_tokens: usage.output,
            cache_read_tokens: usage.cache_read,
            cache_write_tokens: usage.cache_write,
        });
    }

    /// The upstream could not be reached: an error without answer.
    pub(super) fn failed(self) {
        self.complete(0, Measured::default());
    }
}

/// The answer of the upstream for the harness, with the usage read from a copy of its body.
pub(super) fn relay(
    upstream: Response<Incoming>,
    task: ConnectionTask,
    outcome: Outcome,
) -> Response<Body> {
    let (parts, incoming) = upstream.into_parts();
    let parser = outcome.measures_usage.then(|| {
        UsageParser::new(
            outcome.wire_api,
            header_str(&parts.headers, &CONTENT_TYPE),
            header_str(&parts.headers, &CONTENT_ENCODING),
        )
    });
    let streamed = LimitedBody::new(
        incoming.map_err(BoxError::from).boxed_unsync(),
        u64::MAX,
        RESPONSE_IDLE_TIMEOUT,
    );
    let body = UsageBody {
        inner: Guarded::new(streamed.boxed_unsync(), task).boxed_unsync(),
        status: parts.status.as_u16(),
        parser,
        outcome: Some(outcome),
    };
    let mut response = Response::new(body.boxed_unsync());
    *response.status_mut() = parts.status;
    for (name, value) in &parts.headers {
        if !is_hop_by_hop(name) && name != CONTENT_LENGTH {
            response.headers_mut().append(name.clone(), value.clone());
        }
    }
    response
}

/// `2xx`, `4xx`, `5xx` or `error` (no answer); other classes count as `2xx`.
pub(super) fn status_class(status: u16) -> &'static str {
    match status {
        0 => "error",
        400..=499 => "4xx",
        500.. => "5xx",
        _ => "2xx",
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: &hyper::header::HeaderName) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// A response body that passes every frame on unchanged and feeds a copy to the usage parser. The
/// request is reported when the body ends, fails or is dropped (the harness went away).
struct UsageBody {
    /// The upstream's body, ending with an error when it stalls, and keeping the connection to
    /// the upstream alive while it streams.
    inner: Body,
    status: u16,
    parser: Option<UsageParser>,
    outcome: Option<Outcome>,
}

impl UsageBody {
    fn finish(&mut self) {
        let Some(outcome) = self.outcome.take() else {
            return;
        };
        let measured = self
            .parser
            .take()
            .map(UsageParser::finish)
            .unwrap_or_default();
        outcome.complete(self.status, measured);
    }
}

impl hyper::body::Body for UsageBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let polled = Pin::new(&mut self.inner).poll_frame(cx);
        match polled {
            Poll::Ready(Some(Ok(frame))) => {
                if let (Some(data), Some(parser)) = (frame.data_ref(), self.parser.as_mut()) {
                    parser.feed(data);
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(error))) => {
                self.finish();
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                self.finish();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for UsageBody {
    fn drop(&mut self) {
        self.finish();
    }
}
