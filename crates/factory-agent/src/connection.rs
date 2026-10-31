//! mTLS connection management for the Dark Factory runtime.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use df_proto::agent::v1::agent_channel_client::AgentChannelClient;
use df_proto::agent::v1::{
    AgentMessage, ControlMessage, Hello, HelloAck, agent_message, control_message,
};
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
    pub pending_tool_calls: std::collections::HashMap<String, df_proto::agent::v1::ToolCallRequest>,
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
            Some(
                control_message::Body::Cancel(_)
                | control_message::Body::Release(_)
                | control_message::Body::HarnessCredentials(_)
                | control_message::Body::HelloAck(_),
            )
            | None => true,
        }
    }

    /// Track an outgoing tool call expecting a response.
    pub fn track_outgoing_request(&mut self, msg: &AgentMessage) {
        if let Some(agent_message::Body::ToolCallRequest(req)) = &msg.body {
            self.pending_tool_calls
                .insert(req.call_id.clone(), req.clone());
        }
    }

    /// Returns pending tool call requests to be re-sent after reconnect.
    pub fn pending_requests_to_resend(&mut self) -> Vec<AgentMessage> {
        self.pending_tool_calls
            .values()
            .map(|req| AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::ToolCallRequest(req.clone())),
            })
            .collect()
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
            .map_err(|_closed| anyhow::anyhow!("connection manager closed"))
    }

    #[cfg(test)]
    pub async fn recv(&mut self) -> Option<ControlMessage> {
        self.inbound_rx.recv().await
    }

    pub fn shutdown(&self) {
        self.shutdown_tx.send(true).ok(); // receiver may be gone
    }

    pub fn shutdown_tx(&self) -> watch::Sender<bool> {
        self.shutdown_tx.clone()
    }
}

/// Background connection manager implementing mTLS connect, certificate reload,
/// Hello handshake, replay after reconnect, outbox discard on `run_id` mismatch,
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
            if let Err(error) = ob.acknowledge(msg.ack) {
                tracing::warn!(error = %error, ack = msg.ack, "failed to acknowledge outbox messages");
            }
        }
        // Deduplicate commands: Assign, Prompt, ToolCallResult
        if msg.body.is_some() {
            let is_fresh = self
                .command_filter
                .lock()
                .await
                .filter_control_message(&msg);
            if is_fresh {
                inbound_tx.send(msg).await.ok(); // receiver may be gone
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

    /// Connects to the control plane until shutdown: handshake, replay, serve the stream, back
    /// off after a drop.
    async fn run_loop(
        self,
        outbound_rx: mpsc::Receiver<AgentMessage>,
        inbound_tx: mpsc::Sender<ControlMessage>,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        let mut current_delay = self.reconnect_policy.initial_delay;
        let outbound_rx_shared = Arc::new(Mutex::new(outbound_rx));

        let mut flush_deadline: Option<Instant> = None;
        loop {
            if self
                .should_stop(&shutdown_rx, &mut flush_deadline, &outbound_rx_shared)
                .await
            {
                break;
            }

            let Some(mut handshake) = self.connect_and_handshake().await else {
                backoff(
                    &mut shutdown_rx,
                    &mut current_delay,
                    &self.reconnect_policy,
                    flush_deadline,
                )
                .await;
                continue;
            };
            // Handshake succeeded; reset backoff
            current_delay = self.reconnect_policy.initial_delay;

            self.sync_outbox(&handshake.ack, &handshake.tx_stream).await;
            let end = self
                .serve_stream(
                    &mut handshake,
                    &outbound_rx_shared,
                    &inbound_tx,
                    &mut shutdown_rx,
                )
                .await;
            if matches!(end, StreamEnd::Shutdown) {
                break;
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

    /// Reloads the certificates (supporting rotation), connects, opens the bidirectional stream
    /// and exchanges `Hello` and `HelloAck`. `None` after a logged failure: the caller backs off.
    async fn connect_and_handshake(&self) -> Option<Handshake> {
        let tls_files = TlsFiles::load_from_dir(&self.config.tls_dir)
            .inspect_err(|e| tracing::warn!(error = %e, "failed to load TLS files, backing off"))
            .ok()?;

        let mut client = with_timeout(connect_channel(
            &self.config.control_plane_addr,
            &self.config.server_name,
            &tls_files,
        ))
        .await
        .inspect_err(
            |e| tracing::warn!(error = %e, "connection to control plane failed, backing off"),
        )
        .ok()?;

        let (tx_stream, rx_stream) = mpsc::channel(100);
        let response = with_timeout(async {
            client
                .connect(ReceiverStream::new(rx_stream))
                .await
                .map_err(anyhow::Error::from)
        })
        .await
        .inspect_err(
            |e| tracing::warn!(error = %e, "bidirectional stream connect failed, backing off"),
        )
        .ok()?;
        let mut in_stream = response.into_inner();

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
            return None;
        }

        if let Ok(Some(Ok(ControlMessage {
            body: Some(control_message::Body::HelloAck(ack)),
            ..
        }))) = tokio::time::timeout(HANDSHAKE_TIMEOUT, in_stream.next()).await
        {
            Some(Handshake {
                tx_stream,
                in_stream,
                ack,
            })
        } else {
            tracing::warn!("expected HelloAck as first message from control plane, backing off");
            None
        }
    }

    /// Handles `HelloAck`: discards a leftover outbox of another run, or replays the messages
    /// the control plane has not persisted plus the pending requests.
    async fn sync_outbox(&self, hello_ack: &HelloAck, tx_stream: &mpsc::Sender<AgentMessage>) {
        let mut ob = self.outbox.lock().await;
        if !ob.run_state().run_id.is_empty() && ob.run_state().run_id != hello_ack.run_id {
            self.discard_leftover_outbox(&mut ob, &hello_ack.run_id, tx_stream)
                .await;
            return;
        }
        if ob.run_state().run_id.is_empty()
            && let Err(error) = ob.discard(&hello_ack.run_id)
        {
            tracing::warn!(error = %error, run_id = %hello_ack.run_id, "failed to reset the outbox");
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
            if !is_replayed(&unacked_msgs, &msg)
                && let Ok(seq) = ob.append(msg.clone())
            {
                let mut to_send = msg;
                to_send.seq = seq;
                tx_stream.send(to_send).await.ok(); // receiver may be gone
            }
        }
    }

    /// Discards the outbox of the previous run and tells the control plane if messages were lost.
    async fn discard_leftover_outbox(
        &self,
        ob: &mut Outbox,
        new_run_id: &str,
        tx_stream: &mpsc::Sender<AgentMessage>,
    ) {
        tracing::warn!(
            old_run_id = %ob.run_state().run_id,
            new_run_id = %new_run_id,
            "run_id mismatch: discarding leftover outbox"
        );
        // Only messages the control plane never acknowledged are lost; a fully
        // acknowledged leftover is routine on a reused task volume.
        let notice =
            discarded_outbox_notice(ob.read_unacknowledged(0).map(|unacked| unacked.len()));
        if let Err(error) = ob.discard(new_run_id) {
            tracing::warn!(error = %error, run_id = %new_run_id, "failed to reset the outbox");
        }
        *self.command_filter.lock().await = CommandFilter::new();

        if let Some(message) = notice {
            let notice_event = crate::transcript::notice_event(df_proto::transcript::v1::Notice {
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
                tx_stream.send(to_send).await.ok(); // receiver may be gone
            }
        }
    }

    /// Runs the active session with concurrent inbound and outbound tasks until the stream drops
    /// or shutdown is requested.
    async fn serve_stream(
        &self,
        handshake: &mut Handshake,
        outbound_rx: &Arc<Mutex<mpsc::Receiver<AgentMessage>>>,
        inbound_tx: &mpsc::Sender<ControlMessage>,
        shutdown_rx: &mut watch::Receiver<bool>,
    ) -> StreamEnd {
        let (disconnect_tx, mut disconnect_rx) = mpsc::channel::<()>(1);
        let (stop_tx, stop_rx) = watch::channel(false);
        let mut out_worker = self.spawn_send_worker(
            Arc::clone(outbound_rx),
            handshake.tx_stream.clone(),
            disconnect_tx,
            stop_rx,
        );

        loop {
            tokio::select! {
                () = wait_shutdown(shutdown_rx) => {
                    self.flush_at_shutdown(
                        &stop_tx,
                        &mut out_worker,
                        &mut handshake.in_stream,
                        inbound_tx,
                    )
                    .await;
                    return StreamEnd::Shutdown;
                }
                _ = disconnect_rx.recv() => {
                    break;
                }
                inbound_item = handshake.in_stream.next() => {
                    if let Some(Ok(msg)) = inbound_item { self.handle_inbound(msg, inbound_tx).await } else {
                        tracing::info!("control plane stream ended or dropped");
                        break; // Break to reconnect
                    }
                }
            }
        }
        // Let the worker finish the message it is handling so it is persisted for replay
        stop_tx.send(true).ok(); // receiver may be gone
        if tokio::time::timeout(WORKER_STOP_TIMEOUT, &mut out_worker)
            .await
            .is_err()
        {
            out_worker.abort();
        }
        StreamEnd::Dropped
    }

    /// Starts the worker that persists outbound messages in the outbox and sends them on the
    /// stream. It signals `disconnect_tx` when the stream is gone.
    fn spawn_send_worker(
        &self,
        outbound_rx: Arc<Mutex<mpsc::Receiver<AgentMessage>>>,
        tx_stream: mpsc::Sender<AgentMessage>,
        disconnect_tx: mpsc::Sender<()>,
        stop_rx: watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let outbox = Arc::clone(&self.outbox);
        let command_filter = Arc::clone(&self.command_filter);
        tokio::spawn(send_outbound(
            outbox,
            command_filter,
            outbound_rx,
            tx_stream,
            disconnect_tx,
            stop_rx,
        ))
    }

    /// Flushes queued messages (e.g. the final `RunReport`) and gives the control plane a bounded
    /// time to acknowledge them before the process exits.
    async fn flush_at_shutdown(
        &self,
        stop_tx: &watch::Sender<bool>,
        out_worker: &mut tokio::task::JoinHandle<()>,
        in_stream: &mut tonic::Streaming<ControlMessage>,
        inbound_tx: &mpsc::Sender<ControlMessage>,
    ) {
        stop_tx.send(true).ok(); // receiver may be gone
        let drain = async {
            if let Err(error) = (&mut *out_worker).await {
                tracing::warn!(error = %error, "outbound worker failed before the shutdown flush");
            }
            while !self.outbox.lock().await.is_empty() {
                match in_stream.next().await {
                    Some(Ok(msg)) => self.handle_inbound(msg, inbound_tx).await,
                    _ => break,
                }
            }
        };
        if tokio::time::timeout(SHUTDOWN_DRAIN_TIMEOUT, drain)
            .await
            .is_err()
        {
            tracing::warn!("timed out flushing outbox at shutdown");
            out_worker.abort();
        }
    }
}

/// Persists outbound messages in the outbox and sends them on the stream until stopped. It
/// signals `disconnect_tx` when the stream is gone.
async fn send_outbound(
    outbox: Arc<Mutex<Outbox>>,
    command_filter: Arc<Mutex<CommandFilter>>,
    outbound_rx: Arc<Mutex<mpsc::Receiver<AgentMessage>>>,
    tx_stream: mpsc::Sender<AgentMessage>,
    disconnect_tx: mpsc::Sender<()>,
    mut stop_rx: watch::Receiver<bool>,
) {
    loop {
        // Only the wait for the next message can be interrupted by `stop`; a message
        // that was taken off the channel is always persisted. Once stopped, the
        // worker keeps going until the channel is empty.
        let Some(msg) = next_outbound(&outbound_rx, &mut stop_rx).await else {
            // Stopped, or the runner dropped its sender
            if !*stop_rx.borrow() {
                disconnect_tx.send(()).await.ok(); // receiver may be gone
            }
            return;
        };
        command_filter.lock().await.track_outgoing_request(&msg);
        let seq = match Outbox::append_or_stop(&outbox, msg.clone(), &mut stop_rx).await {
            Ok(seq) => seq,
            Err(e) => {
                tracing::error!(error = %e, "failed to append message to outbox");
                continue;
            }
        };
        let mut to_send = msg;
        to_send.seq = seq;
        if tx_stream.send(to_send).await.is_err() {
            disconnect_tx.send(()).await.ok(); // receiver may be gone
            return;
        }
    }
}

/// The next message to send. Only the wait for it can be interrupted by `stop`; a message that
/// was taken off the channel is always handed to the caller. Once stopped, it keeps returning
/// messages until the channel is empty.
async fn next_outbound(
    outbound_rx: &Mutex<mpsc::Receiver<AgentMessage>>,
    stop_rx: &mut watch::Receiver<bool>,
) -> Option<AgentMessage> {
    let mut rx = outbound_rx.lock().await;
    if *stop_rx.borrow() {
        return rx.try_recv().ok();
    }
    tokio::select! {
        msg = rx.recv() => msg,
        _ = stop_rx.changed() => rx.try_recv().ok(),
    }
}

/// An open stream to the control plane after the Hello exchange.
struct Handshake {
    tx_stream: mpsc::Sender<AgentMessage>,
    in_stream: tonic::Streaming<ControlMessage>,
    ack: HelloAck,
}

/// Why [`ConnectionManager::serve_stream`] returned.
enum StreamEnd {
    /// Shutdown was requested and the outbox was flushed.
    Shutdown,
    /// The stream dropped; reconnect.
    Dropped,
}

/// True if `msg` (a pending tool call) was already replayed as one of the unacknowledged outbox
/// `messages`.
fn is_replayed(messages: &[AgentMessage], msg: &AgentMessage) -> bool {
    let Some(agent_message::Body::ToolCallRequest(m_req)) = &msg.body else {
        return false;
    };
    messages.iter().any(|u| {
        matches!(&u.body, Some(agent_message::Body::ToolCallRequest(u_req)) if u_req.call_id == m_req.call_id)
    })
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
async fn with_timeout<T>(fut: impl Future<Output = anyhow::Result<T>>) -> anyhow::Result<T> {
    tokio::time::timeout(HANDSHAKE_TIMEOUT, fut)
        .await
        .map_err(|_elapsed| anyhow::anyhow!("timed out after {HANDSHAKE_TIMEOUT:?}"))?
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
                () = wait_shutdown(shutdown_rx) => {},
                () = tokio::time::sleep(*delay) => {},
            }
        }
    }
    *delay = delay.mul_f64(policy.backoff_factor).min(policy.max_delay);
}

/// Resolves once shutdown has been requested (also if that happened before the call).
async fn wait_shutdown(shutdown_rx: &mut watch::Receiver<bool>) {
    shutdown_rx.wait_for(|requested| *requested).await.ok();
}
