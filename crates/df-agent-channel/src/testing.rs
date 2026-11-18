//! Test fixtures: a server on an ephemeral port (certificates from `df_tls::testing`), a scripted
//! runtime client and database seeding. Enabled by the `test-util` feature.

#![expect(
    clippy::disallowed_methods,
    reason = "test fixtures seed the database directly (AGENTS.md, \"Database access\")"
)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use df_core::testing::{FakeForgeClient, FakeSandboxProvider, FakeSecretStore};
use df_core::{
    InstanceSubscription, LifecycleEventsConfig, PeerIdentity, PersonaId, ProjectId, RunId, TaskId,
};
use df_db::lifecycle_events::EventSinks;
use df_db::{NewTask, Notifier, TaskRow, create_task};
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

use crate::{ChannelConfig, ChannelContext, serve};

#[cfg(test)]
pub(crate) mod gated;

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

/// Status, resume time and resume count of a task, as stored.
#[derive(Debug, sqlx::FromRow)]
pub struct TaskResumeState {
    pub status: String,
    pub resume_at: Option<chrono::DateTime<chrono::Utc>>,
    pub auto_resumes: i32,
}

/// Reads the resume state of a task.
pub async fn task_resume_state(pool: &PgPool, task_id: TaskId) -> TaskResumeState {
    sqlx::query_as("SELECT status, resume_at, auto_resumes FROM tasks WHERE id = $1")
        .bind(task_id.as_uuid())
        .fetch_one(pool)
        .await
        .expect("read the resume state of a task")
}

/// The recorded lifecycle events (their envelopes) in the order they occurred.
pub async fn recorded_events(pool: &PgPool) -> Vec<serde_json::Value> {
    sqlx::query_scalar("SELECT payload FROM lifecycle_events ORDER BY occurred_at, id")
        .fetch_all(pool)
        .await
        .expect("read lifecycle events")
}

/// Seeded database, fakes and a context wired to them.
pub struct Env {
    pub ctx: ChannelContext,
    pub provider: FakeSandboxProvider,
    pub forge: FakeForgeClient,
    pub secrets: FakeSecretStore,
    pub project_id: ProjectId,
    pub persona_id: PersonaId,
    pub task: TaskRow,
}

impl Env {
    /// Seeds project `DF`, persona "Linus" with the model provider "Claude subscription"
    /// (Secret `claude/token`), and task `DF-1`.
    pub async fn new(pool: PgPool) -> Self {
        let provider = FakeSandboxProvider::new();
        let forge = FakeForgeClient::new();
        let secrets = FakeSecretStore::new();
        secrets.insert("claude", "token", "claude-secret-value");
        secrets.insert("forge", "token", "forge-secret-value");

        let (claude_ref, forge_ref) = (uuid::Uuid::now_v7(), uuid::Uuid::now_v7());
        for (id, name, secret) in [
            (claude_ref, "claude", "claude"),
            (forge_ref, "forge", "forge"),
        ] {
            sqlx::query(
                "INSERT INTO credential_refs (id, name, secret_name, secret_key) VALUES ($1, $2, $3, 'token')",
            )
            .bind(id)
            .bind(name)
            .bind(secret)
            .execute(&pool)
            .await
            .expect("seed credential_refs");
        }
        let project_id = ProjectId::new();
        sqlx::query(
            "INSERT INTO projects (id, key, name, repo_url, image, base_branch, forge_credential_id)
             VALUES ($1, 'DF', 'Dark Factory', 'https://git.example.org/peter/dark-factory.git',
                     'registry.example.org/df/project:latest', 'main', $2)",
        )
        .bind(project_id.as_uuid())
        .bind(forge_ref)
        .execute(&pool)
        .await
        .expect("seed project");
        let model_provider = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO model_providers (id, name, credential_ref_id, credential_scheme)
             VALUES ($1, 'Claude subscription', $2, 'claude-subscription')",
        )
        .bind(model_provider)
        .bind(claude_ref)
        .execute(&pool)
        .await
        .expect("seed model provider");
        sqlx::query(
            "WITH api AS (
                 INSERT INTO model_provider_apis (provider_id, wire_api, upstream_url)
                 VALUES ($1, 'anthropic-messages', 'https://api.anthropic.com')
                 RETURNING provider_id)
             INSERT INTO model_provider_models (provider_id, model_id, wire_api)
             SELECT provider_id, 'claude-sonnet-5-5', 'anthropic-messages' FROM api",
        )
        .bind(model_provider)
        .execute(&pool)
        .await
        .expect("seed model provider API and model");
        let persona_id = PersonaId::new();
        sqlx::query(
            "INSERT INTO personas (id, name, system_prompt, model, model_provider_id)
             VALUES ($1, 'Linus', 'You are Linus.', 'claude-sonnet-5-5', $2)",
        )
        .bind(persona_id.as_uuid())
        .bind(model_provider)
        .execute(&pool)
        .await
        .expect("seed persona");

        let (task, _) = create_task(
            &pool,
            NewTask {
                project_id,
                persona_id,
                title: Some("Fix login".into()),
                description: "Validate the OIDC state.".into(),
                issue_number: Some(42),
                created_by: df_core::Principal::system("test"),
                parent_id: None,
                depends_on: Vec::new(),
            },
        )
        .await
        .expect("seed task");

        let notifier = Arc::new(Notifier::start(&pool).await.expect("start notifier"));
        let ctx = ChannelContext {
            pool,
            notifier,
            provider: Arc::new(provider.clone()),
            forge: Arc::new(forge.clone()),
            secrets: Arc::new(secrets.clone()),
            config: Arc::new(ChannelConfig::default()),
            metrics: Arc::default(),
            approval_types: df_db::requests::ApprovalTypes::builtin(),
        };
        Self {
            ctx,
            provider,
            forge,
            secrets,
            project_id,
            persona_id,
            task,
        }
    }

    /// Turns lifecycle events on with one instance subscription to every event type.
    #[must_use]
    pub fn with_events(mut self) -> Self {
        let mut config = ChannelConfig::clone(&self.ctx.config);
        config.events = EventSinks::new(&LifecycleEventsConfig {
            signing_key_file: Some("signing.key".into()),
            subscriptions: vec![InstanceSubscription {
                url: "https://events.example.org/hook".into(),
                events: Vec::new(),
            }],
        });
        self.ctx.config = Arc::new(config);
        self
    }

    /// A second context on the same database, as another replica would have it.
    pub async fn other_replica(&self) -> ChannelContext {
        ChannelContext {
            notifier: Arc::new(
                Notifier::start(&self.ctx.pool)
                    .await
                    .expect("start notifier"),
            ),
            ..self.ctx.clone()
        }
    }

    /// Creates task `DF-2` in the seeded project, for the persona.
    pub async fn second_task(&self) -> TaskId {
        let (task, _) = create_task(
            &self.ctx.pool,
            NewTask {
                project_id: self.project_id,
                persona_id: self.persona_id,
                title: Some("Second".into()),
                description: String::new(),
                issue_number: None,
                created_by: df_core::Principal::system("test"),
                parent_id: None,
                depends_on: Vec::new(),
            },
        )
        .await
        .expect("create a second task");
        task.id
    }

    /// Creates a task of the seeded project on the existing `branch`, attached to `pr`.
    pub async fn task_on_branch(&self, branch: &str, pr: Option<df_db::AttachedPr>) -> TaskRow {
        let (task, _) = df_db::create_task_on(
            &self.ctx.pool,
            NewTask {
                project_id: self.project_id,
                persona_id: self.persona_id,
                title: Some("Existing branch".into()),
                description: String::new(),
                issue_number: None,
                created_by: df_core::Principal::system("test"),
                parent_id: None,
                depends_on: Vec::new(),
            },
            df_db::TaskPlacement {
                branch: Some(
                    df_core::ExistingBranch::parse(branch, "main").expect("a valid test branch"),
                ),
                attached_pr: pr,
            },
        )
        .await
        .expect("create a task on an existing branch");
        task
    }

    /// Creates a run for the task in `state` with pod IP `127.0.0.1`.
    pub async fn insert_run(&self, task_id: TaskId, number: i64, state: &str) -> RunId {
        let run_id = RunId::new();
        sqlx::query(
            "INSERT INTO runs (id, task_id, number, state, pod_ip) VALUES ($1, $2, $3, $4, '127.0.0.1')",
        )
        .bind(run_id.as_uuid())
        .bind(task_id.as_uuid())
        .bind(number)
        .bind(state)
        .execute(&self.ctx.pool)
        .await
        .expect("insert run");
        run_id
    }

    /// A board member as the actor of UI actions.
    pub fn human(&self) -> df_core::Principal {
        df_core::Principal::human(df_core::UserId::new(), "Ada")
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
