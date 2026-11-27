use std::sync::{Arc, Mutex};
use std::time::Duration;

use df_proto::gateway::v1::gateway_mcp_server::{GatewayMcp, GatewayMcpServer};
use df_telemetry::testing::Probe;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status};

use super::*;
use crate::batch::{BUFFER_CAPACITY, Timing};
use crate::link::PlainConnector;
use crate::metrics::Metrics;

const FAST: Timing = Timing {
    flush_interval: Duration::from_millis(20),
    initial_backoff: Duration::from_millis(10),
    max_backoff: Duration::from_millis(20),
};

/// A `GatewayMcp` server that refuses the first calls and then stores the records.
struct FakeControlPlane {
    records: Arc<Mutex<Vec<pb::McpCallRecord>>>,
    failures: Arc<Mutex<usize>>,
    stored: Arc<Notify>,
}

#[tonic::async_trait]
impl GatewayMcp for FakeControlPlane {
    async fn report(
        &self,
        request: Request<pb::McpCallBatch>,
    ) -> Result<Response<pb::ReportMcpCallsResponse>, Status> {
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
        Ok(Response::new(pb::ReportMcpCallsResponse {}))
    }
}

struct Server {
    address: String,
    records: Arc<Mutex<Vec<pb::McpCallRecord>>>,
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
            .add_service(GatewayMcpServer::new(service))
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

fn record(id: usize) -> pb::McpCallRecord {
    pb::McpCallRecord {
        call_id: format!("call-{id}"),
        run_id: "run-1".into(),
        ..pb::McpCallRecord::default()
    }
}

#[tokio::test]
async fn records_are_delivered_in_order_once_after_the_control_plane_failed_first() {
    let server = serve(3).await;
    let reporter = CallReporter::for_test_at(
        Arc::new(PlainConnector),
        server.address.clone(),
        Arc::default(),
        FAST,
        BUFFER_CAPACITY,
    );

    for id in 0..5 {
        reporter.record(record(id));
    }

    server.stored(5).await;
    let ids: Vec<_> = server
        .records
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.call_id.clone())
        .collect();
    assert_eq!(ids, ["call-0", "call-1", "call-2", "call-3", "call-4"]);
    server.task.abort();
}

#[tokio::test]
async fn a_full_buffer_drops_the_oldest_records_and_counts_them() {
    let probe = Probe::new();
    // Nothing listens: the records stay buffered.
    let reporter = CallReporter::for_test_at(
        Arc::new(PlainConnector),
        "127.0.0.1:9".into(),
        Arc::new(Metrics::new(&probe.meter())),
        FAST,
        3,
    );

    for id in 0..5 {
        reporter.record(record(id));
    }

    assert_eq!(probe.total("df.gateway.mcp.calls_dropped", &[]), 2.0);
    let kept: Vec<_> = reporter
        .buffered()
        .iter()
        .map(|r| r.call_id.clone())
        .collect();
    assert!(kept.len() <= 3, "{kept:?}");
    assert!(!kept.contains(&"call-0".to_string()), "{kept:?}");
}
