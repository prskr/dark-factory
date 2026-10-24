//! The `Tail` relay: merges the flow events of all gateway pods for the viewers of the live
//! egress log (`design/egress.md` §8).
//!
//! A replica holds one `Tail` stream per gateway pod, shared by all its viewers, only while it
//! has viewers. Pods are found through the DNS name of the gateway's headless Service. Events of
//! all pods are kept in time order in a buffer, so a viewer that joins gets the recent history of
//! every pod in one sequence and then the live events.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

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

use crate::link::{GrpcConnector, LinkError};

/// How often the gateway pods are looked up.
pub const DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);
/// Events kept for viewers that join later.
const BUFFER_EVENTS: usize = 10_000;
/// Live events a slow viewer may fall behind.
const LIVE_BACKLOG: usize = 1024;
/// How long a new viewer waits for the buffered events of all pods before it starts anyway.
const BACKLOG_WAIT: Duration = Duration::from_secs(2);

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
}

impl Relay {
    pub fn new(directory: Arc<dyn GatewayDirectory>, interval: Duration) -> Self {
        Self {
            directory,
            interval,
            session: Mutex::default(),
        }
    }

    /// A viewer's stream: the buffered events of all gateway pods in time order, an event with
    /// `backlog_end` set, then live events. Dropping it ends the viewer; the last one closes the streams to the pods.
    pub fn subscribe(&self) -> BoxStream<'static, FlowEvent> {
        let session = {
            let mut current = self.session.lock().unwrap_or_else(PoisonError::into_inner);
            current.upgrade().unwrap_or_else(|| {
                let session = Session::start(self.directory.clone(), self.interval);
                *current = Arc::downgrade(&session);
                session
            })
        };
        stream::once(async move {
            session.wait_for_backlog().await;
            let (backlog, live) = session.merged.subscribe();
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
                    let _keep_session_open = &session;
                    event
                })
        })
        .flatten()
        .boxed()
    }
}

/// The shared streams. Dropping it closes them.
struct Session {
    merged: Arc<Merged>,
    task: JoinHandle<()>,
}

impl Session {
    fn start(directory: Arc<dyn GatewayDirectory>, interval: Duration) -> Arc<Self> {
        let merged = Arc::new(Merged::new());
        // The first discovery round counts as a pending backlog until it has started its pods.
        merged.pending.send_modify(|pending| *pending += 1);
        let task = tokio::spawn(follow_pods(directory, interval, merged.clone()));
        Arc::new(Self { merged, task })
    }

    async fn wait_for_backlog(&self) {
        let mut pending = self.merged.pending.subscribe();
        // A timeout is no failure: the viewer starts with what has arrived.
        let _ = tokio::time::timeout(BACKLOG_WAIT, pending.wait_for(|pending| *pending == 0)).await;
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
    buffer: Mutex<VecDeque<FlowEvent>>,
    live: broadcast::Sender<FlowEvent>,
    /// Streams that have not yet delivered their buffered events.
    pending: watch::Sender<usize>,
}

impl Merged {
    fn new() -> Self {
        Self {
            buffer: Mutex::default(),
            live: broadcast::channel(LIVE_BACKLOG).0,
            pending: watch::channel(0).0,
        }
    }

    fn insert(&self, event: FlowEvent) {
        let mut buffer = self.buffer.lock().unwrap_or_else(PoisonError::into_inner);
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
        (buffer.iter().cloned().collect(), self.live.subscribe())
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
