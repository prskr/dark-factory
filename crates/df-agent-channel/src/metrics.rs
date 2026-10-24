//! Run metrics derived from the agent messages the control plane persists
//! (`specs/telemetry/spec.md`, design D4, D5).
//!
//! `persist_batch` collects [`Observation`]s while it applies messages and the session records
//! them after the transaction committed, so a replayed message or a rolled-back batch never
//! counts. Recording is best-effort: a crash between commit and recording loses the sample.
//!
//! Attribute values that come from the sandbox are enum names or go through [`BoundedLabel`].

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use df_proto::agent::v1::{PushStatus, PushTarget, RunOutcome, RuntimeOperation};
use df_proto::transcript::v1::{StopReason, ToolCallStatus, ToolKind, ToolOrigin, Usage};
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter, UpDownCounter};

/// Longest `model` label value in bytes.
const MODEL_MAX_BYTES: usize = 64;
/// Distinct `model` values per process; further ones are recorded as [`OVERFLOW_LABEL`].
const MODEL_MAX_VALUES: usize = 32;
const OVERFLOW_LABEL: &str = "other";
/// Longest plausible runtime operation; longer (and zero) durations are counted as rejected.
const MAX_RUNTIME_DURATION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Run durations: minutes to a day.
const RUN_DURATION_BOUNDS: [f64; 12] = [
    30.0, 60.0, 120.0, 300.0, 600.0, 1200.0, 1800.0, 3600.0, 7200.0, 14400.0, 28800.0, 86400.0,
];
/// Runtime operations: tens of milliseconds to hours.
const RUNTIME_DURATION_BOUNDS: [f64; 16] = [
    0.01, 0.05, 0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0,
    7200.0,
];

/// Something a persisted message says happened.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Observation {
    RunEnded {
        outcome: RunOutcome,
        duration: Duration,
    },
    Turn {
        stop_reason: StopReason,
        usage: Vec<Usage>,
    },
    ToolCall {
        kind: ToolKind,
        origin: ToolOrigin,
        status: ToolCallStatus,
    },
    Push {
        target: PushTarget,
        status: PushStatus,
    },
    Runtime {
        operation: RuntimeOperation,
        duration: Duration,
        ok: bool,
    },
}

/// The instruments of the agent channel. Created from the global meter provider, which the
/// binary registers before the channel starts; without one they do nothing.
#[derive(Debug)]
pub struct Metrics {
    runs_ended: Counter<u64>,
    run_duration: Histogram<f64>,
    turns: Counter<u64>,
    tokens: Counter<u64>,
    tool_calls: Counter<u64>,
    pushes: Counter<u64>,
    runtime_duration: Histogram<f64>,
    runtime_rejected: Counter<u64>,
    connections: UpDownCounter<i64>,
    reconnects: Counter<u64>,
    models: BoundedLabel,
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new(&opentelemetry::global::meter("df-agent-channel"))
    }
}

impl Metrics {
    pub fn new(meter: &Meter) -> Self {
        Self {
            runs_ended: meter
                .u64_counter("df.runs.ended")
                .with_description("Runs that ended")
                .build(),
            run_duration: meter
                .f64_histogram("df.run.duration")
                .with_unit("s")
                .with_description("Time from the start of a run to its report")
                .with_boundaries(RUN_DURATION_BOUNDS.to_vec())
                .build(),
            turns: meter
                .u64_counter("df.agent.turns")
                .with_description("Agent turns that ended")
                .build(),
            tokens: meter
                .u64_counter("df.agent.tokens")
                .with_description("Tokens used by the agent")
                .build(),
            tool_calls: meter
                .u64_counter("df.agent.tool_calls")
                .with_description("Tool calls that reached a terminal status")
                .build(),
            pushes: meter
                .u64_counter("df.agent.pushes")
                .with_description("Push attempts of the runtime")
                .build(),
            runtime_duration: meter
                .f64_histogram("df.agent.runtime.duration")
                .with_unit("s")
                .with_description("Duration of runtime operations, measured by the runtime")
                .with_boundaries(RUNTIME_DURATION_BOUNDS.to_vec())
                .build(),
            runtime_rejected: meter
                .u64_counter("df.agent.runtime.rejected")
                .with_description("Runtime timings with an implausible duration")
                .build(),
            connections: meter
                .i64_up_down_counter("df.agent.connections")
                .with_description("Open agent channel streams")
                .build(),
            reconnects: meter
                .u64_counter("df.agent.reconnects")
                .with_description("Streams of runs that had a stream before")
                .build(),
            models: BoundedLabel::new(MODEL_MAX_VALUES, MODEL_MAX_BYTES),
        }
    }

    /// Records `observations` of a run of `project`.
    pub(crate) fn record(&self, project: &str, observations: &[Observation]) {
        let project = KeyValue::new("project", project.to_string());
        for observation in observations {
            match observation {
                Observation::RunEnded { outcome, duration } => {
                    let attrs = [project.clone(), outcome_attr(*outcome)];
                    self.runs_ended.add(1, &attrs);
                    self.run_duration.record(duration.as_secs_f64(), &attrs);
                }
                Observation::Turn { stop_reason, usage } => {
                    let reason = label(stop_reason.as_str_name(), "STOP_REASON_");
                    self.turns
                        .add(1, &[project.clone(), KeyValue::new("stop_reason", reason)]);
                    for u in usage {
                        self.record_usage(&project, u);
                    }
                }
                Observation::ToolCall {
                    kind,
                    origin,
                    status,
                } => self.tool_calls.add(
                    1,
                    &[
                        project.clone(),
                        KeyValue::new("kind", label(kind.as_str_name(), "TOOL_KIND_")),
                        KeyValue::new("origin", label(origin.as_str_name(), "TOOL_ORIGIN_")),
                        KeyValue::new("status", label(status.as_str_name(), "TOOL_CALL_STATUS_")),
                    ],
                ),
                Observation::Push { target, status } => self.pushes.add(
                    1,
                    &[
                        project.clone(),
                        KeyValue::new("target", label(target.as_str_name(), "PUSH_TARGET_")),
                        KeyValue::new("status", label(status.as_str_name(), "PUSH_STATUS_")),
                    ],
                ),
                Observation::Runtime {
                    operation,
                    duration,
                    ok,
                } => self.record_runtime(&project, *operation, *duration, *ok),
            }
        }
    }

    fn record_usage(&self, project: &KeyValue, usage: &Usage) {
        let model = KeyValue::new("model", self.models.map(&usage.model_id));
        for (token_type, count) in [
            ("input", usage.input_tokens),
            ("output", usage.output_tokens),
            ("cache_read", usage.cache_read_tokens),
            ("cache_write", usage.cache_write_tokens),
        ] {
            if count > 0 {
                self.tokens.add(
                    count,
                    &[
                        project.clone(),
                        model.clone(),
                        KeyValue::new("token_type", token_type),
                    ],
                );
            }
        }
    }

    fn record_runtime(
        &self,
        project: &KeyValue,
        operation: RuntimeOperation,
        duration: Duration,
        ok: bool,
    ) {
        let operation = KeyValue::new(
            "operation",
            label(operation.as_str_name(), "RUNTIME_OPERATION_"),
        );
        if duration.is_zero() || duration > MAX_RUNTIME_DURATION {
            self.runtime_rejected.add(1, &[project.clone(), operation]);
            return;
        }
        let result = if ok { "ok" } else { "error" };
        self.runtime_duration.record(
            duration.as_secs_f64(),
            &[project.clone(), operation, KeyValue::new("result", result)],
        );
    }

    /// Counts an open stream of a run of `project` until the guard is dropped; `reconnect` if
    /// the run had a stream before.
    pub(crate) fn connection(&self, project: &str, reconnect: bool) -> ConnectionGuard {
        let attrs = vec![KeyValue::new("project", project.to_string())];
        self.connections.add(1, &attrs);
        if reconnect {
            self.reconnects.add(1, &attrs);
        }
        ConnectionGuard {
            connections: self.connections.clone(),
            attrs,
        }
    }
}

/// Decrements `df.agent.connections` when the stream ends.
#[derive(Debug)]
pub(crate) struct ConnectionGuard {
    connections: UpDownCounter<i64>,
    attrs: Vec<KeyValue>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.connections.add(-1, &self.attrs);
    }
}

fn outcome_attr(outcome: RunOutcome) -> KeyValue {
    KeyValue::new("outcome", label(outcome.as_str_name(), "RUN_OUTCOME_"))
}

/// The lower-case enum value name without its type prefix, e.g. `end_turn`.
fn label(name: &str, prefix: &str) -> String {
    name.strip_prefix(prefix).unwrap_or(name).to_lowercase()
}

/// Maps free-form values from the sandbox to a bounded set: values are cut to `max_bytes` and
/// after `capacity` distinct values everything new becomes `other`.
#[derive(Debug)]
struct BoundedLabel {
    seen: Mutex<HashSet<String>>,
    capacity: usize,
    max_bytes: usize,
}

impl BoundedLabel {
    fn new(capacity: usize, max_bytes: usize) -> Self {
        Self {
            seen: Mutex::new(HashSet::new()),
            capacity,
            max_bytes,
        }
    }

    fn map(&self, value: &str) -> String {
        let cut = (0..=self.max_bytes.min(value.len()))
            .rev()
            .find(|i| value.is_char_boundary(*i))
            .unwrap_or(0);
        let value = &value[..cut];
        if value.is_empty() {
            return "unspecified".to_string();
        }
        // A poisoned lock only means another thread panicked while holding it; the set is fine.
        let mut seen = self
            .seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if seen.contains(value) {
            value.to_string()
        } else if seen.len() < self.capacity {
            seen.insert(value.to_string());
            value.to_string()
        } else {
            OVERFLOW_LABEL.to_string()
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
pub mod probe {
    //! An in-memory metric pipeline to read back what was recorded.

    use std::collections::HashSet;

    use opentelemetry::metrics::{Meter, MeterProvider as _};
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
    use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};

    /// A meter provider with an in-memory exporter.
    #[allow(missing_debug_implementations)]
    pub struct Probe {
        provider: SdkMeterProvider,
        exporter: InMemoryMetricExporter,
    }

    impl Default for Probe {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Probe {
        pub fn new() -> Self {
            let exporter = InMemoryMetricExporter::default();
            let provider = SdkMeterProvider::builder()
                .with_reader(PeriodicReader::builder(exporter.clone()).build())
                .build();
            Self { provider, exporter }
        }

        /// A meter whose instruments report to this probe.
        pub fn meter(&self) -> Meter {
            self.provider.meter("test")
        }

        /// `(attributes, value)` of every data point of `name`; the value of a histogram point
        /// is its sample count.
        fn points(&self, name: &str) -> Vec<(Vec<(String, String)>, f64)> {
            self.exporter.reset();
            self.provider.force_flush().expect("flush metrics");
            let mut out = Vec::new();
            for resource in self
                .exporter
                .get_finished_metrics()
                .expect("finished metrics")
            {
                for scope in resource.scope_metrics() {
                    for metric in scope.metrics().filter(|m| m.name() == name) {
                        collect(metric.data(), &mut out);
                    }
                }
            }
            out
        }

        /// Sum of the values (histograms: sample counts) of the points of `name` whose
        /// attributes include all of `attrs`.
        pub fn total(&self, name: &str, attrs: &[(&str, &str)]) -> f64 {
            self.points(name)
                .into_iter()
                .filter(|(a, _)| {
                    attrs
                        .iter()
                        .all(|(k, v)| a.iter().any(|(ak, av)| ak == k && av == v))
                })
                .map(|(_, v)| v)
                .sum()
        }

        /// The values attribute `attr` takes over all points of `name`.
        pub fn values(&self, name: &str, attr: &str) -> HashSet<String> {
            self.points(name)
                .into_iter()
                .flat_map(|(a, _)| a.into_iter().filter(|(k, _)| k == attr).map(|(_, v)| v))
                .collect()
        }
    }

    fn collect(data: &AggregatedMetrics, out: &mut Vec<(Vec<(String, String)>, f64)>) {
        macro_rules! points {
            ($data:expr, $convert:expr) => {
                match $data {
                    MetricData::Sum(sum) => out.extend(
                        sum.data_points()
                            .map(|p| (attrs(p.attributes()), $convert(p.value()))),
                    ),
                    MetricData::Gauge(gauge) => out.extend(
                        gauge
                            .data_points()
                            .map(|p| (attrs(p.attributes()), $convert(p.value()))),
                    ),
                    MetricData::Histogram(h) => out.extend(
                        h.data_points()
                            .map(|p| (attrs(p.attributes()), p.count() as f64)),
                    ),
                    MetricData::ExponentialHistogram(_) => {}
                }
            };
        }
        match data {
            AggregatedMetrics::U64(d) => points!(d, |v: u64| v as f64),
            AggregatedMetrics::I64(d) => points!(d, |v: i64| v as f64),
            AggregatedMetrics::F64(d) => points!(d, |v: f64| v),
        }
    }

    fn attrs<'a>(
        attributes: impl Iterator<Item = &'a opentelemetry::KeyValue>,
    ) -> Vec<(String, String)> {
        attributes
            .map(|kv| (kv.key.to_string(), kv.value.to_string()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::probe::Probe;
    use super::*;

    fn usage(model: &str, input: u64, output: u64) -> Usage {
        Usage {
            model_id: model.into(),
            input_tokens: input,
            output_tokens: output,
            ..Usage::default()
        }
    }

    #[test]
    fn bounded_label_truncates_and_caps_distinct_values() {
        let label = BoundedLabel::new(2, 4);
        assert_eq!(label.map("abcdefgh"), "abcd");
        assert_eq!(label.map("abcdzzzz"), "abcd");
        assert_eq!(label.map("b"), "b");
        assert_eq!(label.map("c"), "other");
        assert_eq!(label.map("abcd"), "abcd");
        assert_eq!(label.map(""), "unspecified");
    }

    #[test]
    fn bounded_label_cuts_on_a_char_boundary() {
        let label = BoundedLabel::new(2, 4);
        // "é" is two bytes: the cut at byte 4 would split the second one.
        assert_eq!(label.map("aaéé"), "aaé");
    }

    #[test]
    fn token_usage_is_recorded_per_type() {
        let probe = Probe::new();
        Metrics::new(&probe.meter()).record(
            "DF",
            &[Observation::Turn {
                stop_reason: StopReason::EndTurn,
                usage: vec![usage("claude-sonnet-5-5", 100, 20)],
            }],
        );
        let model = ("model", "claude-sonnet-5-5");
        assert_eq!(
            probe.total("df.agent.tokens", &[model, ("token_type", "input")]),
            100.0
        );
        assert_eq!(
            probe.total("df.agent.tokens", &[model, ("token_type", "output")]),
            20.0
        );
        assert_eq!(
            probe.total("df.agent.tokens", &[("token_type", "cache_read")]),
            0.0
        );
        assert_eq!(
            probe.total(
                "df.agent.turns",
                &[("stop_reason", "end_turn"), ("project", "DF")]
            ),
            1.0
        );
    }

    #[test]
    fn model_flood_stays_bounded() {
        let probe = Probe::new();
        let usages: Vec<_> = (0..100)
            .map(|i| usage(&format!("model-{i}"), 1, 0))
            .collect();
        Metrics::new(&probe.meter()).record(
            "DF",
            &[Observation::Turn {
                stop_reason: StopReason::EndTurn,
                usage: usages,
            }],
        );
        assert_eq!(
            probe.values("df.agent.tokens", "model").len(),
            MODEL_MAX_VALUES + 1
        );
    }

    #[test]
    fn run_end_counts_outcome_and_duration() {
        let probe = Probe::new();
        Metrics::new(&probe.meter()).record(
            "DF",
            &[Observation::RunEnded {
                outcome: RunOutcome::Completed,
                duration: Duration::from_secs(90),
            }],
        );
        let attrs = [("outcome", "completed"), ("project", "DF")];
        assert_eq!(probe.total("df.runs.ended", &attrs), 1.0);
        assert_eq!(probe.total("df.run.duration", &attrs), 1.0);
    }

    #[test]
    fn tool_calls_and_pushes_use_enum_names() {
        let probe = Probe::new();
        Metrics::new(&probe.meter()).record(
            "DF",
            &[
                Observation::ToolCall {
                    kind: ToolKind::Execute,
                    origin: ToolOrigin::Harness,
                    status: ToolCallStatus::Completed,
                },
                Observation::Push {
                    target: PushTarget::TaskBranch,
                    status: PushStatus::RejectedNonFastForward,
                },
            ],
        );
        let call = [
            ("kind", "execute"),
            ("origin", "harness"),
            ("status", "completed"),
        ];
        assert_eq!(probe.total("df.agent.tool_calls", &call), 1.0);
        let push = [
            ("target", "task_branch"),
            ("status", "rejected_non_fast_forward"),
        ];
        assert_eq!(probe.total("df.agent.pushes", &push), 1.0);
    }

    #[test]
    fn implausible_runtime_durations_are_rejected() {
        let probe = Probe::new();
        let timing = |duration_ms| Observation::Runtime {
            operation: RuntimeOperation::Turn,
            duration: Duration::from_millis(duration_ms),
            ok: true,
        };
        Metrics::new(&probe.meter()).record(
            "DF",
            &[
                timing(1500),
                timing(0),
                timing(30 * 24 * 60 * 60 * 1000),
                Observation::Runtime {
                    operation: RuntimeOperation::Push,
                    duration: Duration::from_millis(20),
                    ok: false,
                },
            ],
        );
        let turn = [("operation", "turn")];
        assert_eq!(probe.total("df.agent.runtime.duration", &turn), 1.0);
        assert_eq!(probe.total("df.agent.runtime.rejected", &turn), 2.0);
        assert_eq!(
            probe.total(
                "df.agent.runtime.duration",
                &[("operation", "push"), ("result", "error")]
            ),
            1.0
        );
    }

    #[test]
    fn connections_are_balanced_and_reconnects_counted() {
        let probe = Probe::new();
        let metrics = Metrics::new(&probe.meter());
        let first = metrics.connection("DF", false);
        drop(first);
        let second = metrics.connection("DF", true);
        assert_eq!(probe.total("df.agent.connections", &[]), 1.0);
        drop(second);
        assert_eq!(probe.total("df.agent.connections", &[]), 0.0);
        assert_eq!(probe.total("df.agent.reconnects", &[]), 1.0);
    }
}
