//! Test fixtures: a server on an ephemeral port (certificates from `df_tls::testing`), a scripted
//! runtime client and a context on top of `df_orchestrator::testing::Env`. Enabled by the
//! `test-util` feature.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::Duration;

use df_core::{PeerIdentity, RunId};
use df_proto::agent::v1::agent_channel_client::AgentChannelClient;
use df_proto::agent::v1::{AgentMessage, ControlMessage, Hello, agent_message};
use df_tls::TlsReloader;
use df_tls::testing::{ClientCert, SERVER_NAME, TestPki};
use sqlx::PgPool;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};
use tonic::{Status, Streaming};

pub use df_orchestrator::testing::{Fixture, TaskResumeState, recorded_events, task_resume_state};

use crate::{ChannelConfig, ChannelContext, serve};

/// Pod SAN used by [`Env`] for its run.
pub const POD_SAN: &str = "spiffe://dark-factory/ns/sandboxes/pod/run-pod/uid-1";

/// The agent port on an ephemeral loopback port, serving the TLS files of `dir`.
pub struct TestServer {
    pub addr: SocketAddr,
    dir: tempfile::TempDir,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl TestServer {
    pub async fn start(ctx: ChannelContext, pki: &TestPki) -> std::io::Result<Self> {
        let dir = tempfile::tempdir()?;
        let files = pki.write_server_tls(dir.path())?;
        let tls = TlsReloader::load(&files)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .check_every(Duration::ZERO);
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let (shutdown, stop) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            serve(ctx, listener, tls, async {
                stop.await.ok(); // a dropped server handle stops it as well
            })
            .await
            .ok(); // the test only observes whether the task ended
        });
        Ok(Self {
            addr,
            dir,
            shutdown: Some(shutdown),
            task,
        })
    }

    /// Stops the server gracefully; `false` if it was still running after `within`.
    pub async fn stop(&mut self, within: Duration) -> bool {
        if let Some(tx) = self.shutdown.take() {
            tx.send(()).ok(); // the server may have stopped already
        }
        tokio::time::timeout(within, &mut self.task).await.is_ok()
    }

    /// Replaces the served certificate, key and client CA, as cert-manager does on renewal.
    pub fn rotate(&self, pki: &TestPki) -> std::io::Result<()> {
        pki.write_server_tls(self.dir.path()).map(|_| ())
    }

    /// Opens a TLS channel; `client` is the certificate to present, if any.
    pub async fn channel(
        &self,
        pki: &TestPki,
        client: Option<&ClientCert>,
    ) -> Result<Channel, tonic::transport::Error> {
        Self::channel_to(self.addr, pki, client).await
    }

    /// Like [`Self::channel`] for an agent port started elsewhere.
    pub async fn channel_to(
        addr: SocketAddr,
        pki: &TestPki,
        client: Option<&ClientCert>,
    ) -> Result<Channel, tonic::transport::Error> {
        let mut tls = ClientTlsConfig::new()
            .domain_name(SERVER_NAME)
            .ca_certificate(Certificate::from_pem(&pki.ca_pem));
        if let Some(c) = client {
            tls = tls.identity(Identity::from_pem(&c.cert_pem, &c.key_pem));
        }
        Endpoint::from_shared(format!("https://{addr}"))?
            .tls_config(tls)?
            .connect()
            .await
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            tx.send(()).ok(); // the server may have stopped already
        }
        self.task.abort();
    }
}

/// A scripted runtime: sends messages with increasing `seq` and reads control messages.
pub struct RuntimeClient {
    tx: mpsc::Sender<AgentMessage>,
    rx: Streaming<ControlMessage>,
    next_seq: u64,
}

impl RuntimeClient {
    /// Opens `Connect` on `channel`. The server may reject the stream here.
    pub async fn open(channel: Channel) -> Result<Self, Status> {
        let (tx, rx) = mpsc::channel(64);
        let response = AgentChannelClient::new(channel)
            .max_decoding_message_size(32 * 1024 * 1024)
            .connect(ReceiverStream::new(rx))
            .await?;
        Ok(Self {
            tx,
            rx: response.into_inner(),
            next_seq: 1,
        })
    }

    /// Sends `Hello` (seq 0) and returns the `HelloAck`.
    pub async fn hello(&mut self) -> Result<ControlMessage, Status> {
        self.send_raw(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::Hello(Hello {
                runtime_version: "test".into(),
                ..Hello::default()
            })),
        })
        .await;
        self.recv().await
    }

    /// Sends `body` with the next sequence number and returns that number.
    pub async fn send(&mut self, body: agent_message::Body) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.send_raw(AgentMessage {
            seq,
            body: Some(body),
        })
        .await;
        seq
    }

    /// Sends a message exactly as given (replays, gaps, large payloads).
    pub async fn send_raw(&self, msg: AgentMessage) {
        // The stream may already be closed by the server; the test then observes the status.
        self.tx.send(msg).await.ok();
    }

    /// Next control message; an error status ends the stream.
    pub async fn recv(&mut self) -> Result<ControlMessage, Status> {
        match self.rx.message().await? {
            Some(msg) => Ok(msg),
            None => Err(Status::aborted("stream closed")),
        }
    }

    /// Reads control messages until `pred` accepts one; returns everything read.
    pub async fn recv_until(
        &mut self,
        pred: impl Fn(&ControlMessage) -> bool,
    ) -> Result<Vec<ControlMessage>, Status> {
        let mut seen = Vec::new();
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(10), self.recv())
                .await
                .map_err(|_elapsed| Status::deadline_exceeded("no matching control message"))??;
            let done = pred(&msg);
            seen.push(msg);
            if done {
                return Ok(seen);
            }
        }
    }
}

/// The seeded orchestration fixture (`provider`, `task`, `insert_run`, ... through `Deref`) and
/// the one [`ChannelContext`] wired to its fakes; the orchestration context is `ctx.orch`.
pub struct Env {
    pub ctx: ChannelContext,
    fixture: Fixture,
}

impl Deref for Env {
    type Target = Fixture;

    fn deref(&self) -> &Fixture {
        &self.fixture
    }
}

impl DerefMut for Env {
    fn deref_mut(&mut self) -> &mut Fixture {
        &mut self.fixture
    }
}

impl Env {
    /// Seeds like [`Fixture::seed`].
    pub async fn new(pool: PgPool) -> Self {
        let (fixture, orch) = Fixture::seed(pool).await;
        let ctx = ChannelContext {
            orch,
            config: Arc::new(ChannelConfig::default()),
            metrics: Arc::default(),
            approval_types: df_db::requests::ApprovalTypes::builtin(),
        };
        Self { ctx, fixture }
    }

    /// Turns lifecycle events on, see [`df_orchestrator::testing::with_events`].
    #[must_use]
    pub fn with_events(mut self) -> Self {
        df_orchestrator::testing::with_events(&mut self.ctx.orch);
        self
    }

    /// A second context on the same database, as another replica would have it.
    pub async fn other_replica(&self) -> ChannelContext {
        ChannelContext {
            orch: df_orchestrator::testing::other_replica(&self.ctx.orch).await,
            ..self.ctx.clone()
        }
    }

    /// Makes the provider resolve [`POD_SAN`] from loopback to `run_id`.
    pub fn bind_pod(&self, run_id: RunId) {
        self.provider.bind_identity(
            PeerIdentity {
                uri_san: POD_SAN.into(),
                peer_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            },
            run_id,
        );
    }

    /// A `connecting` run 1 whose pod ([`POD_SAN`] from loopback) the provider resolves.
    pub async fn running_run(&self) -> RunId {
        let run_id = self.insert_run(self.task.id, 1, "connecting").await;
        self.bind_pod(run_id);
        run_id
    }
}
