//! `GatewayLog.Tail`: the gateway pod's flow events for the control plane.

use std::pin::Pin;
use std::sync::Arc;

use df_proto::gateway::v1::gateway_log_server::{GatewayLog, GatewayLogServer};
use df_proto::gateway::v1::{FlowEvent, TailRequest};
use futures::{Stream, StreamExt, stream};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

use crate::flow::{FlowLog, matches};
use crate::peer::require_dns_san;

/// Serves `Tail` from the ring buffer.
pub struct TailService {
    flows: Arc<FlowLog>,
}

type TailStream = Pin<Box<dyn Stream<Item = Result<FlowEvent, Status>> + Send>>;

impl TailService {
    pub fn new(flows: Arc<FlowLog>) -> Self {
        Self { flows }
    }
}

#[tonic::async_trait]
impl GatewayLog for TailService {
    type TailStream = TailStream;

    async fn tail(&self, request: Request<TailRequest>) -> Result<Response<TailStream>, Status> {
        let filter = request.into_inner().filter.unwrap_or_default();
        let (backlog, live) = self.flows.subscribe(&filter);
        let backlog_end = FlowEvent {
            backlog_end: true,
            ..Default::default()
        };
        let live = BroadcastStream::new(live).filter_map(move |received| {
            std::future::ready(match received {
                Ok(event) => matches(&filter, &event).then_some(Ok(event)),
                // The subscriber was too slow: tell the viewer that events are missing.
                Err(BroadcastStreamRecvError::Lagged(_)) => Some(Ok(FlowEvent {
                    gap: true,
                    ..Default::default()
                })),
            })
        });
        let events = stream::iter(backlog.into_iter().chain([backlog_end]).map(Ok)).chain(live);
        Ok(Response::new(Box::pin(events)))
    }
}

/// Serves `Tail` on `incoming` until `shutdown` resolves. With `client_name` only a peer whose
/// certificate has that DNS name may call it.
pub async fn serve<I, IO>(
    flows: Arc<FlowLog>,
    incoming: I,
    client_name: Option<String>,
    shutdown: impl Future<Output = ()>,
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
    let guard = move |request: Request<()>| match &client_name {
        Some(name) => require_dns_san(&request, name).map(|()| request),
        None => Ok(request),
    };
    Server::builder()
        .add_service(GatewayLogServer::with_interceptor(
            TailService::new(flows),
            guard,
        ))
        .serve_with_incoming_shutdown(incoming, shutdown)
        .await
}

#[cfg(test)]
mod tests {
    use df_proto::gateway::v1::gateway_log_client::GatewayLogClient;
    use df_proto::gateway::v1::{FlowFilter, Verdict};
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;

    use super::*;

    fn event(host: &str, verdict: Verdict) -> FlowEvent {
        FlowEvent {
            host: host.into(),
            verdict: verdict.into(),
            ..Default::default()
        }
    }

    async fn next_event(
        stream: &mut tonic::Streaming<FlowEvent>,
    ) -> Result<FlowEvent, Box<dyn std::error::Error>> {
        Ok(
            tokio::time::timeout(std::time::Duration::from_secs(5), stream.message())
                .await??
                .ok_or("stream ended")?,
        )
    }

    async fn client(flows: Arc<FlowLog>) -> GatewayLogClient<tonic::transport::Channel> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve(
            flows,
            TcpListenerStream::new(listener),
            None,
            std::future::pending(),
        ));
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect()
            .await
            .unwrap();
        GatewayLogClient::new(channel)
    }

    #[tokio::test]
    async fn tail_streams_the_backlog_a_marker_and_then_live_events()
    -> Result<(), Box<dyn std::error::Error>> {
        let flows = Arc::new(FlowLog::new(10));
        flows.record(event("old.example.com", Verdict::Allowed));
        let mut client = client(flows.clone()).await;
        let mut stream = client.tail(TailRequest::default()).await?.into_inner();

        assert_eq!(next_event(&mut stream).await?.host, "old.example.com");
        assert!(next_event(&mut stream).await?.backlog_end);
        flows.record(event("new.example.com", Verdict::Denied));
        assert_eq!(next_event(&mut stream).await?.host, "new.example.com");
        Ok(())
    }

    #[tokio::test]
    async fn tail_with_a_denied_filter_streams_only_denied_events()
    -> Result<(), Box<dyn std::error::Error>> {
        let flows = Arc::new(FlowLog::new(10));
        flows.record(event("ok-old.example.com", Verdict::Allowed));
        flows.record(event("bad-old.example.com", Verdict::Denied));
        let mut client = client(flows.clone()).await;
        let request = TailRequest {
            filter: Some(FlowFilter {
                verdict: Verdict::Denied.into(),
                ..Default::default()
            }),
        };
        let mut stream = client.tail(request).await?.into_inner();
        assert_eq!(next_event(&mut stream).await?.host, "bad-old.example.com");
        assert!(next_event(&mut stream).await?.backlog_end);
        flows.record(event("ok-new.example.com", Verdict::Allowed));
        flows.record(event("bad-new.example.com", Verdict::Denied));
        assert_eq!(next_event(&mut stream).await?.host, "bad-new.example.com");
        Ok(())
    }

    #[tokio::test]
    async fn a_lagging_subscriber_gets_a_gap_marker() -> Result<(), Status> {
        let flows = Arc::new(FlowLog::new(10));
        let service = TailService::new(flows.clone());
        let mut stream = service
            .tail(Request::new(TailRequest::default()))
            .await?
            .into_inner();
        assert!(stream.next().await.unwrap()?.backlog_end);
        // More than the live channel holds, while nobody reads.
        for n in 0..5000 {
            flows.record(event(&n.to_string(), Verdict::Allowed));
        }
        assert!(stream.next().await.unwrap()?.gap);
        Ok(())
    }
}
