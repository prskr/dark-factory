//! Flow events: the in-memory ring buffer of a gateway pod and the filter of `Tail`.

use std::collections::{HashSet, VecDeque};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration as StdDuration, Instant};

use df_proto::gateway::v1::{FlowEvent, FlowFilter, Phase, Verdict};
use df_proto::pbjson_types::{Duration, Timestamp};
use tokio::sync::broadcast;

/// Events kept per gateway pod by default.
pub const DEFAULT_CAPACITY: usize = 10_000;
/// Events older than this are dropped by default.
pub const DEFAULT_MAX_AGE: StdDuration = StdDuration::from_secs(3600);
/// Shortest maximum age; a zero would drop every event.
pub const MIN_MAX_AGE: StdDuration = StdDuration::from_secs(1);
/// How often `record` scans a buffer for expired events.
const PRUNE_INTERVAL: StdDuration = StdDuration::from_secs(60);
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

/// The oldest `time` (in seconds) that is still young enough, for a maximum age of `max_age`.
pub fn cutoff(max_age: StdDuration) -> i64 {
    timestamp_now()
        .seconds
        .saturating_sub(i64::try_from(max_age.as_secs()).unwrap_or(i64::MAX))
}

/// Events in arrival order that drop the expired ones, at the latest on the first `prune_if_due`
/// after a minute and on every read.
#[derive(Default)]
pub struct AgedEvents {
    pub events: VecDeque<FlowEvent>,
    last_prune: Option<Instant>,
}

impl AgedEvents {
    /// Removes the expired events unless that happened less than a minute before `now`.
    pub fn prune_if_due(&mut self, cutoff: i64, now: Instant) {
        if self
            .last_prune
            .is_some_and(|last| now.duration_since(last) < PRUNE_INTERVAL)
        {
            return;
        }
        self.last_prune = Some(now);
        let open = open_flows(self.events.iter());
        self.events.retain(|e| !expired(e, cutoff, &open));
    }

    /// Whether `event`, which is not in the buffer yet, is expired on arrival: a late closing event
    /// is, the `opened` event of a connection that is still open is not.
    pub fn arrives_expired(&self, event: &FlowEvent, cutoff: i64) -> bool {
        // Only a late event needs the scan of the buffer.
        if event.time.as_ref().is_none_or(|t| t.seconds >= cutoff) {
            return false;
        }
        let open = open_flows(self.events.iter().chain(std::iter::once(event)));
        expired(event, cutoff, &open)
    }

    /// The events that are not expired, whatever their position.
    pub fn unexpired(&self, cutoff: i64) -> impl Iterator<Item = &FlowEvent> {
        let open = open_flows(self.events.iter());
        self.events
            .iter()
            .filter(move |e| !expired(e, cutoff, &open))
    }
}

/// Ring buffer of the latest events plus a channel with the live ones.
pub struct FlowLog {
    capacity: usize,
    max_age: StdDuration,
    buffer: Mutex<AgedEvents>,
    live: broadcast::Sender<FlowEvent>,
}

impl FlowLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            max_age: DEFAULT_MAX_AGE,
            buffer: Mutex::default(),
            live: broadcast::channel(LIVE_BACKLOG).0,
        }
    }

    /// Events older than `max_age` are dropped.
    #[must_use]
    pub fn with_max_age(mut self, max_age: StdDuration) -> Self {
        self.max_age = max_age.max(MIN_MAX_AGE);
        self
    }

    /// Stores the event, dropping the oldest one if the buffer is full, and sends it to the live
    /// subscribers.
    pub fn record(&self, event: FlowEvent) {
        self.record_at(event, cutoff(self.max_age), Instant::now());
    }

    fn record_at(&self, event: FlowEvent, cutoff: i64, now: Instant) {
        let mut buffer = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        buffer.prune_if_due(cutoff, now);
        if buffer.events.len() == self.capacity {
            buffer.events.pop_front();
        }
        buffer.events.push_back(event.clone());
        // Sent under the lock, so a subscriber sees every event exactly once, in the backlog or live.
        // An error only means nobody is subscribed.
        self.live.send(event).ok(); // receiver may be gone
    }

    /// The buffered events matching `filter` and a receiver of everything recorded from now on.
    pub fn subscribe(
        &self,
        filter: &FlowFilter,
    ) -> (Vec<FlowEvent>, broadcast::Receiver<FlowEvent>) {
        let buffer = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        let backlog = buffer
            .unexpired(cutoff(self.max_age))
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

/// Identifies a connection: flow ids are only unique per gateway pod.
type FlowKey = (String, u64);

fn flow_key(event: &FlowEvent) -> FlowKey {
    (event.gateway_pod.clone(), event.flow_id)
}

/// The connections whose `opened` event has no `closed` event in `events`.
fn open_flows<'a>(events: impl Iterator<Item = &'a FlowEvent> + Clone) -> HashSet<FlowKey> {
    let closed: HashSet<_> = events
        .clone()
        .filter(|e| e.phase() == Phase::Closed)
        .map(flow_key)
        .collect();
    events
        .filter(|e| e.phase() == Phase::Opened && e.flow_id != 0)
        .map(flow_key)
        .filter(|key| !closed.contains(key))
        .collect()
}

/// Whether `event` is too old: its time is before `cutoff`, unless it is the `opened` event of
/// a connection in `open`. Events without a time are never expired.
fn expired(event: &FlowEvent, cutoff: i64, open: &HashSet<FlowKey>) -> bool {
    event.time.as_ref().is_some_and(|t| t.seconds < cutoff)
        && !(event.phase() == Phase::Opened && open.contains(&flow_key(event)))
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

    fn aged(host: &str, phase: Phase, flow_id: u64, age: i64) -> FlowEvent {
        FlowEvent {
            host: host.into(),
            phase: phase.into(),
            flow_id,
            time: Some(Timestamp {
                seconds: timestamp_now().seconds - age,
                nanos: 0,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn expired_follows_the_cutoff_and_spares_open_tunnels() {
        let cutoff = 1000;
        let at = |seconds, phase: Phase, flow_id| FlowEvent {
            phase: phase.into(),
            flow_id,
            time: Some(Timestamp { seconds, nanos: 0 }),
            ..Default::default()
        };
        let events = [
            at(10, Phase::Opened, 1),
            at(10, Phase::Opened, 2),
            at(20, Phase::Closed, 2),
        ];
        let open = open_flows(events.iter());
        assert!(!expired(&events[0], cutoff, &open), "still open");
        assert!(expired(&events[1], cutoff, &open), "closed since");
        assert!(expired(&events[2], cutoff, &open), "closed events age");
        assert!(!expired(&at(1000, Phase::Closed, 3), cutoff, &open));
        assert!(!expired(&FlowEvent::default(), cutoff, &open), "no time");
    }

    #[test]
    fn an_expired_event_is_not_handed_out_wherever_it_sits() {
        let log = FlowLog::new(10).with_max_age(StdDuration::from_secs(3600));
        log.record(aged("new", Phase::Closed, 1, 10));
        log.record(aged("old", Phase::Closed, 2, 3660));
        log.record(aged("edge", Phase::Closed, 4, 3500));
        log.record(aged("new2", Phase::Closed, 3, 20));
        let (backlog, _) = log.subscribe(&FlowFilter::default());
        assert_eq!(hosts(&backlog), ["new", "edge", "new2"]);
    }

    #[test]
    fn a_still_open_tunnel_stays_in_the_backlog() {
        let log = FlowLog::new(10);
        log.record(aged("open", Phase::Opened, 1, 5400));
        log.record(aged("done", Phase::Opened, 2, 5400));
        log.record(aged("done", Phase::Closed, 2, 5000));
        let (backlog, _) = log.subscribe(&FlowFilter::default());
        assert_eq!(hosts(&backlog), ["open"]);
    }

    #[test]
    fn record_prunes_expired_events_once_a_minute() {
        let log = FlowLog::new(10);
        let t0 = Instant::now();
        let now = timestamp_now().seconds;
        let cutoff = now - 3600;
        log.record_at(aged("old", Phase::Closed, 1, 3660), cutoff, t0);
        log.record_at(
            aged("b", Phase::Closed, 2, 10),
            cutoff,
            t0 + StdDuration::from_secs(30),
        );
        assert_eq!(log.buffer.lock().unwrap().events.len(), 2, "not due yet");
        log.record_at(
            aged("c", Phase::Closed, 3, 10),
            cutoff,
            t0 + StdDuration::from_secs(61),
        );
        assert_eq!(
            hosts(
                &log.buffer
                    .lock()
                    .unwrap()
                    .events
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
            ),
            ["b", "c"]
        );
    }

    #[test]
    fn the_count_limit_drops_a_pinned_event_too() {
        let log = FlowLog::new(2);
        log.record(aged("open", Phase::Opened, 1, 5400));
        log.record(aged("a", Phase::Closed, 2, 1));
        log.record(aged("b", Phase::Closed, 3, 1));
        let (backlog, _) = log.subscribe(&FlowFilter::default());
        assert_eq!(hosts(&backlog), ["a", "b"]);
    }

    #[test]
    fn a_zero_maximum_age_is_raised_to_one_second() {
        let log = FlowLog::new(10).with_max_age(StdDuration::ZERO);
        log.record(aged("now", Phase::Closed, 1, 0));
        let (backlog, _) = log.subscribe(&FlowFilter::default());
        assert_eq!(hosts(&backlog), ["now"]);
    }
}
