use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use df_proto::gateway::v1::Verdict;
use std::pin::Pin;
use std::task::{Context, Poll};

use df_proto::gateway::v1::gateway_log_server::{GatewayLog, GatewayLogServer};
use futures::Stream;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status};

use super::*;
use crate::flow::FlowLog;
use crate::link::PlainConnector;
use crate::tail;

/// A gateway pod's `Tail` server on a loopback port. It counts its open `Tail` streams.
struct Pod {
    address: String,
    flows: Arc<FlowLog>,
    open: Arc<watch::Sender<usize>>,
    server: JoinHandle<()>,
}

/// `Tail` of a pod that counts the streams it has open.
struct CountingTail {
    inner: tail::TailService,
    open: Arc<watch::Sender<usize>>,
}

/// Counts as open until dropped.
struct OpenStream {
    events: Pin<Box<dyn Stream<Item = Result<FlowEvent, Status>> + Send>>,
    open: Arc<watch::Sender<usize>>,
}

impl Stream for OpenStream {
    type Item = Result<FlowEvent, Status>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.events.as_mut().poll_next(cx)
    }
}

impl Drop for OpenStream {
    fn drop(&mut self) {
        self.open.send_modify(|open| *open -= 1);
    }
}

#[tonic::async_trait]
impl GatewayLog for CountingTail {
    type TailStream = OpenStream;

    async fn tail(&self, request: Request<TailRequest>) -> Result<Response<OpenStream>, Status> {
        let events = self.inner.tail(request).await?.into_inner();
        self.open.send_modify(|open| *open += 1);
        Ok(Response::new(OpenStream {
            events,
            open: self.open.clone(),
        }))
    }
}

impl Pod {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let flows = Arc::new(FlowLog::new(100));
        let open = Arc::new(watch::channel(0).0);
        let service = CountingTail {
            inner: tail::TailService::new(flows.clone()),
            open: open.clone(),
        };
        let server = tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_service(GatewayLogServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await;
        });
        Self {
            address,
            flows,
            open,
            server,
        }
    }

    fn record(&self, host: &str, second: i64) {
        self.flows.record(FlowEvent {
            host: host.into(),
            time: Some(Timestamp {
                seconds: second,
                nanos: 0,
            }),
            verdict: Verdict::Allowed.into(),
            ..Default::default()
        });
    }

    /// Waits until exactly `count` `Tail` streams are open on this pod.
    async fn wait_for_streams(&self, count: usize) {
        let mut open = self.open.subscribe();
        tokio::time::timeout(Duration::from_secs(5), open.wait_for(|open| *open == count))
            .await
            .unwrap_or_else(|_| panic!("{} does not have {count} Tail streams", self.address))
            .unwrap();
    }

    fn streams(&self) -> usize {
        *self.open.borrow()
    }
}

impl Drop for Pod {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// A directory whose pods the test changes.
#[derive(Default)]
struct FakeDirectory {
    pods: Mutex<Vec<String>>,
}

impl FakeDirectory {
    fn set(&self, pods: &[&Pod]) {
        *self.pods.lock().unwrap() = pods.iter().map(|pod| pod.address.clone()).collect();
    }
}

#[async_trait]
impl GatewayDirectory for FakeDirectory {
    async fn pods(&self) -> io::Result<Vec<String>> {
        Ok(self.pods.lock().unwrap().clone())
    }

    async fn connect(&self, pod: &str) -> Result<Channel, LinkError> {
        PlainConnector.connect(pod).await
    }
}

/// A relay that closes its streams as soon as the last viewer left.
fn relay(directory: &Arc<FakeDirectory>) -> Relay {
    let mut relay = Relay::new(directory.clone(), Duration::from_millis(20));
    relay.grace = Duration::ZERO;
    relay
}

async fn next_event(viewer: &mut BoxStream<'static, FlowEvent>) -> FlowEvent {
    tokio::time::timeout(Duration::from_secs(5), viewer.next())
        .await
        .expect("no event within 5 s")
        .expect("the viewer ended")
}

async fn next_host(viewer: &mut BoxStream<'static, FlowEvent>) -> String {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), viewer.next())
            .await
            .expect("no event within 5 s")
            .expect("the viewer ended");
        if !event.backlog_end {
            return event.host;
        }
    }
}

#[tokio::test]
async fn events_of_two_gateway_pods_form_one_time_ordered_stream() {
    let (a, b) = (Pod::start().await, Pod::start().await);
    a.record("a-1", 1);
    b.record("b-2", 2);
    a.record("a-3", 3);
    b.record("b-4", 4);
    let directory = Arc::new(FakeDirectory::default());
    directory.set(&[&a, &b]);
    let relay = relay(&directory);

    let mut viewer = relay.subscribe();
    let mut seen = Vec::new();
    for _ in 0..4 {
        seen.push(next_host(&mut viewer).await);
    }
    assert_eq!(seen, ["a-1", "b-2", "a-3", "b-4"]);

    // Live events of either pod follow.
    b.record("b-5", 5);
    assert_eq!(next_host(&mut viewer).await, "b-5");
    a.record("a-6", 6);
    assert_eq!(next_host(&mut viewer).await, "a-6");
}

#[tokio::test]
async fn a_second_viewer_gets_the_history_from_the_shared_streams() {
    let a = Pod::start().await;
    a.record("old", 1);
    let directory = Arc::new(FakeDirectory::default());
    directory.set(&[&a]);
    let relay = relay(&directory);

    let mut first = relay.subscribe();
    assert_eq!(next_host(&mut first).await, "old");
    a.record("new", 2);
    assert_eq!(next_host(&mut first).await, "new");

    let mut second = relay.subscribe();
    assert_eq!(next_host(&mut second).await, "old");
    assert_eq!(next_host(&mut second).await, "new");
    // One stream per pod, however many viewers.
    assert_eq!(a.streams(), 1);
}

#[tokio::test]
async fn streams_close_when_the_last_viewer_leaves() {
    let (a, b) = (Pod::start().await, Pod::start().await);
    a.record("a", 1);
    let directory = Arc::new(FakeDirectory::default());
    directory.set(&[&a, &b]);
    let relay = relay(&directory);

    let mut first = relay.subscribe();
    let mut second = relay.subscribe();
    assert_eq!(next_host(&mut first).await, "a");
    assert_eq!(next_host(&mut second).await, "a");
    assert_eq!((a.streams(), b.streams()), (1, 1));

    drop(first);
    // Another viewer is still there: nothing closes.
    a.record("a2", 2);
    assert_eq!(next_host(&mut second).await, "a2");
    assert_eq!((a.streams(), b.streams()), (1, 1));

    drop(second);
    a.wait_for_streams(0).await;
    b.wait_for_streams(0).await;

    // A new viewer opens new streams.
    let mut third = relay.subscribe();
    assert_eq!(next_host(&mut third).await, "a");
}

#[tokio::test]
async fn streams_follow_gateway_pods_appearing_and_disappearing() {
    let (a, b) = (Pod::start().await, Pod::start().await);
    let directory = Arc::new(FakeDirectory::default());
    directory.set(&[&a]);
    let relay = relay(&directory);
    let mut viewer = relay.subscribe();
    a.record("from-a", 1);
    assert_eq!(next_host(&mut viewer).await, "from-a");
    assert_eq!(b.streams(), 0);

    directory.set(&[&a, &b]);
    b.wait_for_streams(1).await;
    b.record("from-b", 2);
    assert_eq!(next_host(&mut viewer).await, "from-b");

    directory.set(&[&b]);
    a.wait_for_streams(0).await;
    assert_eq!(b.streams(), 1);
}

#[tokio::test]
async fn a_reopened_stream_does_not_repeat_known_events() {
    let a = Pod::start().await;
    a.record("once", 1);
    let directory = Arc::new(FakeDirectory::default());
    directory.set(&[&a]);
    let relay = relay(&directory);
    let mut viewer = relay.subscribe();
    assert_eq!(next_host(&mut viewer).await, "once");

    // The pod disappears from DNS and returns: its backlog is replayed.
    directory.set(&[]);
    a.wait_for_streams(0).await;
    directory.set(&[&a]);
    a.record("twice", 2);
    assert_eq!(next_host(&mut viewer).await, "twice");
}

#[tokio::test]
async fn buffer_keeps_events_in_time_order() {
    let merged = Merged::new();
    for second in [3, 1, 2] {
        merged.insert(FlowEvent {
            host: second.to_string(),
            time: Some(Timestamp {
                seconds: second,
                nanos: 0,
            }),
            ..Default::default()
        });
    }
    let (backlog, _) = merged.subscribe();
    let hosts: Vec<_> = backlog.iter().map(|e| e.host.as_str()).collect();
    assert_eq!(hosts, ["1", "2", "3"]);
}

#[tokio::test]
async fn a_viewer_gets_a_marker_between_the_backlog_and_live_events() {
    let a = Pod::start().await;
    a.record("old", 1);
    let directory = Arc::new(FakeDirectory::default());
    directory.set(&[&a]);
    let relay = relay(&directory);

    let mut viewer = relay.subscribe();
    let first = next_event(&mut viewer).await;
    assert_eq!(first.host, "old");
    assert!(next_event(&mut viewer).await.backlog_end);
    a.record("new", 2);
    assert_eq!(next_event(&mut viewer).await.host, "new");
}

#[test]
fn a_replayed_event_older_than_a_full_buffer_is_neither_stored_nor_sent() {
    let merged = Merged::new();
    let at = |second: i64| FlowEvent {
        host: second.to_string(),
        time: Some(Timestamp {
            seconds: second,
            nanos: 0,
        }),
        ..Default::default()
    };
    for second in 1000..1000 + BUFFER_EVENTS as i64 {
        merged.insert(at(second));
    }
    let (before, mut live) = merged.subscribe();
    assert_eq!(before.len(), BUFFER_EVENTS);

    merged.insert(at(5));
    let (after, _) = merged.subscribe();
    assert_eq!(after, before, "the buffer is unchanged");
    assert!(
        matches!(live.try_recv(), Err(broadcast::error::TryRecvError::Empty)),
        "viewers get no stale event"
    );

    // A newer event still evicts the oldest one and is sent.
    merged.insert(at(20_000));
    assert_eq!(live.try_recv().unwrap().host, "20000");
}

/// A directory whose connections wait until the test lets them proceed.
struct GatedDirectory {
    inner: FakeDirectory,
    gate: tokio::sync::Semaphore,
}

#[async_trait]
impl GatewayDirectory for GatedDirectory {
    async fn pods(&self) -> io::Result<Vec<String>> {
        self.inner.pods().await
    }

    async fn connect(&self, pod: &str) -> Result<Channel, LinkError> {
        self.gate.acquire().await.unwrap().forget();
        self.inner.connect(pod).await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_viewer_waits_for_the_buffered_events_of_pods_that_connect_late() {
    let (a, b) = (Pod::start().await, Pod::start().await);
    a.record("a-1", 1);
    b.record("b-2", 2);
    let inner = FakeDirectory::default();
    inner.set(&[&a, &b]);
    let directory = Arc::new(GatedDirectory {
        inner,
        gate: tokio::sync::Semaphore::new(0),
    });
    let relay = Relay::new(directory.clone(), Duration::from_millis(20));

    // The viewer subscribes while no pod stream is connected: it must wait for the backlog.
    let mut viewer = relay.subscribe();
    let reader = tokio::spawn(async move {
        let mut seen = Vec::new();
        while seen.len() < 3 {
            seen.push(next_event(&mut viewer).await);
        }
        seen
    });
    // Let both pods' connections proceed.
    directory.gate.add_permits(2);
    let seen = reader.await.unwrap();
    let hosts: Vec<_> = seen
        .iter()
        .map(|e| (e.host.as_str(), e.backlog_end))
        .collect();
    assert_eq!(
        hosts,
        [("a-1", false), ("b-2", false), ("", true)],
        "the backlog comes first, in time order"
    );
}

/// Counts the discovery rounds, i.e. the sessions (the interval is longer than any test).
#[derive(Default)]
struct CountingDirectory {
    lookups: AtomicUsize,
}

impl CountingDirectory {
    fn lookups(&self) -> usize {
        self.lookups.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl GatewayDirectory for CountingDirectory {
    async fn pods(&self) -> io::Result<Vec<String>> {
        self.lookups.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }

    async fn connect(&self, pod: &str) -> Result<Channel, LinkError> {
        PlainConnector.connect(pod).await
    }
}

/// Reads a viewer stream up to its `backlog_end` marker, then drops it.
async fn read_backlog_and_leave(relay: &Relay) {
    let mut viewer = relay.subscribe();
    while !next_event(&mut viewer).await.backlog_end {}
}

#[tokio::test(start_paused = true)]
async fn a_viewer_that_comes_back_within_the_grace_period_reuses_the_session() {
    let directory = Arc::new(CountingDirectory::default());
    let relay = Relay::new(directory.clone(), Duration::from_secs(3600));

    read_backlog_and_leave(&relay).await;
    tokio::time::sleep(SESSION_GRACE / 2).await;
    read_backlog_and_leave(&relay).await;

    assert_eq!(directory.lookups(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_viewer_that_comes_back_after_the_grace_period_starts_a_new_session() {
    let directory = Arc::new(CountingDirectory::default());
    let relay = Relay::new(directory.clone(), Duration::from_secs(3600));

    read_backlog_and_leave(&relay).await;
    tokio::time::sleep(SESSION_GRACE + Duration::from_secs(1)).await;
    read_backlog_and_leave(&relay).await;

    assert_eq!(directory.lookups(), 2);
}

#[tokio::test(start_paused = true)]
async fn the_streams_to_the_pods_stay_open_for_the_grace_period_and_serve_the_next_viewer() {
    let a = Pod::start().await;
    a.record("old", 1);
    let directory = Arc::new(FakeDirectory::default());
    directory.set(&[&a]);
    let relay = Relay::new(directory.clone(), Duration::from_secs(3600));

    let mut first = relay.subscribe();
    assert_eq!(next_host(&mut first).await, "old");
    drop(first);

    tokio::time::sleep(SESSION_GRACE / 2).await;
    assert_eq!(a.streams(), 1);
    // The warm session answers at once: no wait for the pods' backlog.
    let started = tokio::time::Instant::now();
    let mut second = relay.subscribe();
    assert_eq!(next_host(&mut second).await, "old");
    assert!(next_event(&mut second).await.backlog_end);
    assert_eq!(started.elapsed(), Duration::ZERO);
    drop(second);

    // The grace period restarts with the last viewer.
    tokio::time::sleep(SESSION_GRACE / 2 + Duration::from_secs(1)).await;
    assert_eq!(a.streams(), 1);
    tokio::time::sleep(SESSION_GRACE).await;
    a.wait_for_streams(0).await;
}

/// A directory whose discovery never finishes, so the session's backlog stays pending.
struct StuckDirectory;

#[async_trait]
impl GatewayDirectory for StuckDirectory {
    async fn pods(&self) -> io::Result<Vec<String>> {
        std::future::pending().await
    }

    async fn connect(&self, pod: &str) -> Result<Channel, LinkError> {
        PlainConnector.connect(pod).await
    }
}

#[tokio::test(start_paused = true)]
async fn a_viewer_that_comes_back_does_not_wait_for_the_backlog_again() {
    let relay = Relay::new(Arc::new(StuckDirectory), Duration::from_secs(3600));

    let started = tokio::time::Instant::now();
    read_backlog_and_leave(&relay).await;
    assert_eq!(started.elapsed(), BACKLOG_WAIT, "the first viewer waits");

    let started = tokio::time::Instant::now();
    read_backlog_and_leave(&relay).await;
    assert_eq!(
        started.elapsed(),
        Duration::ZERO,
        "the session already waited"
    );
}
