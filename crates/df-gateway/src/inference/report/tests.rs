use std::sync::Mutex;

use df_agent_channel::metrics::probe::Probe;
use df_proto::gateway::v1::gateway_usage_server::{GatewayUsage, GatewayUsageServer};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

use super::*;
use crate::link::{LinkError, PlainConnector};

const FAST: Timing = Timing {
    flush_interval: Duration::from_millis(20),
    initial_backoff: Duration::from_millis(10),
    max_backoff: Duration::from_millis(20),
};

/// A `GatewayUsage` server that refuses the first calls and then stores the records.
struct FakeControlPlane {
    records: Arc<Mutex<Vec<pb::UsageRecord>>>,
    failures: Arc<Mutex<usize>>,
    /// Signals every stored batch.
    stored: Arc<Notify>,
}

#[tonic::async_trait]
impl GatewayUsage for FakeControlPlane {
    async fn report(
        &self,
        request: Request<pb::UsageBatch>,
    ) -> Result<Response<pb::ReportUsageResponse>, Status> {
        {
            let mut failures = self.failures.lock().unwrap();
            if *failures > 0 {
                *failures -= 1;
                return Err(Status::unavailable("restarting"));
            }
        }
        self.records
            .lock()
            .unwrap()
            .extend(request.into_inner().records);
        self.stored.notify_waiters();
        Ok(Response::new(pb::ReportUsageResponse {}))
    }
}

struct Server {
    address: String,
    records: Arc<Mutex<Vec<pb::UsageRecord>>>,
    stored: Arc<Notify>,
    task: JoinHandle<()>,
}

impl Server {
    /// Waits until `count` records were stored; the timeout only keeps a broken test from hanging.
    async fn stored(&self, count: usize) {
        let wait = async {
            loop {
                let stored = self.stored.notified();
                tokio::pin!(stored);
                stored.as_mut().enable();
                if self.records.lock().unwrap().len() >= count {
                    return;
                }
                stored.await;
            }
        };
        tokio::time::timeout(Duration::from_secs(10), wait)
            .await
            .expect("the records were not stored");
    }
}

async fn serve(failures: usize) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let records = Arc::new(Mutex::new(Vec::new()));
    let stored = Arc::new(Notify::new());
    let service = FakeControlPlane {
        records: records.clone(),
        failures: Arc::new(Mutex::new(failures)),
        stored: stored.clone(),
    };
    let task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(GatewayUsageServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    Server {
        address,
        records,
        stored,
        task,
    }
}

fn record(id: usize) -> pb::UsageRecord {
    pb::UsageRecord {
        request_id: format!("req-{id}"),
        run_id: "run-1".into(),
        ..pb::UsageRecord::default()
    }
}

fn reporter(address: &str, probe: &Probe, capacity: usize) -> UsageReporter {
    UsageReporter::for_test_at(
        Arc::new(PlainConnector),
        address.to_string(),
        Arc::new(Metrics::new(&probe.meter())),
        FAST,
        capacity,
    )
}

/// Fails the first `failures` connects, then connects like [`PlainConnector`]; every attempt is
/// signalled on `attempts`.
struct FlakyConnector {
    failures: Mutex<usize>,
    attempts: tokio::sync::mpsc::UnboundedSender<()>,
}

#[tonic::async_trait]
impl GrpcConnector for FlakyConnector {
    async fn connect(&self, authority: &str) -> Result<Channel, LinkError> {
        self.attempts.send(()).ok();
        {
            let mut failures = self.failures.lock().unwrap();
            if *failures > 0 {
                *failures -= 1;
                return Err(LinkError::Read {
                    path: "control plane".into(),
                    source: std::io::Error::other("not reachable yet"),
                });
            }
        }
        PlainConnector.connect(authority).await
    }
}

#[tokio::test]
async fn records_are_delivered_once_after_the_control_plane_failed_first() {
    let server = serve(3).await;
    let reporter = reporter(&server.address, &Probe::new(), BUFFER_CAPACITY);

    for id in 0..5 {
        reporter.record(record(id));
    }

    server.stored(5).await;
    let ids: Vec<_> = server
        .records
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.request_id.clone())
        .collect();
    assert_eq!(ids, ["req-0", "req-1", "req-2", "req-3", "req-4"]);
    server.task.abort();
}

#[tokio::test]
async fn records_wait_for_a_control_plane_that_is_not_reachable_yet() {
    let server = serve(0).await;
    let (attempts, mut tried) = tokio::sync::mpsc::unbounded_channel();
    let reporter = UsageReporter::for_test_at(
        Arc::new(FlakyConnector {
            failures: Mutex::new(2),
            attempts,
        }),
        server.address.clone(),
        Arc::default(),
        FAST,
        BUFFER_CAPACITY,
    );
    reporter.record(record(1));

    // Two failed attempts, then the one that gets through.
    for _ in 0..3 {
        tried.recv().await.unwrap();
    }
    server.stored(1).await;

    assert_eq!(server.records.lock().unwrap().len(), 1);
    server.task.abort();
}

#[tokio::test]
async fn a_full_buffer_drops_the_oldest_records_and_counts_them() {
    let probe = Probe::new();
    // Nothing listens: the records stay buffered.
    let reporter = reporter("127.0.0.1:9", &probe, 3);

    for id in 0..5 {
        reporter.record(record(id));
    }

    assert_eq!(probe.total("df.gateway.inference.usage_dropped", &[]), 2.0);
    let kept: Vec<_> = reporter
        .shared
        .queue
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.request_id.clone())
        .collect();
    assert!(kept.len() <= 3, "{kept:?}");
    assert!(!kept.contains(&"req-0".to_string()), "{kept:?}");
}

#[tokio::test]
async fn shutdown_reports_what_is_buffered() {
    let server = serve(0).await;
    let reporter = UsageReporter::for_test_at(
        Arc::new(PlainConnector),
        server.address.clone(),
        Arc::default(),
        Timing {
            flush_interval: Duration::from_secs(60),
            ..FAST
        },
        BUFFER_CAPACITY,
    );
    reporter.record(record(1));

    reporter.shutdown(Duration::from_secs(5)).await;

    assert_eq!(server.records.lock().unwrap().len(), 1);
    server.task.abort();
}
