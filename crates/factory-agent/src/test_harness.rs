//! Test harness providing self-signed PKI with `rcgen` and an in-process scripted control plane.

use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use df_proto::agent::v1::agent_channel_server::{AgentChannel, AgentChannelServer};
use df_proto::agent::v1::{AgentMessage, ControlMessage};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::Stream;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use tonic::{Request, Response, Status, Streaming};

pub struct TestPki {
    pub ca_cert_pem: String,
    pub ca_key_pem: String,
    pub server_cert_pem: String,
    pub server_key_pem: String,
    pub server_cert_der: Vec<u8>,
    pub server_key_der: Vec<u8>,
    pub client_cert_pem: String,
    pub client_key_pem: String,
    pub server_name: String,
}

impl TestPki {
    /// Generate a fresh test PKI with instance CA, server cert for `server_name`, and pod client cert.
    pub fn generate(server_name: &str) -> anyhow::Result<Self> {
        // 1. Generate CA
        let mut ca_params = CertificateParams::new(Vec::default())?;
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Dark Factory Test Root CA");
        ca_params.key_usages.push(KeyUsagePurpose::DigitalSignature);
        ca_params.key_usages.push(KeyUsagePurpose::KeyCertSign);
        ca_params.key_usages.push(KeyUsagePurpose::CrlSign);

        let ca_key = KeyPair::generate()?;
        let ca_key_pem = ca_key.serialize_pem();
        let ca_cert = ca_params.self_signed(&ca_key)?;
        let issuer = Issuer::new(ca_params, ca_key);

        // 2. Generate Server Certificate
        let mut server_params = CertificateParams::new(vec![server_name.to_string()])?;
        server_params
            .distinguished_name
            .push(DnType::CommonName, server_name);
        server_params.use_authority_key_identifier_extension = true;
        server_params
            .key_usages
            .push(KeyUsagePurpose::DigitalSignature);
        server_params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ServerAuth);

        let server_key = KeyPair::generate()?;
        let server_cert = server_params.signed_by(&server_key, &issuer)?;

        // 3. Generate Client Certificate
        let mut client_params = CertificateParams::new(vec!["agent-sandbox".to_string()])?;
        client_params
            .distinguished_name
            .push(DnType::CommonName, "agent-sandbox");
        client_params.use_authority_key_identifier_extension = true;
        client_params
            .key_usages
            .push(KeyUsagePurpose::DigitalSignature);
        client_params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ClientAuth);

        let client_key = KeyPair::generate()?;
        let client_cert = client_params.signed_by(&client_key, &issuer)?;

        let server_key_der = server_key.serialize_der();
        let server_cert_der = server_cert.der().to_vec();

        Ok(Self {
            ca_cert_pem: ca_cert.pem(),
            ca_key_pem,
            server_cert_pem: server_cert.pem(),
            server_key_pem: server_key.serialize_pem(),
            server_cert_der,
            server_key_der,
            client_cert_pem: client_cert.pem(),
            client_key_pem: client_key.serialize_pem(),
            server_name: server_name.to_string(),
        })
    }

    /// Write client certificates into the specified directory (`ca.crt`, `tls.crt`, `tls.key`).
    pub fn write_client_tls_dir(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("ca.crt"), &self.ca_cert_pem)?;
        std::fs::write(dir.join("tls.crt"), &self.client_cert_pem)?;
        std::fs::write(dir.join("tls.key"), &self.client_key_pem)?;
        Ok(())
    }

    /// Generates an untrusted client cert and key (signed by an unknown rogue CA).
    pub fn generate_untrusted_client() -> anyhow::Result<(String, String)> {
        let mut ca_params = CertificateParams::new(Vec::default())?;
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Evil Rogue CA");
        ca_params.key_usages.push(KeyUsagePurpose::KeyCertSign);

        let ca_key = KeyPair::generate()?;
        let _ca_cert = ca_params.self_signed(&ca_key)?;
        let issuer = Issuer::new(ca_params, ca_key);

        let mut client_params = CertificateParams::new(vec!["untrusted-agent".to_string()])?;
        client_params
            .distinguished_name
            .push(DnType::CommonName, "untrusted-agent");
        client_params
            .key_usages
            .push(KeyUsagePurpose::DigitalSignature);
        client_params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ClientAuth);

        let client_key = KeyPair::generate()?;
        let client_cert = client_params.signed_by(&client_key, &issuer)?;

        Ok((client_cert.pem(), client_key.serialize_pem()))
    }

    /// Generates a valid server certificate for a different hostname signed by the instance CA.
    pub fn generate_server_cert_for_name(
        &self,
        other_name: &str,
    ) -> anyhow::Result<(String, String)> {
        // Re-construct issuer from ca_cert and ca_key
        let mut ca_params = CertificateParams::new(Vec::default())?;
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Dark Factory Test Root CA");
        ca_params.key_usages.push(KeyUsagePurpose::KeyCertSign);

        let mut other_params = CertificateParams::new(vec![other_name.to_string()])?;
        other_params
            .distinguished_name
            .push(DnType::CommonName, other_name);
        other_params
            .key_usages
            .push(KeyUsagePurpose::DigitalSignature);
        other_params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ServerAuth);

        let other_key = KeyPair::generate()?;
        // Generate a separate self-signed server cert representing an untrusted authority
        let other_cert = other_params.self_signed(&other_key)?;
        Ok((other_cert.pem(), other_key.serialize_pem()))
    }

    /// Generates a rotated client certificate signed by the instance CA.
    pub fn generate_rotated_client_cert(&self) -> anyhow::Result<(String, String)> {
        let mut ca_params = CertificateParams::new(Vec::default())?;
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(DnType::CommonName, "Dark Factory Test Root CA");
        ca_params.key_usages.push(KeyUsagePurpose::KeyCertSign);

        let ca_key = KeyPair::from_pem(&self.ca_key_pem)?;
        let issuer = Issuer::new(ca_params, ca_key);

        let mut client_params = CertificateParams::new(vec!["agent-sandbox-rotated".to_string()])?;
        client_params
            .distinguished_name
            .push(DnType::CommonName, "agent-sandbox-rotated");
        client_params.use_authority_key_identifier_extension = true;
        client_params
            .key_usages
            .push(KeyUsagePurpose::DigitalSignature);
        client_params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ClientAuth);

        let client_key = KeyPair::generate()?;
        let client_cert = client_params.signed_by(&client_key, &issuer)?;

        Ok((client_cert.pem(), client_key.serialize_pem()))
    }
}

type ActiveStreamTx = Arc<tokio::sync::Mutex<Option<mpsc::Sender<Result<ControlMessage, Status>>>>>;

struct MockService {
    agent_tx: mpsc::Sender<AgentMessage>,
    active_stream_tx: ActiveStreamTx,
    connection_count: Arc<std::sync::atomic::AtomicUsize>,
}

#[tonic::async_trait]
impl AgentChannel for MockService {
    type ConnectStream =
        Pin<Box<dyn Stream<Item = Result<ControlMessage, Status>> + Send + 'static>>;

    async fn connect(
        &self,
        request: Request<Streaming<AgentMessage>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        self.connection_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut inbound = request.into_inner();
        let agent_tx = self.agent_tx.clone();

        tokio::spawn(async move {
            while let Ok(Some(msg)) = inbound.message().await {
                if agent_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let (tx, rx) = mpsc::channel(100);
        let mut lock = self.active_stream_tx.lock().await;
        *lock = Some(tx);

        let out_stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(out_stream)))
    }
}

pub struct MockControlPlane {
    pub addr: SocketAddr,
    pub agent_rx: mpsc::Receiver<AgentMessage>,
    active_stream_tx: ActiveStreamTx,
    pub connection_count: Arc<std::sync::atomic::AtomicUsize>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl MockControlPlane {
    /// Start a scripted tonic mTLS control plane server on an ephemeral port.
    pub async fn start(
        server_cert_pem: &str,
        server_key_pem: &str,
        client_ca_pem: &str,
    ) -> anyhow::Result<Self> {
        let (agent_tx, agent_rx) = mpsc::channel(100);
        let active_stream_tx = Arc::new(tokio::sync::Mutex::new(None));
        let connection_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let service = MockService {
            agent_tx,
            active_stream_tx: Arc::clone(&active_stream_tx),
            connection_count: Arc::clone(&connection_count),
        };

        let server_identity =
            Identity::from_pem(server_cert_pem.as_bytes(), server_key_pem.as_bytes());
        let client_ca = Certificate::from_pem(client_ca_pem.as_bytes());

        let tls_config = ServerTlsConfig::new()
            .identity(server_identity)
            .client_ca_root(client_ca)
            .client_auth_optional(false);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        tokio::spawn(async move {
            let _ = Server::builder()
                .tls_config(tls_config)
                .expect("valid server tls config")
                .add_service(AgentChannelServer::new(service))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        Ok(Self {
            addr,
            agent_rx,
            active_stream_tx,
            connection_count,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub async fn recv_agent_message(&mut self) -> Option<AgentMessage> {
        self.agent_rx.recv().await
    }

    pub async fn send_control_message(&self, msg: ControlMessage) -> anyhow::Result<()> {
        let lock = self.active_stream_tx.lock().await;
        if let Some(tx) = lock.as_ref() {
            let res: Result<ControlMessage, Status> = Ok(msg);
            tx.send(res)
                .await
                .map_err(|_| anyhow::anyhow!("stream closed"))
        } else {
            Err(anyhow::anyhow!("no active stream"))
        }
    }

    /// Drops the active stream from the server side to simulate network drops or control plane restarts.
    pub async fn drop_active_stream(&self) {
        let mut lock = self.active_stream_tx.lock().await;
        *lock = None;
    }

    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// A lightweight in-process HTTPS server wrapping `git http-backend` for testing.
pub struct LocalHttpsGitServer {
    pub addr: SocketAddr,
    pub ca_cert_pem: String,
    pub server_name: String,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl LocalHttpsGitServer {
    /// Starts an HTTPS test server serving Git repositories under `git_project_root`.
    ///
    /// If `expected_auth` is `Some((user, pass))`, requests without matching Basic auth
    /// receive `401 Unauthorized` with `WWW-Authenticate: Basic realm="Git Access"`.
    pub fn start(
        pki: &TestPki,
        git_project_root: std::path::PathBuf,
        expected_auth: Option<(String, String)>,
    ) -> anyhow::Result<Self> {
        let cert = rustls::pki_types::CertificateDer::from(pki.server_cert_der.clone());
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(pki.server_key_der.clone()),
        );

        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)?;
        let server_config = Arc::new(server_config);

        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        let expected_auth_header = expected_auth.map(|(u, p)| {
            let raw = format!("{u}:{p}");
            format!("Basic {}", simple_base64_encode(raw.as_bytes()))
        });

        std::thread::spawn(move || {
            loop {
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }
                match listener.accept() {
                    Ok((mut sock, _)) => {
                        let config = Arc::clone(&server_config);
                        let root = git_project_root.clone();
                        let expected = expected_auth_header.clone();
                        std::thread::spawn(move || {
                            let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(10)));
                            let _ =
                                sock.set_write_timeout(Some(std::time::Duration::from_secs(10)));
                            let _ = sock.set_nonblocking(false);
                            if let Ok(mut conn) = rustls::ServerConnection::new(config) {
                                let mut stream = rustls::Stream::new(&mut conn, &mut sock);
                                let _ = handle_git_http_request(
                                    &mut stream,
                                    &root,
                                    expected.as_deref(),
                                );
                                // Responses end at EOF: GnuTLS clients reject it without close_notify.
                                // Best effort, the client may already be gone.
                                conn.send_close_notify();
                                let _ = conn.complete_io(&mut sock);
                            }
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            addr,
            ca_cert_pem: pki.ca_cert_pem.clone(),
            server_name: pki.server_name.clone(),
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

fn simple_base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < data.len() {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < data.len() {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

fn handle_git_http_request<S: std::io::Read + std::io::Write>(
    stream: &mut S,
    project_root: &Path,
    expected_auth_header: Option<&str>,
) -> std::io::Result<()> {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Read, Write};

    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0];
    let full_uri = parts[1];

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    // Check Authentication
    if let Some(expected) = expected_auth_header {
        let auth = headers
            .get("authorization")
            .map(String::as_str)
            .unwrap_or("");
        if auth != expected {
            let resp = b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"Git Access\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let stream = reader.get_mut();
            stream.write_all(resp)?;
            stream.flush()?;
            return Ok(());
        }
    }

    // Read body if Content-Length is present
    let content_length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    // Parse path_info and query_string
    let (path_info, query_string) = if let Some((p, q)) = full_uri.split_once('?') {
        (p, q)
    } else {
        (full_uri, "")
    };

    let mut cmd = std::process::Command::new("git");
    cmd.arg("http-backend");
    cmd.env("GIT_PROJECT_ROOT", project_root);
    cmd.env("GIT_HTTP_EXPORT_ALL", "1");
    cmd.env("PATH_INFO", path_info);
    cmd.env("QUERY_STRING", query_string);
    cmd.env("REQUEST_METHOD", method);
    if let Some(ct) = headers.get("content-type") {
        cmd.env("CONTENT_TYPE", ct);
    }
    if let Some(gp) = headers.get("git-protocol") {
        cmd.env("HTTP_GIT_PROTOCOL", gp);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::inherit());

    let mut child = cmd.spawn()?;
    if !body.is_empty()
        && let Some(mut stdin) = child.stdin.take()
    {
        stdin.write_all(&body)?;
    }
    let output = child.wait_with_output()?;

    let stream = reader.get_mut();
    let stdout = &output.stdout;
    let (status_line, body_bytes) = if stdout.starts_with(b"Status: ") {
        let end = stdout.iter().position(|&b| b == b'\n').unwrap_or(0);
        let status = &stdout[8..end];
        let status_line = format!("HTTP/1.1 {}\r\n", String::from_utf8_lossy(status).trim());
        (status_line, &stdout[end + 1..])
    } else {
        ("HTTP/1.1 200 OK\r\n".to_string(), stdout.as_slice())
    };

    stream.write_all(status_line.as_bytes())?;
    stream.write_all(b"Connection: close\r\n")?;
    stream.write_all(body_bytes)?;
    stream.flush()?;

    Ok(())
}

use agent_client_protocol::schema::v1::{
    AgentCapabilities, Error, InitializeRequest, InitializeResponse, LoadSessionRequest,
    LoadSessionResponse, McpServer, NewSessionRequest, NewSessionResponse, PromptRequest,
    PromptResponse, SessionId, StopReason,
};
use agent_client_protocol::{Agent, Builder, ConnectionTo, HandleDispatchFrom, Responder};

/// A scriptable mock ACP agent for testing client/session interactions.
#[derive(Clone, Default)]
pub struct MockAcpAgent {
    pub allow_load_session: bool,
    pub existing_sessions: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub received_mcp_servers: Arc<std::sync::Mutex<Vec<McpServer>>>,
    pub created_sessions: Arc<std::sync::Mutex<Vec<String>>>,
    pub loaded_sessions: Arc<std::sync::Mutex<Vec<String>>>,
    pub received_prompts: Arc<std::sync::Mutex<Vec<PromptRequest>>>,
}

impl MockAcpAgent {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_load_session(mut self, allow: bool) -> Self {
        self.allow_load_session = allow;
        self
    }

    pub fn with_existing_session(self, session_id: &str) -> Self {
        self.existing_sessions
            .lock()
            .unwrap()
            .insert(session_id.to_string());
        self
    }

    pub fn build(
        &self,
    ) -> Builder<
        agent_client_protocol::role::acp::Agent,
        impl HandleDispatchFrom<
            <agent_client_protocol::role::acp::Agent as agent_client_protocol::role::Role>::Counterpart,
        > + use<>,
    >{
        let allow_load = self.allow_load_session;
        let existing = Arc::clone(&self.existing_sessions);
        let mcp_servers_created = Arc::clone(&self.received_mcp_servers);
        let mcp_servers_loaded = Arc::clone(&self.received_mcp_servers);
        let created = Arc::clone(&self.created_sessions);
        let loaded = Arc::clone(&self.loaded_sessions);
        let prompts = Arc::clone(&self.received_prompts);

        Agent
            .builder()
            .name("mock-acp-agent")
            .on_receive_request(
                async move |req: InitializeRequest,
                            responder: Responder<InitializeResponse>,
                            _cx: ConnectionTo<agent_client_protocol::role::acp::Client>| {
                    let mut caps = AgentCapabilities::new();
                    caps = caps.load_session(allow_load);
                    responder.respond(
                        InitializeResponse::new(req.protocol_version).agent_capabilities(caps),
                    )
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |req: NewSessionRequest,
                            responder: Responder<NewSessionResponse>,
                            _cx: ConnectionTo<agent_client_protocol::role::acp::Client>| {
                    let sess_id = format!("sess-{}", uuid::Uuid::now_v7());
                    mcp_servers_created.lock().unwrap().extend(req.mcp_servers);
                    created.lock().unwrap().push(sess_id.clone());
                    responder.respond(NewSessionResponse::new(SessionId::new(&*sess_id)))
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |req: LoadSessionRequest,
                            responder: Responder<LoadSessionResponse>,
                            _cx: ConnectionTo<agent_client_protocol::role::acp::Client>| {
                    let sid = req.session_id.to_string();
                    if allow_load && existing.lock().unwrap().contains(&sid) {
                        mcp_servers_loaded.lock().unwrap().extend(req.mcp_servers);
                        loaded.lock().unwrap().push(sid);
                        responder.respond(LoadSessionResponse::new())
                    } else {
                        responder
                            .respond_with_error(Error::invalid_params().data("session not found"))
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .on_receive_request(
                async move |req: PromptRequest,
                            responder: Responder<PromptResponse>,
                            _cx: ConnectionTo<agent_client_protocol::role::acp::Client>| {
                    prompts.lock().unwrap().push(req);
                    responder.respond(PromptResponse::new(StopReason::EndTurn))
                },
                agent_client_protocol::on_receive_request!(),
            )
    }
}
