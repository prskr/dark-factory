//! The control plane's internal gateway server: `PolicyWatch`, `GatewayGit`, `GatewayEvents` and
//! the stale push scan on one port.

use df_proto::gateway::v1::gateway_events_server::GatewayEventsServer;
use df_proto::gateway::v1::gateway_git_server::GatewayGitServer;
use df_proto::gateway::v1::gateway_policy_server::GatewayPolicyServer;
use futures::Stream;
use tokio_util::sync::CancellationToken;
use tonic::Request;
use tonic::transport::Server;

use super::denial_events::DenialEvents;
use super::git_records::{GitRecords, scan_stale_pushes};
use super::policy_service::PolicyService;
use crate::peer::require_dns_san;

/// Serves `PolicyWatch`, `GatewayGit` and `GatewayEvents` on `incoming`, and runs the stale push
/// scan, until
/// `shutdown` is cancelled. Every replica runs the scan; it is idempotent. Only a peer whose
/// certificate has the DNS name `gateway_name` may call; `None` accepts any peer the TLS layer
/// let in (tests without TLS).
pub async fn serve_internal<I, IO>(
    service: PolicyService,
    incoming: I,
    gateway_name: Option<String>,
    shutdown: CancellationToken,
) -> Result<(), tonic::transport::Error>
where
    I: Stream<Item = Result<IO, std::io::Error>>,
    IO: tokio::io::AsyncRead
        + tokio::io::AsyncWrite
        + tonic::transport::server::Connected
        + Unpin
        + Send
        + 'static,
{
    let guard = move |request: Request<()>| match &gateway_name {
        Some(name) => require_dns_san(&request, name).map(|()| request),
        None => Ok(request),
    };
    let records = GitRecords::new(service.pool.clone());
    let denials = DenialEvents::new(service.pool.clone());
    let scan = tokio::spawn(scan_stale_pushes(service.pool.clone(), shutdown.clone()));
    let served = Server::builder()
        .add_service(GatewayPolicyServer::with_interceptor(
            service,
            guard.clone(),
        ))
        .add_service(GatewayGitServer::with_interceptor(records, guard.clone()))
        .add_service(GatewayEventsServer::with_interceptor(denials, guard))
        .serve_with_incoming_shutdown(incoming, shutdown.clone().cancelled_owned())
        .await;
    scan.abort();
    served
}
