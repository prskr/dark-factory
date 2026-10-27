//! The `Tail` relay: merges the flow events of all gateway pods for the viewers of the live
//! egress log (`design/egress.md` §8).
//!
//! A replica holds one `Tail` stream per gateway pod, shared by all its viewers, only while it
//! has viewers. Pods are found through the DNS name of the gateway's headless Service. Events of
//! all pods are kept in time order in a buffer, so a viewer that joins gets the recent history of
//! every pod in one sequence and then the live events.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use df_proto::gateway::v1::gateway_log_client::GatewayLogClient;
use df_proto::gateway::v1::{FlowEvent, TailRequest};
use df_proto::pbjson_types::Timestamp;
use futures::StreamExt;
use futures::stream::{self, BoxStream};
use tokio::sync::{broadcast, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tonic::transport::Channel;

use crate::flow::{AgedEvents, DEFAULT_MAX_AGE, MIN_MAX_AGE, cutoff};
use crate::link::{GrpcConnector, LinkError};

/// How often the gateway pods are looked up.
pub const DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);
/// Events kept for viewers that join later.
const BUFFER_EVENTS: usize = 10_000;
/// Live events a slow viewer may fall behind.
const LIVE_BACKLOG: usize = 1024;
/// How long, from the start of a session, its viewers wait for the buffered events of all pods
/// before they start anyway.
const BACKLOG_WAIT: Duration = Duration::from_secs(2);

/// How long the streams to the pods stay open after the last viewer left, so that the event
/// stream that follows a page load (which read the backlog and left) finds a warm session.
const SESSION_GRACE: Duration = Duration::from_secs(5);

/// Finds the gateway pods and opens channels to them.
#[async_trait]
pub trait GatewayDirectory: Send + Sync {
    /// `host:port` of every ready gateway pod.
    async fn pods(&self) -> io::Result<Vec<String>>;

    async fn connect(&self, pod: &str) -> Result<Channel, LinkError>;
}

/// Pods from the A records of the headless Service.
pub struct DnsDirectory {
    /// `host:port` that resolves to the pods' addresses.
    service: String,
    connector: Arc<dyn GrpcConnector>,
}

impl DnsDirectory {
    pub fn new(service_host: &str, port: u16, connector: Arc<dyn GrpcConnector>) -> Self {
        Self {
            service: format!("{service_host}:{port}"),
            connector,
        }
    }
}

#[async_trait]
impl GatewayDirectory for DnsDirectory {
    async fn pods(&self) -> io::Result<Vec<String>> {
        let mut pods: Vec<String> = tokio::net::lookup_host(&self.service)
            .await?
            .map(|address| address.to_string())
            .collect();
        pods.sort();
        pods.dedup();
        Ok(pods)
    }

    async fn connect(&self, pod: &str) -> Result<Channel, LinkError> {
        self.connector.connect(pod).await
    }
}

/// What viewers read: the events of all gateway pods.
pub struct Relay {
    directory: Arc<dyn GatewayDirectory>,
    interval: Duration,
    /// The streams to the gateway pods while viewers exist.
    session: Mutex<Weak<Session>>,
    /// Keeps the session alive for `SESSION_GRACE` after a viewer left; only the newest runs.
    linger: Arc<Mutex<Option<JoinHandle<()>>>>,
    grace: Duration,
    /// Events older than this are dropped from the merged buffer.
    max_age: Duration,
}

impl Relay {
    pub fn new(directory: Arc<dyn GatewayDirectory>, interval: Duration) -> Self {
        Self {
            directory,
            interval,
            session: Mutex::default(),
            linger: Arc::default(),
            grace: SESSION_GRACE,
            max_age: DEFAULT_MAX_AGE,
        }
    }

    /// Events older than `max_age` are dropped.
    #[must_use]
    pub fn with_max_age(mut self, max_age: Duration) -> Self {
        self.max_age = max_age.max(MIN_MAX_AGE);
        self
    }

    /// A viewer's stream: the buffered events of all gateway pods in time order, an event with
    /// `backlog_end` set, then live events. Dropping it ends the viewer; the streams to the pods close
    /// `SESSION_GRACE` after the last one left.
    pub fn subscribe(&self) -> BoxStream<'static, FlowEvent> {
        let session = {
            let mut current = self.session.lock().unwrap_or_else(PoisonError::into_inner);
            current.upgrade().unwrap_or_else(|| {
                let session = Session::start(self.directory.clone(), self.interval, self.max_age);
                *current = Arc::downgrade(&session);
                session
            })
        };
        // Owned by the stream, also while it still waits for the backlog.
        let lease = Lease {
            session,
            linger: self.linger.clone(),
            grace: self.grace,
        };
        stream::once(async move {
            lease.wait_for_backlog().await;
            let (backlog, live) = lease.merged.subscribe();
            let live = BroadcastStream::new(live).map(|received| match received {
                Ok(event) => event,
                Err(BroadcastStreamRecvError::Lagged(_)) => FlowEvent {
                    gap: true,
                    ..Default::default()
                },
            });
            // The session lives as long as the stream does.
            let backlog_end = FlowEvent {
                backlog_end: true,
                ..Default::default()
            };
            stream::iter(backlog)
                .chain(stream::once(std::future::ready(backlog_end)))
                .chain(live)
                .map(move |event| {
                    let _keep_session_open = &lease;
                    event
                })
        })
        .flatten()
        .boxed()
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(linger) = self
            .linger
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            linger.abort();
        }
    }
}

/// A viewer's hold on the session. When the viewer leaves, a task keeps the session open for
/// `SESSION_GRACE`; the relay owns that task and replaces it when the next viewer leaves.
struct Lease {
    session: Arc<Session>,
    linger: Arc<Mutex<Option<JoinHandle<()>>>>,
    grace: Duration,
}

impl std::ops::Deref for Lease {
    type Target = Session;

    fn deref(&self) -> &Session {
        &self.session
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        // Outside a runtime (shutdown) the session closes at once, without the grace period.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let session = self.session.clone();
        let grace = self.grace;
        let task = runtime.spawn(async move {
            tokio::time::sleep(grace).await;
            drop(session);
        });
        let mut linger = self.linger.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = linger.replace(task) {
            previous.abort();
        }
    }
}

/// The shared streams. Dropping it closes them.
struct Session {
    merged: Arc<Merged>,
    task: JoinHandle<()>,
    /// Viewers wait for the backlog until then; a viewer that joins later starts at once.
    backlog_deadline: tokio::time::Instant,
}

impl Session {
    fn start(
        directory: Arc<dyn GatewayDirectory>,
        interval: Duration,
        max_age: Duration,
    ) -> Arc<Self> {
        let merged = Arc::new(Merged::new(max_age));
        // The first discovery round counts as a pending backlog until it has started its pods.
        merged.pending.send_modify(|pending| *pending += 1);
        let task = tokio::spawn(follow_pods(directory, interval, merged.clone()));
        Arc::new(Self {
            merged,
            task,
            backlog_deadline: tokio::time::Instant::now() + BACKLOG_WAIT,
        })
    }

    async fn wait_for_backlog(&self) {
        let mut pending = self.merged.pending.subscribe();
        // A timeout is no failure: the viewer starts with what has arrived.
        let _ = tokio::time::timeout_at(
            self.backlog_deadline,
            pending.wait_for(|pending| *pending == 0),
        )
        .await;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Aborting the discovery task drops the pod streams it owns.
        self.task.abort();
    }
}

/// Events of all pods in time order, and the live channel.
struct Merged {
    buffer: Mutex<AgedEvents>,
    max_age: Duration,
    live: broadcast::Sender<FlowEvent>,
    /// Streams that have not yet delivered their buffered events.
    pending: watch::Sender<usize>,
}

impl Merged {
    fn new(max_age: Duration) -> Self {
        Self {
            buffer: Mutex::default(),
            max_age,
            live: broadcast::channel(LIVE_BACKLOG).0,
            pending: watch::channel(0).0,
        }
    }

    fn insert(&self, event: FlowEvent) {
        self.insert_at(event, cutoff(self.max_age), Instant::now());
    }

    fn insert_at(&self, event: FlowEvent, cutoff: i64, now: Instant) {
        let mut buffer = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        buffer.prune_if_due(cutoff, now);
        // A late event would reach the viewers that are connected but no later backlog.
        if buffer.arrives_expired(&event, cutoff) {
            return;
        }
        let buffer = &mut buffer.events;
        let key = time_key(&event);
        let start = buffer.partition_point(|e| time_key(e) < key);
        let end = buffer.partition_point(|e| time_key(e) <= key);
        // A full buffer drops its oldest events: one older than all of them (a replay after a
        // reopened stream) would be dropped at once and must not reach the viewers either.
        if buffer.len() >= BUFFER_EVENTS
            && buffer.front().is_some_and(|oldest| key < time_key(oldest))
        {
            return;
        }
        // A pod's stream that was reopened replays events that are already here.
        if buffer.range(start..end).any(|known| *known == event) {
            return;
        }
        buffer.insert(end, event.clone());
        if buffer.len() > BUFFER_EVENTS {
            buffer.pop_front();
        }
        // Sent under the lock, so a viewer sees every event once, in the backlog or live.
        // An error only means nobody is watching.
        let _ = self.live.send(event);
    }

    fn forward(&self, event: FlowEvent) {
        let _guard = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        // An error only means nobody is watching.
        let _ = self.live.send(event);
    }

    fn subscribe(&self) -> (Vec<FlowEvent>, broadcast::Receiver<FlowEvent>) {
        let buffer = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
        let backlog = buffer.unexpired(cutoff(self.max_age)).cloned().collect();
        (backlog, self.live.subscribe())
    }
}

fn time_key(event: &FlowEvent) -> (i64, i32) {
    let time = event.time.unwrap_or(Timestamp {
        seconds: 0,
        nanos: 0,
    });
    (time.seconds, time.nanos)
}

/// Keeps one `Tail` stream per gateway pod: opens streams for new pods, closes the ones of pods
/// that are gone and reopens streams that broke.
async fn follow_pods(
    directory: Arc<dyn GatewayDirectory>,
    interval: Duration,
    merged: Arc<Merged>,
) {
    // Dropping the set (this task was aborted) aborts the streams.
    let mut streams: JoinSet<()> = JoinSet::new();
    let mut open: HashMap<String, tokio::task::AbortHandle> = HashMap::new();
    let mut first_round = true;
    loop {
        match directory.pods().await {
            Ok(pods) => {
                open.retain(|pod, handle| {
                    let keep = pods.contains(pod) && !handle.is_finished();
                    if !keep {
                        handle.abort();
                    }
                    keep
                });
                for pod in pods {
                    open.entry(pod.clone()).or_insert_with(|| {
                        // Claimed here, before the task runs and before the first round ends:
                        // viewers wait for this pod's backlog from the start.
                        let backlog = Backlog::claim(&merged);
                        streams.spawn(pod_stream(directory.clone(), pod, backlog))
                    });
                }
            }
            Err(error) => tracing::warn!(%error, "could not look up the gateway pods"),
        }
        if first_round {
            first_round = false;
            merged.pending.send_modify(|pending| *pending -= 1);
        }
        while streams.try_join_next().is_some() {}
        tokio::time::sleep(interval).await;
    }
}

/// A pod's claim on the viewers' wait for buffered events. Released when the backlog arrived or
/// when the stream task ends or is dropped, even if it never ran.
struct Backlog {
    merged: Arc<Merged>,
    open: bool,
}

impl Backlog {
    fn claim(merged: &Arc<Merged>) -> Self {
        merged.pending.send_modify(|pending| *pending += 1);
        Self {
            merged: merged.clone(),
            open: true,
        }
    }

    fn done(&mut self) {
        if std::mem::take(&mut self.open) {
            self.merged.pending.send_modify(|pending| *pending -= 1);
        }
    }
}

impl Drop for Backlog {
    fn drop(&mut self) {
        self.done();
    }
}

async fn pod_stream(directory: Arc<dyn GatewayDirectory>, pod: String, mut backlog: Backlog) {
    let merged = backlog.merged.clone();
    let result = async {
        let channel = directory.connect(&pod).await?;
        let mut events = GatewayLogClient::new(channel)
            .tail(TailRequest::default())
            .await?
            .into_inner();
        while let Some(event) = events.message().await? {
            if event.backlog_end {
                backlog.done();
            } else if event.gap {
                merged.forward(event);
            } else {
                merged.insert(event);
            }
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    match result {
        Ok(()) => tracing::debug!(%pod, "tail stream ended"),
        Err(error) => tracing::warn!(%pod, %error, "tail stream failed"),
    }
}

#[cfg(test)]
mod tests;
