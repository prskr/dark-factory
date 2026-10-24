//! Test fixtures: private PKI, a server on an ephemeral port, a scripted runtime client and
//! database seeding. Enabled by the `test-util` feature.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use df_core::testing::{FakeForgeClient, FakeSandboxProvider, FakeSecretStore};
use df_core::{PeerIdentity, PersonaId, ProjectId, RunId, TaskId};
use df_db::{NewTask, Notifier, TaskRow, create_task};
use df_proto::agent::v1::agent_channel_client::AgentChannelClient;
use df_proto::agent::v1::{AgentMessage, ControlMessage, Hello, agent_message};
use rcgen::string::Ia5String;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SanType,
};
use sqlx::PgPool;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};
use tonic::{Status, Streaming};

use crate::{ChannelConfig, ChannelContext, TlsReloader, serve};

/// Server name of the test server certificate.
pub const SERVER_NAME: &str = "localhost";
/// Pod SAN used by [`Env`] for its run.
pub const POD_SAN: &str = "spiffe://dark-factory/ns/sandboxes/pod/run-pod/uid-1";

/// A client certificate and its key.
pub struct ClientCert {
    pub cert_pem: String,
    pub key_pem: String,
    pub cert_der: Vec<u8>,
}

/// An instance CA with a server certificate for [`SERVER_NAME`].
pub struct TestPki {
    pub ca_pem: String,
    pub server_cert_pem: String,
    pub server_key_pem: String,
    issuer: Issuer<'static, KeyPair>,
}

fn ca_params(common_name: &str) -> CertificateParams {
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, common_name);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params
}

fn client_cert(issuer: &Issuer<'_, KeyPair>, uri_san: &str) -> anyhow_free::Result<ClientCert> {
    let mut params = CertificateParams::default();
    params
        .distinguished_name
        .push(DnType::CommonName, "sandbox");
    params.subject_alt_names = vec![SanType::URI(Ia5String::try_from(uri_san.to_string())?)];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let key = KeyPair::generate()?;
    let cert = params.signed_by(&key, issuer)?;
    Ok(ClientCert {
        cert_pem: cert.pem(),
        key_pem: key.serialize_pem(),
        cert_der: cert.der().to_vec(),
    })
}

/// `Result` with a boxed error, for fixtures.
pub mod anyhow_free {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
}

impl TestPki {
    pub fn new() -> anyhow_free::Result<Self> {
        let ca_params = ca_params("Dark Factory Test CA");
        let ca_key = KeyPair::generate()?;
        let ca_cert = ca_params.self_signed(&ca_key)?;
        let issuer = Issuer::new(ca_params, ca_key);

        let mut server = CertificateParams::new(vec![SERVER_NAME.to_string()])?;
        server.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_key = KeyPair::generate()?;
        let server_cert = server.signed_by(&server_key, &issuer)?;

        Ok(Self {
            ca_pem: ca_cert.pem(),
            server_cert_pem: server_cert.pem(),
            server_key_pem: server_key.serialize_pem(),
            issuer,
        })
    }

    /// A client certificate for a sandbox pod, signed by this CA.
    pub fn pod_client(&self, uri_san: &str) -> anyhow_free::Result<ClientCert> {
        client_cert(&self.issuer, uri_san)
    }

    /// A client certificate signed by an unrelated CA.
    pub fn rogue_client(uri_san: &str) -> anyhow_free::Result<ClientCert> {
        let params = ca_params("Rogue CA");
        let key = KeyPair::generate()?;
        params.self_signed(&key)?;
        client_cert(&Issuer::new(params, key), uri_san)
    }

    /// Like [`Self::write_agent_tls`], but the certificate is for `dns_name` and also valid for
    /// client authentication: the identity of a peer that is both client and server (gateway,
    /// control plane).
    pub fn write_peer_tls(
        &self,
        dir: &std::path::Path,
        dns_name: &str,
    ) -> anyhow_free::Result<df_core::config::AgentTlsConfig> {
        let mut params = CertificateParams::new(vec![dns_name.to_string()])?;
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        let key = KeyPair::generate()?;
        let cert = params.signed_by(&key, &self.issuer)?;
        let files = df_core::config::AgentTlsConfig {
            cert_file: dir.join("tls.crt"),
            key_file: dir.join("tls.key"),
            client_ca_file: dir.join("ca.crt"),
        };
        std::fs::write(&files.cert_file, cert.pem())?;
        std::fs::write(&files.key_file, key.serialize_pem())?;
        std::fs::write(&files.client_ca_file, &self.ca_pem)?;
        Ok(files)
    }

    /// Writes `tls.crt`, `tls.key` (server) and `ca.crt` into `dir` and returns the matching
    /// agent port configuration.
    pub fn write_agent_tls(
        &self,
        dir: &std::path::Path,
    ) -> std::io::Result<df_core::config::AgentTlsConfig> {
        let files = df_core::config::AgentTlsConfig {
            cert_file: dir.join("tls.crt"),
            key_file: dir.join("tls.key"),
            client_ca_file: dir.join("ca.crt"),
        };
        std::fs::write(&files.cert_file, &self.server_cert_pem)?;
        std::fs::write(&files.key_file, &self.server_key_pem)?;
        std::fs::write(&files.client_ca_file, &self.ca_pem)?;
        Ok(files)
    }
}

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
        let files = pki.write_agent_tls(dir.path())?;
        let tls = TlsReloader::load(&files)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?
            .check_every(Duration::ZERO);
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let (shutdown, stop) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = serve(ctx, listener, tls, async {
                let _ = stop.await;
            })
            .await;
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
            let _ = tx.send(());
        }
        tokio::time::timeout(within, &mut self.task).await.is_ok()
    }

    /// Replaces the served certificate, key and client CA, as cert-manager does on renewal.
    pub fn rotate(&self, pki: &TestPki) -> std::io::Result<()> {
        pki.write_agent_tls(self.dir.path()).map(|_| ())
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
            let _ = tx.send(());
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
        let _ = self.tx.send(msg).await;
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
            let msg = tokio::time::timeout(std::time::Duration::from_secs(10), self.recv())
                .await
                .map_err(|_| Status::deadline_exceeded("no matching control message"))??;
            let done = pred(&msg);
            seen.push(msg);
            if done {
                return Ok(seen);
            }
        }
    }
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
    /// Seeds project `DF`, persona "Linus" with Secret `claude/token`, and task `DF-1`.
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
        let persona_id = PersonaId::new();
        sqlx::query(
            "INSERT INTO personas (id, name, system_prompt, harness_credential_id)
             VALUES ($1, 'Linus', 'You are Linus.', $2)",
        )
        .bind(persona_id.as_uuid())
        .bind(claude_ref)
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
                created_by: None,
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
