//! Scripted egress events for local debugging of the network traffic page (`serve --demo-flows`,
//! feature `dev-tools`). Not part of a release build.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use df_proto::gateway::v1::{FlowEvent, Phase, Verdict};
use df_proto::pbjson_types::{Duration as ProtoDuration, Timestamp};
use df_web::egress::FlowSource;
use futures::StreamExt;
use futures::stream::{self, BoxStream};

/// Events of the 10,000-event mode: the size of a gateway pod's ring buffer.
const BULK_EVENTS: usize = 10_000;
/// Time between the live events that follow the backlog.
const LIVE_INTERVAL: Duration = Duration::from_secs(3);

/// Run id, task key, run number and project key.
const RUNS: [(&str, &str, u32, &str); 3] = [
    ("a4c2e7f0-1111-4a2b-9c3d-000000000001", "DF-7", 2, "DF"),
    ("b91d0c33-2222-4a2b-9c3d-000000000002", "DF-8", 1, "DF"),
    ("c07e55a1-3333-4a2b-9c3d-000000000003", "WEB-2", 1, "WEB"),
];

/// Host, port, method and what happens to it: `Some(rule)` is allowed by that rule, `None`
/// denied for the reason in the next field.
const TARGETS: [(&str, u32, Option<&str>, &str); 9] = [
    ("api.anthropic.com", 443, Some("api.anthropic.com"), ""),
    ("github.com", 443, Some("github.com"), ""),
    ("index.crates.io", 443, Some("*.crates.io"), ""),
    ("registry.npmjs.org", 443, Some("registry.npmjs.org"), ""),
    ("pypi.org", 443, Some("pypi.org"), ""),
    ("files.pythonhosted.org", 443, None, "not-allowlisted"),
    ("statsig.anthropic.com", 443, None, "not-allowlisted"),
    ("203.0.113.42", 22, None, "ip-literal"),
    ("intranet.example.com", 443, None, "private-destination"),
];

/// Which script `--demo-flows` plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum DemoMode {
    /// A mix of allowed, denied and open connections, a gap and several runs.
    Script,
    /// 10,000 events from a few runs, to see how the page copes with a full buffer.
    Bulk,
}

/// Replays a fixed backlog to every viewer, then adds an event every few seconds.
pub struct DemoFlows {
    backlog: Vec<FlowEvent>,
    interval: Duration,
}

impl DemoFlows {
    pub fn new(mode: DemoMode) -> Self {
        let count = match mode {
            DemoMode::Script => 40,
            DemoMode::Bulk => BULK_EVENTS,
        };
        let mut backlog: Vec<FlowEvent> = (0..count)
            .map(|index| event(index, (count - index) as i64 * 5))
            .collect();
        if mode == DemoMode::Script {
            // A tunnel that is still open, and a viewer that lost events.
            backlog.push(FlowEvent {
                phase: Phase::Opened.into(),
                duration: None,
                bytes_out: 0,
                bytes_in: 0,
                host: "ghcr.io".into(),
                matched_rule: "ghcr.io".into(),
                flow_id: 1_000,
                ..event(1, 2)
            });
            backlog.insert(
                count / 2,
                FlowEvent {
                    gap: true,
                    ..Default::default()
                },
            );
        }
        Self {
            backlog,
            interval: LIVE_INTERVAL,
        }
    }
}

impl FlowSource for DemoFlows {
    fn subscribe(&self) -> BoxStream<'static, FlowEvent> {
        let marker = FlowEvent {
            backlog_end: true,
            ..Default::default()
        };
        let interval = self.interval;
        let live = stream::unfold(self.backlog.len(), move |index| async move {
            tokio::time::sleep(interval).await;
            Some((event(index, 0), index + 1))
        });
        stream::iter(self.backlog.clone())
            .chain(stream::iter([marker]))
            .chain(live)
            .boxed()
    }
}

fn now() -> Timestamp {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp {
        seconds: i64::try_from(since_epoch.as_secs()).unwrap_or(0),
        nanos: 0,
    }
}

fn event(index: usize, seconds_ago: i64) -> FlowEvent {
    let (run_id, task_key, run_number, project_key) = RUNS[index % RUNS.len()];
    let (host, port, rule, reason) = TARGETS[(index * 7 + index / 3) % TARGETS.len()];
    let mut time = now();
    time.seconds -= seconds_ago;
    FlowEvent {
        time: Some(time),
        gateway_pod: "gateway-0".into(),
        source_ip: format!("10.42.0.{}", 10 + index % RUNS.len()),
        run_id: run_id.into(),
        task_key: task_key.into(),
        run_number,
        project_key: project_key.into(),
        method: "CONNECT".into(),
        host: host.into(),
        port,
        sni: if rule.is_some() {
            host.into()
        } else {
            String::new()
        },
        verdict: if rule.is_some() {
            Verdict::Allowed
        } else {
            Verdict::Denied
        }
        .into(),
        reason: reason.into(),
        matched_rule: rule.unwrap_or_default().into(),
        phase: Phase::Closed.into(),
        flow_id: index as u64 + 1,
        bytes_out: 1_200 + 37 * index as u64,
        bytes_in: 48_000 + 911 * index as u64,
        duration: Some(ProtoDuration {
            seconds: 1 + (index % 90) as i64,
            nanos: 250_000_000,
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn backlog_of(mode: DemoMode) -> Vec<FlowEvent> {
        let mut events = DemoFlows::new(mode).subscribe();
        let mut backlog = Vec::new();
        while let Some(event) = events.next().await {
            if event.backlog_end {
                break;
            }
            backlog.push(event);
        }
        backlog
    }

    #[tokio::test]
    async fn the_script_has_every_kind_of_event_and_several_runs() {
        let backlog = backlog_of(DemoMode::Script).await;
        assert!(backlog.iter().any(|e| e.gap));
        assert!(backlog.iter().any(|e| e.phase() == Phase::Opened && !e.gap));
        assert!(backlog.iter().any(|e| e.verdict() == Verdict::Denied));
        assert!(
            backlog
                .iter()
                .any(|e| e.verdict() == Verdict::Allowed && !e.matched_rule.is_empty())
        );
        let runs: std::collections::HashSet<_> = backlog.iter().map(|e| &e.run_id).collect();
        assert!(runs.len() >= 3);
    }

    #[tokio::test]
    async fn the_bulk_mode_fills_a_ring_buffer() {
        assert_eq!(backlog_of(DemoMode::Bulk).await.len(), BULK_EVENTS);
    }

    #[tokio::test]
    async fn live_events_follow_the_backlog() {
        let mut flows = DemoFlows::new(DemoMode::Script);
        flows.interval = Duration::ZERO;
        let mut events = flows.subscribe();
        loop {
            if events.next().await.is_some_and(|e| e.backlog_end) {
                break;
            }
        }
        let live = events.next().await.expect("a live event");
        assert!(!live.host.is_empty() && !live.backlog_end);
    }
}
