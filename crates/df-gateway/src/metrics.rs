//! Gateway instruments (`specs/telemetry/spec.md`, "Gateway metrics").
//!
//! Destination hosts are never attributes; denial reasons are the fixed strings of the proxy.

use df_proto::gateway::v1::Verdict;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter, UpDownCounter};

/// The instruments of one gateway pod. [`Metrics::default`] takes them from the global meter
/// provider, which the binary registers before the gateway starts.
#[derive(Debug)]
pub struct Metrics {
    connections: Counter<u64>,
    active: UpDownCounter<i64>,
    bytes: Counter<u64>,
    policy_version: Gauge<u64>,
    inference_requests: Counter<u64>,
    inference_duration: Histogram<f64>,
    usage_dropped: Counter<u64>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new(&opentelemetry::global::meter("df-gateway"))
    }
}

impl Metrics {
    pub fn new(meter: &Meter) -> Self {
        Self {
            connections: meter
                .u64_counter("df.gateway.connections")
                .with_description("Proxy requests by final decision")
                .build(),
            active: meter
                .i64_up_down_counter("df.gateway.active_connections")
                .with_description("Requests that are connected to their destination")
                .build(),
            bytes: meter
                .u64_counter("df.gateway.bytes")
                .with_unit("By")
                .with_description("Bytes relayed between sandboxes and destinations")
                .build(),
            policy_version: meter
                .u64_gauge("df.gateway.policy_version")
                .with_description("Version of the last applied allowlist snapshot")
                .build(),
            inference_requests: meter
                .u64_counter("df.gateway.inference.requests")
                .with_description("Model requests forwarded by the inference endpoint")
                .build(),
            inference_duration: meter
                .f64_histogram("df.gateway.inference.duration")
                .with_unit("s")
                .with_description("Duration of model requests until their response ended")
                .build(),
            usage_dropped: meter
                .u64_counter("df.gateway.inference.usage_dropped")
                .with_description("Usage records dropped because the buffer was full or refused")
                .build(),
        }
    }

    /// A request is connected to its destination.
    pub(crate) fn opened(&self) {
        self.active.add(1, &[]);
    }

    /// A request ended; `opened` if [`Metrics::opened`] was called for it. `upstream` and
    /// `downstream` are the bytes from the sandbox to the destination and back.
    pub(crate) fn closed(
        &self,
        opened: bool,
        verdict: Verdict,
        reason: &str,
        upstream: u64,
        downstream: u64,
    ) {
        if opened {
            self.active.add(-1, &[]);
        }
        let decision = Decision::of(verdict);
        let reason = match decision {
            Decision::Denied => reason,
            Decision::Allowed => "none",
        };
        self.connections.add(
            1,
            &[
                KeyValue::new("decision", decision.as_str()),
                KeyValue::new("reason", reason.to_string()),
            ],
        );
        for (direction, count) in [("upstream", upstream), ("downstream", downstream)] {
            if count > 0 {
                self.bytes
                    .add(count, &[KeyValue::new("direction", direction)]);
            }
        }
    }

    /// A model request ended: `status_class` is `2xx`, `4xx`, `5xx` or `error`. Models, hosts and
    /// runs are no attributes.
    pub(crate) fn inference_finished(&self, wire_api: &str, status_class: &str, seconds: f64) {
        self.inference_requests.add(
            1,
            &[
                KeyValue::new("wire_api", wire_api.to_string()),
                KeyValue::new("status_class", status_class.to_string()),
            ],
        );
        self.inference_duration
            .record(seconds, &[KeyValue::new("wire_api", wire_api.to_string())]);
    }

    /// `count` usage records were dropped.
    pub(crate) fn usage_dropped(&self, count: u64) {
        self.usage_dropped.add(count, &[]);
    }

    /// A snapshot with `version` was applied.
    pub(crate) fn policy_applied(&self, version: u64) {
        self.policy_version.record(version, &[]);
    }
}

/// The final decision on a request, shared by the metrics and the `gateway.connect` span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Allowed,
    Denied,
}

impl Decision {
    pub(crate) fn of(verdict: Verdict) -> Self {
        match verdict {
            Verdict::Denied => Decision::Denied,
            _ => Decision::Allowed,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Decision::Allowed => "allowed",
            Decision::Denied => "denied",
        }
    }
}
