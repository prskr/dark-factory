//! An in-memory metric pipeline to read back what was recorded.

use std::collections::HashSet;

use opentelemetry::metrics::{Meter, MeterProvider as _};
use opentelemetry_sdk::metrics::data::{
    AggregatedMetrics, MetricData, ResourceMetrics, ScopeMetrics,
};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};

/// A meter provider with an in-memory exporter.
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
        let finished = self
            .exporter
            .get_finished_metrics()
            .expect("finished metrics");
        let metrics = finished
            .iter()
            .flat_map(ResourceMetrics::scope_metrics)
            .flat_map(ScopeMetrics::metrics)
            .filter(|m| m.name() == name);
        for metric in metrics {
            collect(metric.data(), &mut out);
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

#[expect(
    clippy::cast_precision_loss,
    reason = "test probe: counts are compared as floats"
)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_filter_by_attributes_and_values_list_the_attribute() {
        let probe = Probe::new();
        let counter = probe.meter().u64_counter("df.test.calls").build();
        counter.add(2, &[opentelemetry::KeyValue::new("kind", "a")]);
        counter.add(3, &[opentelemetry::KeyValue::new("kind", "b")]);

        assert_eq!(probe.total("df.test.calls", &[]), 5.0);
        assert_eq!(probe.total("df.test.calls", &[("kind", "b")]), 3.0);
        assert_eq!(
            probe.values("df.test.calls", "kind"),
            HashSet::from(["a".to_owned(), "b".to_owned()])
        );
        assert_eq!(probe.total("df.test.missing", &[]), 0.0);
    }
}
