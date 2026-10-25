//! The egress proxy: `CONNECT` tunnels and plain HTTP forwarding, decided by [`crate::policy`].
//!
//! Every request becomes one [`FlowEvent`]. Names are resolved here, never by the destination's
//! choice of address, and addresses in blocked ranges are refused.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use df_core::host::{normalize_host, strip_brackets};
use df_proto::gateway::v1::{FlowEvent, Phase, Verdict};
use http_body_util::{BodyExt, Empty, Full, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::header::{HOST, HeaderName};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::OnUpgrade;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::{AbortHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::Instrument as _;

use crate::count::{Counted, Direction, Metered, Traffic};
use crate::flow::{FlowLog, duration_proto, timestamp_now};
use crate::metrics::{Decision, Metrics};
use crate::policy::{self, Policy, Sandbox};
use crate::resolve::{BlockedRanges, Resolver};
use crate::sni;

/// Time to connect to the destination.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Time the destination has to accept the HTTP connection and to answer with response headers.
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(30);
/// Time open tunnels get to record their events when the proxy stops.
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(2);
/// Time a client has to send its ClientHello after the tunnel was established.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
const PROXY_HEADERS: [HeaderName; 2] = [
    HeaderName::from_static("proxy-connection"),
    HeaderName::from_static("proxy-authorization"),
];

/// Opens connections to destinations.
#[async_trait]
pub trait Dialer: Send + Sync {
    async fn dial(&self, destination: SocketAddr) -> std::io::Result<TcpStream>;
}

/// Plain TCP connections with a timeout.
pub struct TcpDialer;

#[async_trait]
impl Dialer for TcpDialer {
    async fn dial(&self, destination: SocketAddr) -> std::io::Result<TcpStream> {
        tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(destination))
            .await
            .map_err(|_| std::io::ErrorKind::TimedOut)?
    }
}

type ResponseBody = BoxBody<Bytes, hyper::Error>;

/// The latest policy of a gateway pod; `None` until the first snapshot arrived.
pub type PolicyReceiver = watch::Receiver<Option<Arc<Policy>>>;

/// The proxy of one gateway pod.
pub struct Proxy {
    /// Name of the gateway pod, recorded in flow events.
    pod: String,
    policy: PolicyReceiver,
    resolver: Arc<dyn Resolver>,
    dialer: Arc<dyn Dialer>,
    blocked: BlockedRanges,
    flows: Arc<FlowLog>,
    metrics: Arc<Metrics>,
    tunnels: Arc<Tunnels>,
    /// Last flow id handed out.
    last_flow: AtomicU64,
    /// Tunnels and upstream HTTP connections. Aborted when `serve` ends.
    /// `None` once `serve` started shutting down: nothing new is accepted.
    tasks: Mutex<Option<JoinSet<()>>>,
}

/// What a request is about once it was parsed.
struct Target {
    host: String,
    port: u16,
}

/// The answer to requests that are in flight when the proxy stops: the policy denied nothing.
const SHUTDOWN: Refusal = Refusal {
    status: StatusCode::SERVICE_UNAVAILABLE,
    verdict: Verdict::Allowed,
    reason: "gateway-shutdown",
};

/// A request the proxy does not carry out.
struct Refusal {
    status: StatusCode,
    verdict: Verdict,
    reason: &'static str,
}

impl Proxy {
    pub fn new(
        pod: impl Into<String>,
        policy: PolicyReceiver,
        resolver: Arc<dyn Resolver>,
        dialer: Arc<dyn Dialer>,
        blocked: BlockedRanges,
        flows: Arc<FlowLog>,
        metrics: Arc<Metrics>,
    ) -> Self {
        Self {
            pod: pod.into(),
            policy,
            resolver,
            dialer,
            blocked,
            flows,
            metrics,
            tunnels: Arc::default(),
            // Ids must not repeat when the container restarts under the same pod name: start at
            // the current time (microseconds).
            last_flow: AtomicU64::new(unix_micros()),
            tasks: Mutex::new(Some(JoinSet::new())),
        }
    }

    /// Serves connections from `listener` until `shutdown` resolves; open tunnels and upstream
    /// connections end with it.
    pub async fn serve(self: Arc<Self>, listener: TcpListener, shutdown: impl Future<Output = ()>) {
        let root = CancellationToken::new();
        let mut connections = JoinSet::new();
        connections.spawn(self.clone().enforce_policy_changes(root.clone()));
        tokio::pin!(shutdown);
        loop {
            while connections.try_join_next().is_some() {}
            tokio::select! {
                () = &mut shutdown => break,
                accepted = listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        connections.spawn(self.clone().connection(stream, peer.ip(), root.clone()));
                    }
                    Err(error) => {
                        tracing::warn!(%error, "accept failed on the proxy port; backing off");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
            }
        }
        // Tunnels and in-flight requests observe `root`, record their events and end; client
        // connections finish gracefully. Give them a moment, then stop whatever is left (the
        // tasks hold this proxy, so they must not outlive `serve`).
        root.cancel();
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .unwrap_or_default();
        let drained = async {
            while tasks.join_next().await.is_some() {}
            while connections.join_next().await.is_some() {}
        };
        let _ = tokio::time::timeout(SHUTDOWN_DRAIN, drained).await;
        tasks.abort_all();
        connections.abort_all();
    }

    /// Runs `task` until it ends or `serve` does. After `serve` started shutting down the task is
    /// handed back instead.
    fn spawn<F: Future<Output = ()> + Send + 'static>(&self, task: F) -> Result<AbortHandle, F> {
        let mut guard = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(tasks) = guard.as_mut() else {
            return Err(task);
        };
        while tasks.try_join_next().is_some() {}
        Ok(tasks.spawn(task))
    }

    /// Closes the open tunnels that a new snapshot no longer allows.
    async fn enforce_policy_changes(self: Arc<Self>, stop: CancellationToken) {
        let mut policy = self.policy.clone();
        loop {
            tokio::select! {
                () = stop.cancelled() => return,
                changed = policy.changed() => if changed.is_err() { return },
            }
            let current = policy.borrow_and_update().clone();
            self.tunnels.close_disallowed(current.as_deref());
        }
    }

    async fn connection(self: Arc<Self>, stream: TcpStream, peer: IpAddr, root: CancellationToken) {
        let stop = root.clone();
        let service = service_fn(move |request| {
            let proxy = self.clone();
            let root = root.clone();
            async move { Ok::<_, Infallible>(proxy.handle(peer, request, root).await) }
        });
        let connection = http1::Builder::new()
            .serve_connection(TokioIo::new(stream), service)
            .with_upgrades();
        let mut connection = std::pin::pin!(connection);
        let result = tokio::select! {
            result = connection.as_mut() => result,
            () = stop.cancelled() => {
                // Finish the response in progress, close the connection when it is idle.
                connection.as_mut().graceful_shutdown();
                connection.await
            }
        };
        if let Err(error) = result {
            tracing::debug!(%error, %peer, "proxy connection ended with an error");
        }
    }

    async fn handle(
        self: Arc<Self>,
        peer: IpAddr,
        request: Request<Incoming>,
        root: CancellationToken,
    ) -> Response<ResponseBody> {
        // `decision` (and `host` if allowed) are set when the request ends.
        let span = tracing::info_span!(
            "gateway.connect",
            method = %request.method(),
            decision = tracing::field::Empty,
            host = tracing::field::Empty,
        );
        async move {
            if request.method() == Method::CONNECT {
                self.connect(peer, request, root).await
            } else {
                self.forward(peer, request, root).await
            }
        }
        .instrument(span)
        .await
    }

    /// The steps both kinds of request start with: the policy decision, the flow event and the
    /// resolution of the destination. `Err` is the final answer, its event is recorded.
    async fn admit(
        &self,
        peer: IpAddr,
        request: policy::Request<'_>,
        method: &str,
        target: &Target,
        root: &CancellationToken,
    ) -> Result<Admitted, Box<Response<ResponseBody>>> {
        let policy = self.policy.borrow().clone();
        let decision = policy::decide(policy.as_deref(), peer, request);
        let flow_id = self.last_flow.fetch_add(1, Ordering::Relaxed) + 1;
        let mut flow = Flow::start(
            &self.pod,
            flow_id,
            peer,
            decision.sandbox,
            method,
            target,
            self.metrics.clone(),
        );
        let rule = match decision.verdict {
            Ok(rule) => rule,
            Err(denial) => {
                flow.event.reason = denial.reason().into();
                self.finish(flow, Verdict::Denied);
                return Err(Box::new(reply(status_of(denial), denial.reason())));
            }
        };
        flow.event.matched_rule.clone_from(&rule.pattern);
        let resolved = tokio::select! {
            () = root.cancelled() => Err(SHUTDOWN),
            resolved = self.resolve(&target.host) => resolved,
        };
        match resolved {
            Ok(ip) => Ok(Admitted { flow, ip }),
            Err(refusal) => Err(Box::new(self.refuse(flow, &refusal))),
        }
    }

    // -----------------------------------------------------------------------------------------
    // CONNECT
    // -----------------------------------------------------------------------------------------

    async fn connect(
        self: Arc<Self>,
        peer: IpAddr,
        request: Request<Incoming>,
        root: CancellationToken,
    ) -> Response<ResponseBody> {
        let Some(target) = connect_target(request.uri()) else {
            return reply(StatusCode::BAD_REQUEST, "CONNECT needs host:port\n");
        };
        let connect = policy::Request::Connect {
            host: &target.host,
            port: target.port,
        };
        let Admitted { flow, ip } = match self.admit(peer, connect, "CONNECT", &target, &root).await
        {
            Ok(admitted) => admitted,
            Err(response) => return *response,
        };
        let upgrade = hyper::upgrade::on(request);
        let handle = self.tunnels.register(Tunnel::new(peer, &target, &root));
        let proxy = self.clone();
        let tunnel = async move {
            proxy
                .tunnel(
                    upgrade,
                    SocketAddr::new(ip, target.port),
                    target.host,
                    flow,
                    handle,
                )
                .await;
        };
        if let Err(tunnel) = self.spawn(tunnel) {
            // Stopping: the tunnel's token is cancelled, so it records its event and ends at
            // once. The upgrade is never answered.
            tunnel.await;
            return reply(SHUTDOWN.status, SHUTDOWN.reason);
        }
        Response::new(Empty::new().map_err(|never| match never {}).boxed())
    }

    /// Runs an established tunnel: checks the SNI against the `CONNECT` host, connects to the
    /// destination and copies bytes until either side ends or the tunnel is closed.
    async fn tunnel(
        &self,
        upgrade: OnUpgrade,
        destination: SocketAddr,
        host: String,
        mut flow: Flow,
        handle: TunnelHandle,
    ) {
        let traffic = Arc::new(Traffic::default());
        let cancel = handle.cancel.clone();
        let outcome = tokio::select! {
            () = cancel.cancelled() => {
                match handle.reason.get().copied() {
                    Some(reason) => Err((reason, Verdict::Denied)),
                    // The gateway stops; the policy did not deny anything.
                    None => Err(("gateway-shutdown", Verdict::Allowed)),
                }
            }
            outcome = self.relay(upgrade, destination, &host, &mut flow, &traffic) => outcome,
        };
        drop(handle);
        // The counters outlive the relay, so a tunnel that was closed still reports its bytes.
        flow.event.bytes_out = traffic.sent();
        flow.event.bytes_in = traffic.received();
        match outcome {
            Ok(()) => self.finish(flow, Verdict::Allowed),
            Err((reason, verdict)) => {
                flow.event.reason = reason.into();
                self.finish(flow, verdict);
            }
        }
    }

    async fn relay(
        &self,
        upgrade: OnUpgrade,
        destination: SocketAddr,
        host: &str,
        flow: &mut Flow,
        traffic: &Arc<Traffic>,
    ) -> Result<(), (&'static str, Verdict)> {
        let upstream_failed = ("upstream-failed", Verdict::Allowed);
        let mut client = TokioIo::new(upgrade.await.map_err(|_| upstream_failed)?);
        let hello = tokio::time::timeout(HELLO_TIMEOUT, sni::read_first_record(&mut client))
            .await
            .ok()
            .and_then(Result::ok);
        let sni = hello.as_deref().and_then(sni::server_name);
        flow.event.sni = sni.clone().unwrap_or_default();
        let (Some(hello), Some(_)) = (hello, sni.filter(|sni| sni == host)) else {
            // Dropping the client closes the tunnel before the destination saw a byte.
            return Err(("sni-mismatch", Verdict::Denied));
        };
        let upstream = self
            .dial(host, destination)
            .await
            .map_err(|_| upstream_failed)?;
        let mut upstream = Counted::new(upstream, traffic.clone());
        upstream
            .write_all(&hello)
            .await
            .map_err(|_| upstream_failed)?;
        flow.opened(&self.flows);
        copy_bidirectional(&mut client, &mut upstream)
            .await
            .map(|_| ())
            .map_err(|error| {
                tracing::debug!(%error, host, "tunnel ended with an error");
                ("tunnel-error", Verdict::Allowed)
            })
    }

    // -----------------------------------------------------------------------------------------
    // Plain HTTP
    // -----------------------------------------------------------------------------------------

    async fn forward(
        self: Arc<Self>,
        peer: IpAddr,
        mut request: Request<Incoming>,
        root: CancellationToken,
    ) -> Response<ResponseBody> {
        let Some(target) = http_target(request.uri()) else {
            return reply(
                StatusCode::BAD_REQUEST,
                "this is a proxy: use an absolute http:// URL\n",
            );
        };
        let http = policy::Request::Http {
            host: &target.host,
            port: target.port,
        };
        let method = request.method().to_string();
        let Admitted { mut flow, ip } = match self.admit(peer, http, &method, &target, &root).await
        {
            Ok(admitted) => admitted,
            Err(response) => return *response,
        };
        to_origin_form(&mut request, &target);
        let traffic = Arc::new(Traffic::default());
        let request = request.map(|body| Metered::new(body, traffic.clone(), Direction::Sent));
        let sent = tokio::select! {
            () = root.cancelled() => Err(SHUTDOWN.reason),
            sent = self.send_request(&target.host, SocketAddr::new(ip, target.port), request, &root) => sent,
        };
        match sent {
            Ok(response) => {
                flow.event.http_status = u32::from(response.status().as_u16());
                flow.opened(&self.flows);
                // The event is recorded when the response body ends (or is abandoned), with the
                // bytes of both bodies.
                let flows = self.flows.clone();
                let stop = root.clone();
                let (head, body) = response.into_parts();
                let body =
                    Metered::new(body, traffic, Direction::Received).on_end(move |traffic| {
                        flow.event.bytes_out = traffic.sent();
                        flow.event.bytes_in = traffic.received();
                        // A body cut off by the proxy stopping says so (it is not told apart
                        // from one that ended just before).
                        if stop.is_cancelled() {
                            flow.event.reason = SHUTDOWN.reason.into();
                        }
                        flow.finish(&flows, Verdict::Allowed);
                    });
                Response::from_parts(head, body.boxed())
            }
            Err(reason) => {
                // Stopping closes the upstream connection too, which can win the race.
                let reason = if root.is_cancelled() {
                    SHUTDOWN.reason
                } else {
                    reason
                };
                flow.event.reason = reason.into();
                flow.event.bytes_out = traffic.sent();
                flow.event.bytes_in = traffic.received();
                self.finish(flow, Verdict::Allowed);
                let status = if reason == SHUTDOWN.reason {
                    SHUTDOWN.status
                } else {
                    StatusCode::BAD_GATEWAY
                };
                reply(status, reason)
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Shared
    // -----------------------------------------------------------------------------------------

    /// Connects to `destination`, logging why it failed.
    async fn dial(&self, host: &str, destination: SocketAddr) -> std::io::Result<TcpStream> {
        self.dialer.dial(destination).await.inspect_err(|error| {
            tracing::warn!(host, address = %destination.ip(), %error, "connecting to the destination failed");
        })
    }

    /// Resolves `host` and returns the address to connect to. A name with any blocked address is
    /// refused as a whole, so a mixed answer cannot be used to reach an internal address.
    async fn resolve(&self, host: &str) -> Result<IpAddr, Refusal> {
        let addresses = match self.resolver.resolve(host).await {
            Ok(addresses) if !addresses.is_empty() => addresses,
            _ => {
                return Err(Refusal {
                    status: StatusCode::BAD_GATEWAY,
                    verdict: Verdict::Allowed,
                    reason: "resolve-failed",
                });
            }
        };
        if addresses.iter().any(|ip| self.blocked.contains(*ip)) {
            return Err(Refusal {
                status: StatusCode::FORBIDDEN,
                verdict: Verdict::Denied,
                reason: "private-destination",
            });
        }
        Ok(addresses[0])
    }

    /// Sends `request` (origin form) to `destination` over a fresh connection and waits for the
    /// response headers. The connection is driven by a task of this proxy until the response ends.
    async fn send_request(
        &self,
        host: &str,
        destination: SocketAddr,
        request: Request<Metered<Incoming>>,
        root: &CancellationToken,
    ) -> Result<Response<Incoming>, &'static str> {
        let mut connection_task = None;
        let exchange = async {
            let stream = self
                .dial(host, destination)
                .await
                .map_err(|_| "upstream-failed")?;
            let (mut sender, connection) =
                hyper::client::conn::http1::handshake(TokioIo::new(stream))
                    .await
                    .map_err(|_| "upstream-failed")?;
            let stop = root.clone();
            let driver = async move {
                tokio::select! {
                    () = stop.cancelled() => {}
                    result = connection => if let Err(error) = result {
                        tracing::debug!(%error, "upstream connection ended with an error");
                    },
                }
            };
            connection_task = Some(self.spawn(driver).map_err(|_| SHUTDOWN.reason)?);
            sender
                .send_request(request)
                .await
                .map_err(|_| "upstream-failed")
        };
        let result = tokio::time::timeout(UPSTREAM_TIMEOUT, exchange)
            .await
            .map_err(|_| "upstream-timeout")
            .and_then(|result| result);
        if result.is_err()
            && let Some(task) = connection_task
        {
            // No response will come: close the connection instead of leaving it waiting.
            task.abort();
        }
        result
    }

    fn refuse(&self, mut flow: Flow, refusal: &Refusal) -> Response<ResponseBody> {
        flow.event.reason = refusal.reason.into();
        self.finish(flow, refusal.verdict);
        reply(refusal.status, refusal.reason)
    }

    fn finish(&self, flow: Flow, verdict: Verdict) {
        flow.finish(&self.flows, verdict);
    }
}

/// A request that passed the policy: its event so far and the address to connect to.
struct Admitted {
    flow: Flow,
    ip: IpAddr,
}

/// The event of one request while it is handled.
struct Flow {
    event: FlowEvent,
    started: Instant,
    metrics: Arc<Metrics>,
    /// The `gateway.connect` span of the request; closed once the flow is finished.
    span: tracing::Span,
    /// Connected to the destination (counted in `df.gateway.active_connections`).
    open: bool,
}

impl Flow {
    fn start(
        pod: &str,
        flow_id: u64,
        source: IpAddr,
        sandbox: Option<&Sandbox>,
        method: &str,
        target: &Target,
        metrics: Arc<Metrics>,
    ) -> Self {
        let sandbox = sandbox
            .cloned()
            .map(|s| (s.run_id, s.task_key, s.project_key, s.persona_id));
        let (run_id, task_key, project_key, persona_id) = sandbox.unwrap_or_default();
        Self {
            event: FlowEvent {
                gateway_pod: pod.to_string(),
                flow_id,
                source_ip: source.to_string(),
                run_id,
                task_key,
                project_key,
                persona_id,
                method: method.to_string(),
                host: target.host.clone(),
                port: u32::from(target.port),
                ..Default::default()
            },
            started: Instant::now(),
            metrics,
            span: tracing::Span::current(),
            open: false,
        }
    }

    /// Records that the destination is connected (the request is allowed and under way).
    fn opened(&mut self, flows: &FlowLog) {
        self.open = true;
        self.metrics.opened();
        let mut event = self.event.clone();
        event.phase = Phase::Opened.into();
        event.verdict = Verdict::Allowed.into();
        event.time = Some(timestamp_now());
        event.duration = Some(duration_proto(self.started.elapsed()));
        flows.record(event);
    }

    fn finish(self, flows: &FlowLog, verdict: Verdict) {
        let mut event = self.event;
        self.metrics.closed(
            self.open,
            verdict,
            &event.reason,
            event.bytes_out,
            event.bytes_in,
        );
        self.span.record("decision", Decision::of(verdict).as_str());
        if verdict != Verdict::Denied {
            self.span.record("host", event.host.as_str());
        }
        event.phase = Phase::Closed.into();
        event.time = Some(timestamp_now());
        event.verdict = verdict.into();
        event.duration = Some(duration_proto(self.started.elapsed()));
        flows.record(event);
    }
}

fn unix_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_micros()).unwrap_or(0))
}

fn status_of(denial: policy::Denial) -> StatusCode {
    StatusCode::from_u16(denial.status()).unwrap_or(StatusCode::FORBIDDEN)
}

fn reply(status: StatusCode, message: &str) -> Response<ResponseBody> {
    let mut response = Response::new(
        Full::new(Bytes::from(format!("egress: {message}\n")))
            .map_err(|never| match never {})
            .boxed(),
    );
    *response.status_mut() = status;
    response
}

fn connect_target(uri: &Uri) -> Option<Target> {
    let authority = uri.authority()?;
    Some(Target {
        host: host_of(authority.host()),
        port: authority.port_u16().unwrap_or(443),
    })
}

fn http_target(uri: &Uri) -> Option<Target> {
    if uri.scheme_str() != Some("http") {
        return None;
    }
    let authority = uri.authority()?;
    Some(Target {
        host: host_of(authority.host()),
        port: authority.port_u16().unwrap_or(80),
    })
}

/// Lower case, without the brackets of an IPv6 literal or a trailing dot.
fn host_of(host: &str) -> String {
    normalize_host(strip_brackets(host))
}

/// Rewrites an absolute-form request for the origin server: path only, `Host` of the target, no
/// proxy headers.
fn to_origin_form<B>(request: &mut Request<B>, target: &Target) {
    let path = request
        .uri()
        .path_and_query()
        .map_or("/", |path| path.as_str())
        .to_string();
    if let Ok(uri) = path.parse() {
        *request.uri_mut() = uri;
    }
    for header in &PROXY_HEADERS {
        request.headers_mut().remove(header);
    }
    let host = if target.port == 80 {
        target.host.clone()
    } else {
        format!("{}:{}", target.host, target.port)
    };
    if let Ok(value) = host.parse() {
        request.headers_mut().insert(HOST, value);
    }
}

// ---------------------------------------------------------------------------------------------
// Open tunnels
// ---------------------------------------------------------------------------------------------

/// Tunnels that are open at the moment, so a snapshot that removes their host can close them.
#[derive(Default)]
struct Tunnels {
    open: Mutex<Vec<Tunnel>>,
    next_id: AtomicU64,
}

struct Tunnel {
    id: u64,
    source: IpAddr,
    host: String,
    port: u16,
    cancel: CancellationToken,
    /// Why the tunnel was cancelled, set before it is.
    reason: Arc<OnceLock<&'static str>>,
}

impl Tunnel {
    fn new(source: IpAddr, target: &Target, root: &CancellationToken) -> Self {
        Self {
            id: 0,
            source,
            host: target.host.clone(),
            port: target.port,
            cancel: root.child_token(),
            reason: Arc::default(),
        }
    }
}

/// Removes its tunnel from the registry when dropped.
struct TunnelHandle {
    registry: Arc<Tunnels>,
    id: u64,
    cancel: CancellationToken,
    reason: Arc<OnceLock<&'static str>>,
}

impl Drop for TunnelHandle {
    fn drop(&mut self) {
        self.registry.lock().retain(|tunnel| tunnel.id != self.id);
    }
}

impl Tunnels {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Tunnel>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn register(self: &Arc<Self>, mut tunnel: Tunnel) -> TunnelHandle {
        tunnel.id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let handle = TunnelHandle {
            registry: self.clone(),
            id: tunnel.id,
            cancel: tunnel.cancel.clone(),
            reason: tunnel.reason.clone(),
        };
        self.lock().push(tunnel);
        handle
    }

    /// Cancels every tunnel that `policy` would deny as a new connection.
    fn close_disallowed(&self, policy: Option<&Policy>) {
        for tunnel in self.lock().iter() {
            let request = policy::Request::Connect {
                host: &tunnel.host,
                port: tunnel.port,
            };
            if let Err(denial) = policy::decide(policy, tunnel.source, request).verdict {
                // The host left the run's allowlist; the other denials say what changed.
                let reason = match denial {
                    policy::Denial::NotAllowlisted => "removed-from-allowlist",
                    other => other.reason(),
                };
                // Set once: the first reason stays.
                let _ = tunnel.reason.set(reason);
                tunnel.cancel.cancel();
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
