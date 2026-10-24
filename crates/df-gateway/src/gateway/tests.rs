use std::net::SocketAddr;
use std::pin::Pin;
use std::time::Duration;

use anyhow::Result;
use df_proto::gateway::v1::gateway_policy_server::{GatewayPolicy, GatewayPolicyServer};
use df_proto::gateway::v1::{HostRule, PolicyUpdate, PolicyWatchRequest, SandboxEntry, Snapshot};
use futures::{Stream, StreamExt};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, TcpListenerStream};
use tonic::{Request, Response, Status};

use super::*;
use crate::link::PlainConnector;
use crate::policy::Policy;
use crate::proxy::tests::{RedirectDialer, StubResolver, connect_head, request};

/// A control plane that sends `initial` on connect and then whatever is sent to `updates`.
struct FakeControlPlane {
    initial: PolicyUpdate,
    updates: broadcast::Sender<PolicyUpdate>,
    /// Ends the streams, like a replica that shuts down.
    stop: CancellationToken,
}

#[tonic::async_trait]
impl GatewayPolicy for FakeControlPlane {
    type PolicyWatchStream = Pin<Box<dyn Stream<Item = Result<PolicyUpdate, Status>> + Send>>;

    async fn policy_watch(
        &self,
        _: Request<PolicyWatchRequest>,
    ) -> Result<Response<Self::PolicyWatchStream>, Status> {
        let updates = BroadcastStream::new(self.updates.subscribe())
            .filter_map(|u| async { u.ok().map(Ok) })
            .take_until(self.stop.clone().cancelled_owned());
        let first = futures::stream::once(std::future::ready(Ok(self.initial.clone())));
        Ok(Response::new(Box::pin(first.chain(updates))))
    }
}

fn snapshot(version: u64, hosts: &[&str]) -> PolicyUpdate {
    PolicyUpdate {
        version,
        snapshot: Some(Snapshot {
            sandboxes: vec![SandboxEntry {
                pod_ip: "127.0.0.1".into(),
                run_id: "run-1".into(),
                project_key: "DF".into(),
                hosts: hosts
                    .iter()
                    .map(|host| HostRule {
                        pattern: (*host).into(),
                        port: 443,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }],
        }),
    }
}

/// A running fake control plane; stopping it ends its streams and its server.
struct ControlPlane {
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl ControlPlane {
    async fn stop(self) {
        self.stop.cancel();
        self.task.await.unwrap();
    }
}

fn serve_control_plane(addr: SocketAddr, initial: PolicyUpdate) -> ControlPlane {
    let listener = std::net::TcpListener::bind(addr).unwrap();
    listener.set_nonblocking(true).unwrap();
    let listener = TcpListener::from_std(listener).unwrap();
    let stop = CancellationToken::new();
    let service = FakeControlPlane {
        initial,
        updates: broadcast::channel(8).0,
        stop: stop.clone(),
    };
    let signal = stop.clone().cancelled_owned();
    let task = tokio::spawn(async move {
        let _ = tonic::transport::Server::builder()
            .add_service(GatewayPolicyServer::new(service))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), signal)
            .await;
    });
    ControlPlane { stop, task }
}

struct Running {
    proxy: SocketAddr,
    health: SocketAddr,
    shutdown: CancellationToken,
    task: JoinHandle<Result<(), GatewayError>>,
    policy: watch::Receiver<Option<Arc<Policy>>>,
}

async fn start_gateway(control_plane: SocketAddr) -> Result<Running> {
    start_gateway_with(control_plane, Arc::default()).await
}

async fn start_gateway_with(
    control_plane: SocketAddr,
    metrics: Arc<crate::metrics::Metrics>,
) -> Result<Running> {
    let health = TcpListener::bind("127.0.0.1:0").await?;
    let proxy = TcpListener::bind("127.0.0.1:0").await?;
    let tail = TcpListener::bind("127.0.0.1:0").await?;
    let running = (proxy.local_addr()?, health.local_addr()?);
    let shutdown = CancellationToken::new();
    let settings = Settings {
        pod: "gw-0".into(),
        control_plane: control_plane.to_string(),
        buffer_size: 100,
        blocked_cidrs: vec![],
        retry: Duration::from_millis(50),
        control_plane_name: None,
        metrics,
    };
    let outside = Outside {
        connector: Arc::new(PlainConnector),
        resolver: Arc::new(StubResolver::of(&[("api.anthropic.com", "203.0.113.10")])),
        dialer: Arc::new(RedirectDialer {
            to: "127.0.0.1:1".parse()?,
        }),
    };
    let (policy_tx, policy) = watch::channel(None);
    let task = tokio::spawn(run(
        settings,
        policy_tx,
        outside,
        health,
        proxy,
        TcpListenerStream::new(tail),
        shutdown.clone(),
    ));
    Ok(Running {
        proxy: running.0,
        health: running.1,
        shutdown,
        task,
        policy,
    })
}

async fn ready_status(health: SocketAddr) -> Result<u16> {
    let (_, response) = request(
        health,
        "GET /readyz HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await?;
    Ok(response[9..12].parse()?)
}

/// Waits until the policy of the gateway satisfies `condition`.
async fn wait_for_policy(
    gateway: &Running,
    condition: impl Fn(&Option<Arc<Policy>>) -> bool,
) -> Result<()> {
    let mut policy = gateway.policy.clone();
    tokio::time::timeout(Duration::from_secs(5), policy.wait_for(condition)).await??;
    Ok(())
}

async fn connect_status(proxy: SocketAddr, host: &str) -> Result<String> {
    let (_, response) = request(proxy, &connect_head(host, 443)).await?;
    Ok(response[9..12].to_string())
}

#[tokio::test]
async fn gateway_that_starts_before_the_control_plane_denies_with_503_until_the_snapshot_arrives()
-> Result<()> {
    let free = std::net::TcpListener::bind("127.0.0.1:0")?;
    let control_plane = free.local_addr()?;
    drop(free);
    let gateway = start_gateway(control_plane).await?;

    assert_eq!(ready_status(gateway.health).await?, 503);
    assert_eq!(
        connect_status(gateway.proxy, "api.anthropic.com").await?,
        "503"
    );

    let _server = serve_control_plane(control_plane, snapshot(1, &["api.anthropic.com"]));
    wait_for_policy(&gateway, Option::is_some).await?;
    assert_eq!(ready_status(gateway.health).await?, 200);
    assert_eq!(
        connect_status(gateway.proxy, "api.anthropic.com").await?,
        "200"
    );
    assert_eq!(connect_status(gateway.proxy, "example.com").await?, "403");

    gateway.shutdown.cancel();
    gateway.task.await??;
    Ok(())
}

#[tokio::test]
async fn healthz_is_up_without_a_snapshot() -> Result<()> {
    let free = std::net::TcpListener::bind("127.0.0.1:0")?;
    let control_plane = free.local_addr()?;
    drop(free);
    let gateway = start_gateway(control_plane).await?;
    let (_, response) = request(
        gateway.health,
        "GET /healthz HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    )
    .await?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    gateway.shutdown.cancel();
    gateway.task.await??;
    Ok(())
}

#[tokio::test]
async fn gateway_keeps_its_policy_while_the_control_plane_is_gone_and_resyncs_after_it_returns()
-> Result<()> {
    let free = std::net::TcpListener::bind("127.0.0.1:0")?;
    let control_plane = free.local_addr()?;
    drop(free);
    let first = serve_control_plane(control_plane, snapshot(1, &["api.anthropic.com"]));
    let gateway = start_gateway(control_plane).await?;
    wait_for_policy(&gateway, Option::is_some).await?;
    assert_eq!(ready_status(gateway.health).await?, 200);
    assert_eq!(
        connect_status(gateway.proxy, "api.anthropic.com").await?,
        "200"
    );

    // The control plane replica restarts: policy stays in force and the pod stays ready.
    first.stop().await;
    assert_eq!(ready_status(gateway.health).await?, 200);
    assert_eq!(
        connect_status(gateway.proxy, "api.anthropic.com").await?,
        "200"
    );

    // Another replica comes up with a newer snapshot that no longer allows the host.
    let _second = serve_control_plane(control_plane, snapshot(2, &[]));
    wait_for_policy(&gateway, |policy| {
        policy.as_ref().is_some_and(|policy| policy.version == 2)
    })
    .await?;
    assert_eq!(
        connect_status(gateway.proxy, "api.anthropic.com").await?,
        "403"
    );
    gateway.shutdown.cancel();
    gateway.task.await??;
    Ok(())
}

#[tokio::test]
async fn the_applied_policy_version_is_exported() -> Result<()> {
    let probe = df_agent_channel::metrics::probe::Probe::new();
    let free = std::net::TcpListener::bind("127.0.0.1:0")?;
    let control_plane = free.local_addr()?;
    drop(free);
    let _control_plane = serve_control_plane(control_plane, snapshot(7, &["api.anthropic.com"]));
    let metrics = Arc::new(crate::metrics::Metrics::new(&probe.meter()));
    let gateway = start_gateway_with(control_plane, metrics).await?;

    wait_for_policy(&gateway, |policy| {
        policy.as_ref().is_some_and(|policy| policy.version == 7)
    })
    .await?;

    assert_eq!(probe.total("df.gateway.policy_version", &[]), 7.0);
    gateway.shutdown.cancel();
    gateway.task.await??;
    Ok(())
}

#[tokio::test]
async fn gateway_rejects_bad_blocked_cidrs() -> Result<()> {
    let health = TcpListener::bind("127.0.0.1:0").await?;
    let proxy = TcpListener::bind("127.0.0.1:0").await?;
    let tail = TcpListener::bind("127.0.0.1:0").await?;
    let result = run(
        Settings {
            pod: "gw".into(),
            control_plane: "127.0.0.1:1".into(),
            buffer_size: 1,
            blocked_cidrs: vec!["nonsense".into()],
            retry: Duration::from_secs(1),
            control_plane_name: None,
            metrics: Arc::default(),
        },
        watch::channel(None).0,
        Outside {
            connector: Arc::new(PlainConnector),
            resolver: Arc::new(StubResolver::of(&[])),
            dialer: Arc::new(RedirectDialer {
                to: "127.0.0.1:1".parse()?,
            }),
        },
        health,
        proxy,
        TcpListenerStream::new(tail),
        CancellationToken::new(),
    )
    .await;
    assert!(matches!(result, Err(GatewayError::BlockedCidr(_))));
    Ok(())
}
