//! Flow events: the in-memory ring buffer of a gateway pod and the filter of `Tail`.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use df_proto::gateway::v1::{FlowEvent, FlowFilter, Verdict};
use df_proto::pbjson_types::{Duration, Timestamp};
use tokio::sync::broadcast;

/// Events kept per gateway pod by default.
pub const DEFAULT_CAPACITY: usize = 10_000;
/// Live events a slow `Tail` subscriber may fall behind before it loses some.
const LIVE_BACKLOG: usize = 1024;

/// The current time as the `time` of a flow event.
pub fn timestamp_now() -> Timestamp {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp {
        seconds: i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX),
        nanos: i32::try_from(since_epoch.subsec_nanos()).unwrap_or_default(),
    }
}

/// `elapsed` as the `duration` of a flow event.
pub fn duration_proto(elapsed: std::time::Duration) -> Duration {
    Duration {
        seconds: i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX),
        nanos: i32::try_from(elapsed.subsec_nanos()).unwrap_or_default(),
    }
}

/// Ring buffer of the latest events plus a channel with the live ones.
pub struct FlowLog {
    capacity: usize,
    buffer: Mutex<VecDeque<FlowEvent>>,
    live: broadcast::Sender<FlowEvent>,
}

impl FlowLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            buffer: Mutex::default(),
            live: broadcast::channel(LIVE_BACKLOG).0,
        }
    }

    /// Stores the event, dropping the oldest one if the buffer is full, and sends it to the live
    /// subscribers.
    pub fn record(&self, event: FlowEvent) {
        let mut buffer = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        if buffer.len() == self.capacity {
            buffer.pop_front();
        }
        buffer.push_back(event.clone());
        // Sent under the lock, so a subscriber sees every event exactly once, in the backlog or live.
        // An error only means nobody is subscribed.
        let _ = self.live.send(event);
    }

    /// The buffered events matching `filter` and a receiver of everything recorded from now on.
    pub fn subscribe(
        &self,
        filter: &FlowFilter,
    ) -> (Vec<FlowEvent>, broadcast::Receiver<FlowEvent>) {
        let buffer = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        let backlog = buffer
            .iter()
            .filter(|event| matches(filter, event))
            .cloned()
            .collect();
        (backlog, self.live.subscribe())
    }
}

/// Whether `event` passes `filter`; every set field must match.
pub fn matches(filter: &FlowFilter, event: &FlowEvent) -> bool {
    let wanted = |want: &str, have: &str| want.is_empty() || want == have;
    (filter.verdict() == Verdict::Unspecified || filter.verdict() == event.verdict())
        && wanted(&filter.project_key, &event.project_key)
        && wanted(&filter.task_key, &event.task_key)
        && wanted(&filter.persona_id, &event.persona_id)
        && event.host.contains(&filter.host)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(host: &str, verdict: Verdict) -> FlowEvent {
        FlowEvent {
            host: host.into(),
            verdict: verdict.into(),
            project_key: "DF".into(),
            ..Default::default()
        }
    }

    fn hosts(events: &[FlowEvent]) -> Vec<&str> {
        events.iter().map(|e| e.host.as_str()).collect()
    }

    #[test]
    fn the_oldest_event_is_dropped_when_the_buffer_is_full() {
        let log = FlowLog::new(3);
        for n in 1..=4 {
            log.record(event(&format!("h{n}"), Verdict::Allowed));
        }
        let (backlog, _) = log.subscribe(&FlowFilter::default());
        assert_eq!(hosts(&backlog), ["h2", "h3", "h4"]);
    }

    #[test]
    fn default_capacity_is_10000() {
        let log = FlowLog::new(DEFAULT_CAPACITY);
        for n in 0..=DEFAULT_CAPACITY {
            log.record(event(&n.to_string(), Verdict::Allowed));
        }
        let (backlog, _) = log.subscribe(&FlowFilter::default());
        assert_eq!(backlog.len(), 10_000);
        assert_eq!(backlog[0].host, "1");
    }

    #[test]
    fn filter_matches_every_set_field() {
        let denied = event("statsig.anthropic.com", Verdict::Denied);
        let only = |f: FlowFilter| matches(&f, &denied);
        assert!(only(FlowFilter::default()));
        assert!(only(FlowFilter {
            verdict: Verdict::Denied.into(),
            ..Default::default()
        }));
        assert!(!only(FlowFilter {
            verdict: Verdict::Allowed.into(),
            ..Default::default()
        }));
        assert!(only(FlowFilter {
            host: "anthropic".into(),
            project_key: "DF".into(),
            ..Default::default()
        }));
        assert!(!only(FlowFilter {
            project_key: "OTHER".into(),
            ..Default::default()
        }));
        assert!(!only(FlowFilter {
            task_key: "DF-1".into(),
            ..Default::default()
        }));
    }

    #[tokio::test]
    async fn subscribers_get_the_backlog_then_live_events_without_gaps_or_duplicates() {
        let log = FlowLog::new(10);
        log.record(event("old", Verdict::Allowed));
        let (backlog, mut live) = log.subscribe(&FlowFilter::default());
        log.record(event("new", Verdict::Allowed));
        assert_eq!(hosts(&backlog), ["old"]);
        assert_eq!(live.recv().await.unwrap().host, "new");
    }
}
