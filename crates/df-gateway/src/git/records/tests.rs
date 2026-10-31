use std::sync::Mutex;

use df_proto::gateway::v1::gateway_git_server::{GatewayGit, GatewayGitServer};
use df_proto::gateway::v1::{PushVerdict, RefUpdate};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::*;
use crate::link::PlainConnector;

/// Fast retries, so a failing control plane does not slow the tests down.
pub(crate) const FAST: RetryPolicy = RetryPolicy {
    budget: Duration::from_millis(300),
    attempt_timeout: Duration::from_millis(200),
    initial_backoff: Duration::from_millis(10),
    max_backoff: Duration::from_millis(20),
};

/// A `GatewayGit` server on a loopback port that keeps every record it stores.
pub(crate) struct FakeControlPlane {
    pub(crate) address: String,
    pub(crate) records: Arc<Mutex<Vec<pb::PushRecord>>>,
    /// Calls still refused.
    pub(crate) failures: Arc<Mutex<usize>>,
    /// Status of a refused call.
    pub(crate) code: Arc<Mutex<tonic::Code>>,
    pub(crate) calls: Arc<Mutex<usize>>,
    stored: tokio::sync::watch::Receiver<usize>,
    server: JoinHandle<()>,
}

impl FakeControlPlane {
    /// Stores records after refusing the first `failures` calls with `UNAVAILABLE`.
    pub(crate) async fn start(failures: usize) -> FakeControlPlane {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let records = Arc::new(Mutex::new(Vec::new()));
        let failures = Arc::new(Mutex::new(failures));
        let code = Arc::new(Mutex::new(tonic::Code::Unavailable));
        let calls = Arc::new(Mutex::new(0));
        let (count, stored) = tokio::sync::watch::channel(0);
        let service = Store {
            records: records.clone(),
            failures: failures.clone(),
            code: code.clone(),
            calls: calls.clone(),
            count,
        };
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(GatewayGitServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        FakeControlPlane {
            address,
            records,
            failures,
            code,
            calls,
            stored,
            server,
        }
    }

    pub(crate) fn recorder(&self, policy: RetryPolicy) -> GrpcRecorder {
        GrpcRecorder::new(
            Arc::new(PlainConnector),
            self.address.clone(),
            "gateway-0".into(),
            policy,
        )
    }

    pub(crate) fn stored(&self) -> Vec<pb::PushRecord> {
        self.records.lock().unwrap().clone()
    }

    /// Waits until at least `count` records are stored.
    pub(crate) async fn wait_for(&self, count: usize) -> Vec<pb::PushRecord> {
        let mut stored = self.stored.clone();
        stored.wait_for(|n| *n >= count).await.unwrap();
        self.stored()
    }
}

impl Drop for FakeControlPlane {
    fn drop(&mut self) {
        self.server.abort();
    }
}

struct Store {
    records: Arc<Mutex<Vec<pb::PushRecord>>>,
    failures: Arc<Mutex<usize>>,
    code: Arc<Mutex<tonic::Code>>,
    calls: Arc<Mutex<usize>>,
    count: tokio::sync::watch::Sender<usize>,
}

#[tonic::async_trait]
impl GatewayGit for Store {
    async fn record_push(
        &self,
        request: Request<pb::PushRecord>,
    ) -> Result<Response<pb::RecordPushResponse>, Status> {
        *self.calls.lock().unwrap() += 1;
        {
            let mut failures = self.failures.lock().unwrap();
            if *failures > 0 {
                *failures -= 1;
                return Err(Status::new(*self.code.lock().unwrap(), "refused"));
            }
        }
        let count = {
            let mut records = self.records.lock().unwrap();
            records.push(request.into_inner());
            records.len()
        };
        self.count.send_replace(count);
        Ok(Response::new(pb::RecordPushResponse {}))
    }
}

/// A recorder for an address where nothing listens.
async fn unreachable(policy: RetryPolicy) -> GrpcRecorder {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    GrpcRecorder::new(
        Arc::new(PlainConnector),
        address,
        "gateway-0".into(),
        policy,
    )
}

fn record(push_id: Uuid, verdict: PushVerdict) -> PushRecord {
    PushRecord {
        push_id,
        run_id: "run-1".into(),
        refs: vec![RefUpdate {
            name: "refs/heads/df/task".into(),
            ..RefUpdate::default()
        }],
        verdict,
        bytes: 42,
        duration: Duration::from_millis(1500),
    }
}

#[tokio::test]
async fn every_state_of_a_push_is_stored_under_its_push_id() {
    let control_plane = FakeControlPlane::start(0).await;
    let recorder = control_plane.recorder(FAST);
    let push_id = Uuid::now_v7();

    recorder
        .record(record(push_id, PushVerdict::Forwarding))
        .await
        .unwrap();
    recorder
        .record(record(push_id, PushVerdict::Accepted))
        .await
        .unwrap();

    let stored = control_plane.stored();
    assert_eq!(stored.len(), 2);
    for (message, verdict) in stored
        .iter()
        .zip([PushVerdict::Forwarding, PushVerdict::Accepted])
    {
        assert_eq!(message.push_id, push_id.to_string());
        assert_eq!(message.verdict(), verdict);
        assert_eq!(message.gateway_pod, "gateway-0");
        assert_eq!(message.run_id, "run-1");
        assert_eq!(message.bytes, 42);
        assert_eq!(message.refs.len(), 1);
        let duration = message.duration.unwrap();
        assert_eq!((duration.seconds, duration.nanos), (1, 500_000_000));
    }
}

#[tokio::test]
async fn a_failing_attempt_is_retried_with_the_same_push_id() {
    let control_plane = FakeControlPlane::start(2).await;
    let recorder = control_plane.recorder(FAST);
    let push_id = Uuid::now_v7();

    recorder
        .record(record(push_id, PushVerdict::Forwarding))
        .await
        .unwrap();

    let stored = control_plane.stored();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].push_id, push_id.to_string());
}

#[tokio::test]
async fn a_forwarding_record_fails_when_the_control_plane_is_unreachable() {
    let recorder = unreachable(FAST).await;

    let result = recorder
        .record(record(Uuid::now_v7(), PushVerdict::Forwarding))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn a_rejected_record_fails_when_the_control_plane_keeps_failing() {
    let control_plane = FakeControlPlane::start(usize::MAX).await;
    let recorder = control_plane.recorder(FAST);

    let result = recorder
        .record(record(Uuid::now_v7(), PushVerdict::Rejected))
        .await;

    assert!(result.is_err());
    assert!(control_plane.stored().is_empty());
}

/// A connector whose connect never finishes, like a control plane that accepts and stays silent.
struct Silent;

#[async_trait]
impl GrpcConnector for Silent {
    async fn connect(
        &self,
        _authority: &str,
    ) -> Result<tonic::transport::Channel, crate::link::LinkError> {
        std::future::pending().await
    }
}

#[tokio::test(start_paused = true)]
async fn a_forwarding_record_fails_within_the_budget_when_the_control_plane_never_answers() {
    let policy = RetryPolicy::default();
    let recorder = GrpcRecorder::new(Arc::new(Silent), "cp:1".into(), "gateway-0".into(), policy);
    let started = Instant::now();

    let result = recorder
        .record(record(Uuid::now_v7(), PushVerdict::Forwarding))
        .await;

    assert!(result.is_err());
    assert!(
        started.elapsed() <= policy.budget,
        "took {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_final_record_that_fails_is_stored_later_from_the_queue() {
    // The first window fails; the queue gets the record through once the control plane is back.
    let control_plane = FakeControlPlane::start(1000).await;
    let policy = RetryPolicy {
        budget: Duration::from_millis(50),
        ..FAST
    };
    let recorder = control_plane.recorder(policy);
    let push_id = Uuid::now_v7();

    let answered = recorder
        .record(record(push_id, PushVerdict::Accepted))
        .await;
    assert!(answered.is_ok(), "a final record must not fail the push");
    assert!(control_plane.stored().is_empty());

    *control_plane.failures.lock().unwrap() = 0;
    let stored = control_plane.wait_for(1).await;
    assert_eq!(stored[0].push_id, push_id.to_string());
    assert_eq!(stored[0].verdict(), PushVerdict::Accepted);
}

#[tokio::test]
async fn a_record_refused_as_invalid_is_not_retried() {
    let control_plane = FakeControlPlane::start(usize::MAX).await;
    *control_plane.code.lock().unwrap() = tonic::Code::InvalidArgument;
    let recorder = control_plane.recorder(RetryPolicy::default());
    let started = Instant::now();

    let result = recorder
        .record(record(Uuid::now_v7(), PushVerdict::Rejected))
        .await;

    assert!(result.is_err());
    assert_eq!(*control_plane.calls.lock().unwrap(), 1);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn shutdown_stores_queued_records_when_the_control_plane_comes_back_in_time() {
    let control_plane = FakeControlPlane::start(usize::MAX).await;
    let policy = RetryPolicy {
        budget: Duration::from_millis(50),
        ..FAST
    };
    let recorder = Arc::new(control_plane.recorder(policy));
    let push_id = Uuid::now_v7();
    recorder
        .record(record(push_id, PushVerdict::Accepted))
        .await
        .unwrap();

    let shutdown = tokio::spawn({
        let recorder = recorder.clone();
        async move { recorder.shutdown(Duration::from_secs(10)).await }
    });
    *control_plane.failures.lock().unwrap() = 0;
    shutdown.await.unwrap();

    let stored = control_plane.stored();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].push_id, push_id.to_string());
}

#[tokio::test(start_paused = true)]
async fn shutdown_gives_up_after_the_grace_period() {
    let recorder = GrpcRecorder::new(
        Arc::new(Silent),
        "cp:1".into(),
        "gateway-0".into(),
        RetryPolicy::default(),
    );
    recorder
        .record(record(Uuid::now_v7(), PushVerdict::Accepted))
        .await
        .unwrap();
    let started = Instant::now();

    recorder.shutdown(Duration::from_secs(5)).await;

    assert!(started.elapsed() <= Duration::from_secs(6));
}
