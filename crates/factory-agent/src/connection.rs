//! mTLS connection management for the Dark Factory runtime.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use df_proto::agent::v1::agent_channel_client::AgentChannelClient;
use df_proto::agent::v1::{AgentMessage, ControlMessage, Hello, agent_message, control_message};
use tokio::sync::{Mutex, mpsc, watch};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};

use crate::config::RuntimeConfig;
use crate::outbox::Outbox;

/// In-memory TLS certificates and private key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsFiles {
    pub ca_cert: Vec<u8>,
    pub client_cert: Vec<u8>,
    pub client_key: Vec<u8>,
}

impl TlsFiles {
    /// Load `ca.crt`, `tls.crt`, and `tls.key` from the specified directory.
    pub fn load_from_dir(dir: &Path) -> anyhow::Result<Self> {
        let ca_path = dir.join("ca.crt");
        let cert_path = dir.join("tls.crt");
        let key_path = dir.join("tls.key");

        let ca_cert = std::fs::read(&ca_path).map_err(|e| {
            anyhow::anyhow!(
                "failed to read CA certificate from {}: {e}",
                ca_path.display()
            )
        })?;
        let client_cert = std::fs::read(&cert_path).map_err(|e| {
            anyhow::anyhow!(
                "failed to read pod certificate from {}: {e}",
                cert_path.display()
            )
        })?;
        let client_key = std::fs::read(&key_path).map_err(|e| {
            anyhow::anyhow!(
                "failed to read pod private key from {}: {e}",
                key_path.display()
            )
        })?;

        Ok(Self {
            ca_cert,
            client_cert,
            client_key,
        })
    }

    /// Construct `ClientTlsConfig` configured with the instance CA, pod identity, and server name.
    pub fn client_tls_config(&self, server_name: &str) -> ClientTlsConfig {
        let ca = Certificate::from_pem(&self.ca_cert);
        let identity = Identity::from_pem(&self.client_cert, &self.client_key);

        ClientTlsConfig::new()
            .domain_name(server_name)
            .ca_certificate(ca)
            .identity(identity)
    }
}

/// Build a tonic `Endpoint` configured with mTLS for the control plane.
pub fn build_endpoint(
    addr: &str,
    server_name: &str,
    tls_files: &TlsFiles,
) -> anyhow::Result<Endpoint> {
    let tls = tls_files.client_tls_config(server_name);
    let endpoint = Endpoint::from_shared(addr.to_string())?.tls_config(tls)?;
    Ok(endpoint)
}

/// Connect to the control plane and return an `AgentChannelClient`.
pub async fn connect_channel(
    addr: &str,
    server_name: &str,
    tls_files: &TlsFiles,
) -> anyhow::Result<AgentChannelClient<Channel>> {
    let endpoint = build_endpoint(addr, server_name, tls_files)?;
    let channel = endpoint.connect().await?;
    Ok(AgentChannelClient::new(channel))
}

/// Upper bound for each connection setup step (connect, stream open, Hello/HelloAck).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a worker may take to finish the message it is handling when it is asked to stop.
const WORKER_STOP_TIMEOUT: Duration = Duration::from_secs(5);
/// How long, after a shutdown request, the manager keeps reconnecting to deliver queued messages.
const SHUTDOWN_RECONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long shutdown waits for queued messages to be sent and acknowledged.
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the runtime waits for a `CredentialGrant`. Pending requests older than this are not
/// re-sent after a reconnect, because the requester has given up.
pub const CREDENTIAL_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Exponential backoff policy for reconnecting to the control plane.
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_factor: f64,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_factor: 2.0,
        }
    }
}

/// Deduplication and pending-request tracking for control plane communication.
#[derive(Debug, Default)]
pub struct CommandFilter {
    pub assigned_runs: std::collections::HashSet<String>,
    pub seen_prompt_ids: std::collections::HashSet<String>,
    pub seen_tool_results: std::collections::HashSet<String>,
    pub seen_credential_grants: std::collections::HashSet<String>,
    pub pending_tool_calls: std::collections::HashMap<String, df_proto::agent::v1::ToolCallRequest>,
    pub pending_credentials: std::collections::HashMap<
        String,
        (std::time::Instant, df_proto::agent::v1::CredentialRequest),
    >,
}

impl CommandFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if the message is fresh (not a duplicate) and records it.
    pub fn filter_control_message(&mut self, msg: &ControlMessage) -> bool {
        match &msg.body {
            Some(control_message::Body::Assign(assign)) => {
                self.assigned_runs.insert(assign.run_id.clone())
            }
            Some(control_message::Body::Prompt(prompt)) => {
                // An empty id carries no identity, so it can't be deduplicated
                prompt.prompt_id.is_empty() || self.seen_prompt_ids.insert(prompt.prompt_id.clone())
            }
            Some(control_message::Body::ToolCallResult(res)) => {
                self.pending_tool_calls.remove(&res.call_id);
                self.seen_tool_results.insert(res.call_id.clone())
            }
            Some(control_message::Body::CredentialGrant(grant)) => {
                self.pending_credentials.remove(&grant.request_id);
                self.seen_credential_grants.insert(grant.request_id.clone())
            }
            Some(control_message::Body::Cancel(_))
            | Some(control_message::Body::Release(_))
            | Some(control_message::Body::HarnessCredentials(_)) => true,
            Some(control_message::Body::HelloAck(_)) | None => true,
        }
    }

    /// Track outgoing tool call or credential request expecting a response.
    pub fn track_outgoing_request(&mut self, msg: &AgentMessage) {
        match &msg.body {
            Some(agent_message::Body::ToolCallRequest(req)) => {
                self.pending_tool_calls
                    .insert(req.call_id.clone(), req.clone());
            }
            Some(agent_message::Body::CredentialRequest(req)) => {
                self.pending_credentials.insert(
                    req.request_id.clone(),
                    (std::time::Instant::now(), req.clone()),
                );
            }
            _ => {}
        }
    }

    /// Returns pending tool call and credential requests to be re-sent after reconnect.
    ///
    /// Credential requests older than [`CREDENTIAL_REQUEST_TIMEOUT`] are dropped instead.
    pub fn pending_requests_to_resend(&mut self) -> Vec<AgentMessage> {
        self.pending_credentials
            .retain(|_, (sent_at, _)| sent_at.elapsed() < CREDENTIAL_REQUEST_TIMEOUT);

        let mut list = Vec::new();
        for req in self.pending_tool_calls.values() {
            list.push(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::ToolCallRequest(req.clone())),
            });
        }
        for (_, req) in self.pending_credentials.values() {
            list.push(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::CredentialRequest(req.clone())),
            });
        }
        list
    }
}

/// Handle to communicate with the background connection manager.
pub struct ConnectionHandle {
    pub outbound_tx: mpsc::Sender<AgentMessage>,
    pub inbound_rx: mpsc::Receiver<ControlMessage>,
    shutdown_tx: watch::Sender<bool>,
    /// The background task. After requesting shutdown, await it so queued messages get flushed.
    pub task: Option<tokio::task::JoinHandle<()>>,
}

impl ConnectionHandle {
    #[cfg(test)]
    pub async fn send(&self, msg: AgentMessage) -> anyhow::Result<()> {
        self.outbound_tx
            .send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("connection manager closed"))
    }

    #[cfg(test)]
    pub async fn recv(&mut self) -> Option<ControlMessage> {
        self.inbound_rx.recv().await
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    pub fn shutdown_tx(&self) -> watch::Sender<bool> {
        self.shutdown_tx.clone()
    }
}

/// Background connection manager implementing mTLS connect, certificate reload,
/// Hello handshake, replay after reconnect, outbox discard on run_id mismatch,
/// and exponential reconnect backoff.
pub struct ConnectionManager {
    config: RuntimeConfig,
    outbox: Arc<Mutex<Outbox>>,
    reconnect_policy: ReconnectPolicy,
    command_filter: Arc<Mutex<CommandFilter>>,
}

impl ConnectionManager {
    pub fn new(
        config: RuntimeConfig,
        outbox: Arc<Mutex<Outbox>>,
        reconnect_policy: ReconnectPolicy,
    ) -> Self {
        Self {
            config,
            outbox,
            reconnect_policy,
            command_filter: Arc::new(Mutex::new(CommandFilter::new())),
        }
    }

    /// Spawn the connection manager background task and return its handle.
    pub fn spawn(self) -> ConnectionHandle {
        let (outbound_tx, outbound_rx) = mpsc::channel(100);
        let (inbound_tx, inbound_rx) = mpsc::channel(100);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let task = tokio::spawn(async move {
            self.run_loop(outbound_rx, inbound_tx, shutdown_rx).await;
        });

        ConnectionHandle {
            outbound_tx,
            inbound_rx,
            shutdown_tx,
            task: Some(task),
        }
    }

    /// Applies an inbound control message: acknowledges the outbox, deduplicates commands and
    /// forwards fresh ones to the runner.
    async fn handle_inbound(&self, msg: ControlMessage, inbound_tx: &mpsc::Sender<ControlMessage>) {
        // Acknowledge outbox up to cumulative ack seq
        if msg.ack > 0 {
            let mut ob = self.outbox.lock().await;
            let _ = ob.acknowledge(msg.ack);
        }
        // Deduplicate commands: Assign, Prompt, ToolCallResult, CredentialGrant
        if msg.body.is_some() {
            let is_fresh = self
                .command_filter
                .lock()
                .await
                .filter_control_message(&msg);
            if is_fresh {
                let _ = inbound_tx.send(msg).await;
            } else {
                tracing::debug!(?msg, "ignoring duplicate control message");
            }
        }
    }

    /// Moves messages still queued in the outbound channel into the outbox, so they survive a
    /// shutdown that happens while no send worker is running.
    async fn drain_outbound_to_outbox(&self, outbound_rx: &Mutex<mpsc::Receiver<AgentMessage>>) {
        let mut rx = outbound_rx.lock().await;
        while let Ok(msg) = rx.try_recv() {
            if let Err(e) = self.outbox.lock().await.append(msg) {
                tracing::error!(error = %e, "failed to persist queued message at shutdown");
            }
        }
    }

    /// Decides at the top of the connect loop whether to exit. After a shutdown request the
    /// manager keeps reconnecting until the outbox is empty or the flush deadline passes, so a
    /// final `RunReport` still reaches the control plane after an outage.
    async fn should_stop(
        &self,
        shutdown_rx: &watch::Receiver<bool>,
        flush_deadline: &mut Option<Instant>,
        outbound_rx: &Mutex<mpsc::Receiver<AgentMessage>>,
    ) -> bool {
        if !*shutdown_rx.borrow() {
            return false;
        }
        let deadline =
            *flush_deadline.get_or_insert_with(|| Instant::now() + SHUTDOWN_RECONNECT_TIMEOUT);
        self.drain_outbound_to_outbox(outbound_rx).await;
        let pending = !self.outbox.lock().await.is_empty();
        if pending && Instant::now() < deadline {
            return false;
        }
        if pending {
            tracing::warn!("exiting with unacknowledged messages left in the outbox");
        }
        true
    }

    #[expect(
        clippy::too_many_lines,
        reason = "split run_loop into connect, send and receive steps"
    )]
    async fn run_loop(
        self,
        outbound_rx: mpsc::Receiver<AgentMessage>,
        inbound_tx: mpsc::Sender<ControlMessage>,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        let mut current_delay = self.reconnect_policy.initial_delay;
        let outbound_rx_shared = Arc::new(tokio::sync::Mutex::new(outbound_rx));

        let mut flush_deadline: Option<Instant> = None;
        'outer: loop {
            if self
                .should_stop(&shutdown_rx, &mut flush_deadline, &outbound_rx_shared)
                .await
            {
                break;
            }

            // 1. Reload certificates on every connect attempt (supporting cert rotation)
            let tls_files = match TlsFiles::load_from_dir(&self.config.tls_dir) {
                Ok(files) => files,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to load TLS files, backing off");
                    backoff(
                        &mut shutdown_rx,
                        &mut current_delay,
                        &self.reconnect_policy,
                        flush_deadline,
                    )
                    .await;
                    continue;
                }
            };

            // 2. Connect to control plane
            let mut client = match with_timeout(connect_channel(
                &self.config.control_plane_addr,
                &self.config.server_name,
                &tls_files,
            ))
            .await
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(error = %e, "connection to control plane failed, backing off");
                    backoff(
                        &mut shutdown_rx,
                        &mut current_delay,
                        &self.reconnect_policy,
                        flush_deadline,
                    )
                    .await;
                    continue;
                }
            };

            // 3. Open bidirectional stream
            let (tx_stream, rx_stream) = mpsc::channel(100);
            let response = match with_timeout(async {
                client
                    .connect(ReceiverStream::new(rx_stream))
                    .await
                    .map_err(anyhow::Error::from)
            })
            .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = %e, "bidirectional stream connect failed, backing off");
                    backoff(
                        &mut shutdown_rx,
                        &mut current_delay,
                        &self.reconnect_policy,
                        flush_deadline,
                    )
                    .await;
                    continue;
                }
            };
            let mut in_stream = response.into_inner();

            // 4. Send Hello
            let previous_run_id = self.outbox.lock().await.run_state().run_id.clone();
            let hello = AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::Hello(Hello {
                    runtime_version: env!("CARGO_PKG_VERSION").to_string(),
                    capabilities: vec!["darkfactory.agent.v1".to_string()],
                    previous_run_id,
                })),
            };

            if tx_stream.send(hello).await.is_err() {
                tracing::warn!("failed to send Hello over stream, backing off");
                backoff(
                    &mut shutdown_rx,
                    &mut current_delay,
                    &self.reconnect_policy,
                    flush_deadline,
                )
                .await;
                continue;
            }

            // 5. Receive HelloAck
            let hello_ack = match tokio::time::timeout(HANDSHAKE_TIMEOUT, in_stream.next()).await {
                Ok(Some(Ok(ControlMessage {
                    body: Some(control_message::Body::HelloAck(ack)),
                    ..
                }))) => ack,
                _ => {
                    tracing::warn!(
                        "expected HelloAck as first message from control plane, backing off"
                    );
                    backoff(
                        &mut shutdown_rx,
                        &mut current_delay,
                        &self.reconnect_policy,
                        flush_deadline,
                    )
                    .await;
                    continue;
                }
            };

            // Handshake succeeded; reset backoff
            current_delay = self.reconnect_policy.initial_delay;

            // 6. Handle HelloAck: check run-id mismatch and replay unacknowledged messages
            {
                let mut ob = self.outbox.lock().await;
                if !ob.run_state().run_id.is_empty() && ob.run_state().run_id != hello_ack.run_id {
                    tracing::warn!(
                        old_run_id = %ob.run_state().run_id,
                        new_run_id = %hello_ack.run_id,
                        "run_id mismatch: discarding leftover outbox"
                    );
                    // Only messages the control plane never acknowledged are lost; a fully
                    // acknowledged leftover is routine on a reused task volume.
                    let notice = discarded_outbox_notice(
                        ob.read_unacknowledged(0).map(|unacked| unacked.len()),
                    );
                    let _ = ob.discard(&hello_ack.run_id);
                    *self.command_filter.lock().await = CommandFilter::new();

                    if let Some(message) = notice {
                        let notice_event =
                            crate::transcript::notice_event(df_proto::transcript::v1::Notice {
                                level: df_proto::transcript::v1::NoticeLevel::Warning as i32,
                                code: "runtime.outbox_discarded".to_string(),
                                message,
                            });

                        let notice_msg = AgentMessage {
                            seq: 0,
                            body: Some(agent_message::Body::TranscriptEvent(notice_event)),
                        };

                        if let Ok(seq) = ob.append(notice_msg.clone()) {
                            let mut to_send = notice_msg;
                            to_send.seq = seq;
                            let _ = tx_stream.send(to_send).await;
                        }
                    }
                } else {
                    if ob.run_state().run_id.is_empty() {
                        let _ = ob.discard(&hello_ack.run_id);
                    }
                    // Replay all unacknowledged messages after HelloAck.last_persisted_seq
                    let mut unacked_msgs = Vec::new();
                    if let Ok(unacked) = ob.read_unacknowledged(hello_ack.last_persisted_seq) {
                        for msg in &unacked {
                            if tx_stream.send(msg.clone()).await.is_err() {
                                break;
                            }
                        }
                        unacked_msgs = unacked;
                    }

                    // Re-send any pending tool call or credential requests waiting for responses
                    let pending = self
                        .command_filter
                        .lock()
                        .await
                        .pending_requests_to_resend();
                    for msg in pending {
                        let already_replayed =
                            unacked_msgs.iter().any(|u| match (&u.body, &msg.body) {
                                (
                                    Some(agent_message::Body::ToolCallRequest(u_req)),
                                    Some(agent_message::Body::ToolCallRequest(m_req)),
                                ) => u_req.call_id == m_req.call_id,
                                (
                                    Some(agent_message::Body::CredentialRequest(u_req)),
                                    Some(agent_message::Body::CredentialRequest(m_req)),
                                ) => u_req.request_id == m_req.request_id,
                                _ => false,
                            });
                        if !already_replayed && let Ok(seq) = ob.append(msg.clone()) {
                            let mut to_send = msg;
                            to_send.seq = seq;
                            let _ = tx_stream.send(to_send).await;
                        }
                    }
                }
            }

            // 7. Active session loop with concurrent inbound and outbound tasks (Finding 2)
            let (disconnect_tx, mut disconnect_rx) = mpsc::channel::<()>(1);
            let (stop_tx, mut stop_rx) = watch::channel(false);
            let mut out_worker = {
                let outbox = Arc::clone(&self.outbox);
                let command_filter = Arc::clone(&self.command_filter);
                let outbound_rx = Arc::clone(&outbound_rx_shared);
                let tx_stream = tx_stream.clone();
                let disconnect_tx = disconnect_tx.clone();
                tokio::spawn(async move {
                    loop {
                        // Only the wait for the next message can be interrupted by `stop`; a message
                        // that was taken off the channel is always persisted. Once stopped, the
                        // worker keeps going until the channel is empty.
                        let msg = {
                            let mut rx = outbound_rx.lock().await;
                            if *stop_rx.borrow() {
                                rx.try_recv().ok()
                            } else {
                                tokio::select! {
                                    msg = rx.recv() => msg,
                                    _ = stop_rx.changed() => rx.try_recv().ok(),
                                }
                            }
                        };
                        match msg {
                            Some(msg) => {
                                command_filter.lock().await.track_outgoing_request(&msg);
                                match Outbox::append_or_stop(&outbox, msg.clone(), &mut stop_rx)
                                    .await
                                {
                                    Ok(seq) => {
                                        let mut to_send = msg;
                                        to_send.seq = seq;
                                        if tx_stream.send(to_send).await.is_err() {
                                            let _ = disconnect_tx.send(()).await;
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "failed to append message to outbox");
                                    }
                                }
                            }
                            None => {
                                // Stopped, or the runner dropped its sender
                                if !*stop_rx.borrow() {
                                    let _ = disconnect_tx.send(()).await;
                                }
                                break;
                            }
                        }
                    }
                })
            };

            loop {
                tokio::select! {
                    _ = wait_shutdown(&mut shutdown_rx) => {
                        // Flush queued messages (e.g. the final RunReport) and give the control
                        // plane a bounded time to acknowledge them before the process exits.
                        let _ = stop_tx.send(true);
                        let drain = async {
                            let _ = (&mut out_worker).await;
                            while !self.outbox.lock().await.is_empty() {
                                match in_stream.next().await {
                                    Some(Ok(msg)) => self.handle_inbound(msg, &inbound_tx).await,
                                    _ => break,
                                }
                            }
                        };
                        if tokio::time::timeout(SHUTDOWN_DRAIN_TIMEOUT, drain).await.is_err() {
                            tracing::warn!("timed out flushing outbox at shutdown");
                            out_worker.abort();
                        }
                        break 'outer;
                    }
                    _ = disconnect_rx.recv() => {
                        break;
                    }
                    inbound_item = in_stream.next() => {
                        match inbound_item {
                            Some(Ok(msg)) => self.handle_inbound(msg, &inbound_tx).await,
                            _ => {
                                tracing::info!("control plane stream ended or dropped");
                                break; // Break to reconnect
                            }
                        }
                    }
                }
            }
            // Let the worker finish the message it is handling so it is persisted for replay
            let _ = stop_tx.send(true);
            if tokio::time::timeout(WORKER_STOP_TIMEOUT, &mut out_worker)
                .await
                .is_err()
            {
                out_worker.abort();
            }

            // Apply backoff before reconnecting after an unexpected drop
            backoff(
                &mut shutdown_rx,
                &mut current_delay,
                &self.reconnect_policy,
                flush_deadline,
            )
            .await;
        }

        // Shutdown while no worker was running: persist whatever is still queued
        self.drain_outbound_to_outbox(&outbound_rx_shared).await;
    }
}

/// The notice text for a leftover outbox that is discarded, given the number of its unacknowledged
/// messages: `None` if nothing was lost. An outbox that cannot be read may have held unsent
/// messages, so it gets a notice without a count.
pub(crate) fn discarded_outbox_notice(unacked: anyhow::Result<usize>) -> Option<String> {
    match unacked {
        Ok(0) => None,
        Ok(n) => Some(format!(
            "{n} unsent message(s) from the previous run were discarded"
        )),
        Err(error) => {
            tracing::warn!(error = %error, "failed to read the leftover outbox before discarding it");
            Some(
                "The outbox of the previous run could not be read; unsent messages may have been discarded"
                    .to_string(),
            )
        }
    }
}

/// Runs a connection setup step with [`HANDSHAKE_TIMEOUT`].
async fn with_timeout<T>(
    fut: impl std::future::Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    tokio::time::timeout(HANDSHAKE_TIMEOUT, fut)
        .await
        .map_err(|_| anyhow::anyhow!("timed out after {HANDSHAKE_TIMEOUT:?}"))?
}

/// Sleeps for `delay`, then grows it per `policy`.
///
/// Returns early when shutdown is requested, unless a flush deadline is already running: then it
/// sleeps (at most until the deadline) so the manager keeps trying to reach the control plane.
async fn backoff(
    shutdown_rx: &mut watch::Receiver<bool>,
    delay: &mut Duration,
    policy: &ReconnectPolicy,
    flush_deadline: Option<Instant>,
) {
    match flush_deadline {
        Some(deadline) => {
            let remaining = deadline.saturating_duration_since(Instant::now());
            tokio::time::sleep((*delay).min(remaining)).await;
        }
        None => {
            tokio::select! {
                _ = wait_shutdown(shutdown_rx) => {},
                _ = tokio::time::sleep(*delay) => {},
            }
        }
    }
    *delay = delay.mul_f64(policy.backoff_factor).min(policy.max_delay);
}

/// Resolves once shutdown has been requested (also if that happened before the call).
async fn wait_shutdown(shutdown_rx: &mut watch::Receiver<bool>) {
    let _ = shutdown_rx.wait_for(|requested| *requested).await;
}
