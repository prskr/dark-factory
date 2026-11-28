//! Test fixtures of the MCP endpoint: fake upstreams behind servers of a snapshot, the endpoint
//! behind a plain-HTTP listener, and a collector for call records.

mod call;
mod endpoint;
mod list;

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use df_proto::gateway::v1 as pb;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::HeaderMap;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::{JoinHandle, JoinSet};

use super::report::CallSink;
use super::testing::StaticResolver;
use super::*;
use crate::policy::Policy;

pub(super) const POD: &str = "10.1.0.7";
pub(super) const RUN: &str = "0197a000-0000-7000-8000-000000000001";

/// Collects call records.
#[derive(Default)]
pub(super) struct Records {
    records: Mutex<Vec<pb::McpCallRecord>>,
    added: Notify,
}

impl Records {
    pub(super) fn all(&self) -> Vec<pb::McpCallRecord> {
        self.records.lock().unwrap().clone()
    }

    /// `(server, tool, outcome)` of every record.
    pub(super) fn summary(&self) -> Vec<(String, String, df_core::mcp::McpCallOutcome)> {
        self.all()
            .into_iter()
            .map(|r| {
                (
                    r.server_name,
                    r.tool,
                    crate::convert::mcp_outcome_from_proto(r.outcome).unwrap(),
                )
            })
            .collect()
    }

    /// Waits until `count` records exist; the timeout only keeps a broken test from hanging.
    pub(super) async fn wait_for(&self, count: usize) {
        let wait = async {
            loop {
                let added = self.added.notified();
                tokio::pin!(added);
                added.as_mut().enable();
                if self.records.lock().unwrap().len() >= count {
                    return;
                }
                added.await;
            }
        };
        tokio::time::timeout(Duration::from_secs(10), wait)
            .await
            .expect("the records did not arrive");
    }
}

impl CallSink for Records {
    fn record(&self, record: pb::McpCallRecord) {
        self.records.lock().unwrap().push(record);
        self.added.notify_waiters();
    }
}

/// A server of the snapshot pointing at `url`, reachable on the loopback address ("Private
/// network") with the bearer credential `tok-<id>`.
pub(super) fn server(id: &str, name: &str, url: &str, version: u64) -> pb::McpServer {
    pb::McpServer {
        id: id.into(),
        name: name.into(),
        url: url.into(),
        credential_ref: format!("cred-{id}"),
        credential_scheme: Some(pb::mcp_server::CredentialScheme::Bearer(
            pb::McpBearerScheme {},
        )),
        private_network: true,
        config_version: version,
    }
}

/// A snapshot with one run at [`POD`] that has `enabled` servers, all of `servers` registered.
/// A server whose id starts with `nocred` has no credential in the snapshot.
pub(super) fn policy(servers: &[pb::McpServer], enabled: &[&str]) -> Policy {
    Policy::from_update(pb::PolicyUpdate {
        version: 1,
        snapshot: Some(pb::Snapshot {
            sandboxes: vec![pb::SandboxEntry {
                pod_ip: POD.into(),
                run_id: RUN.into(),
                task_key: "DF-1".into(),
                project_key: "DF".into(),
                mcp_server_ids: enabled.iter().map(|id| (*id).to_owned()).collect(),
                ..Default::default()
            }],
            mcp_servers: servers.iter().map(|s| (s.id.clone(), s.clone())).collect(),
            provider_credentials: servers
                .iter()
                .filter(|s| !s.id.starts_with("nocred"))
                .map(|s| {
                    (
                        s.credential_ref.clone(),
                        pb::ProviderCredential {
                            value: format!("tok-{}", s.id),
                        },
                    )
                })
                .collect(),
            ..Default::default()
        }),
    })
}

/// The endpoint behind a plain-HTTP listener; every connection counts as coming from [`POD`].
pub(super) struct Env {
    pub mcp: Arc<Mcp>,
    pub records: Arc<Records>,
    pub policy: Arc<Mutex<Option<Arc<Policy>>>>,
    addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Env {
    pub(super) async fn start(policy: Option<Policy>) -> Self {
        Self::start_with(policy, HashMap::new(), BlockedRanges::none()).await
    }

    pub(super) async fn start_with(
        policy: Option<Policy>,
        hosts: HashMap<String, Vec<IpAddr>>,
        blocked: BlockedRanges,
    ) -> Self {
        let records = Arc::new(Records::default());
        let mcp = Arc::new(
            Mcp::new(
                UpstreamConnector::new().unwrap().allowing_plain_http(true),
                Arc::new(StaticResolver(hosts)),
                blocked,
                records.clone(),
            )
            .with_list_timeout(Duration::from_millis(500)),
        );
        let policy = Arc::new(Mutex::new(policy.map(Arc::new)));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (mcp_in, policy_in) = (mcp.clone(), policy.clone());
        let task = tokio::spawn(async move {
            // The connections belong to this task: aborting it ends them too.
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _peer)) = accepted else { break };
                        let (mcp, policy) = (mcp_in.clone(), policy_in.clone());
                        connections.spawn(serve_front_connection(stream, mcp, policy));
                    }
                    Some(_finished) = connections.join_next() => {}
                }
            }
        });
        Self {
            mcp,
            records,
            policy,
            addr,
            task,
        }
    }

    pub(super) fn set_policy(&self, policy: Policy) {
        *self.policy.lock().unwrap() = Some(Arc::new(policy));
    }

    /// Sends a request and returns the status, headers and body text.
    pub(super) async fn send(
        &self,
        method: Method,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (StatusCode, HeaderMap, String) {
        let stream = tokio::net::TcpStream::connect(self.addr).await.unwrap();
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake::<_, Full<Bytes>>(TokioIo::new(stream))
                .await
                .unwrap();
        let mut request = Request::builder()
            .method(method)
            .uri("/mcp")
            .header("host", "gateway");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let exchange = async {
            let response = sender
                .send_request(
                    request
                        .body(Full::new(Bytes::from(body.to_owned())))
                        .unwrap(),
                )
                .await
                .unwrap();
            let (parts, body) = response.into_parts();
            let body = body.collect().await.unwrap().to_bytes();
            (
                parts.status,
                parts.headers,
                String::from_utf8_lossy(&body).into_owned(),
            )
        };
        // The connection is driven by this call, so nothing outlives it.
        tokio::select! {
            answer = exchange => answer,
            _ = connection => panic!("the connection ended before the answer"),
        }
    }

    /// `initialize` and the session id of the answer.
    pub(super) async fn session(&self) -> String {
        let body = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {"sampling": {}}}});
        let (status, headers, _text) = self.send(Method::POST, &[], &body.to_string()).await;
        assert_eq!(status, StatusCode::OK);
        headers["mcp-session-id"].to_str().unwrap().to_owned()
    }

    /// A JSON-RPC request on `session`; the answer's JSON.
    pub(super) async fn rpc(&self, session: &str, method: &str, params: Value) -> Value {
        let (status, _headers, text) = self.rpc_raw(session, method, params).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        serde_json::from_str(&text).unwrap()
    }

    pub(super) async fn rpc_raw(
        &self,
        session: &str,
        method: &str,
        params: Value,
    ) -> (StatusCode, HeaderMap, String) {
        let body = json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params});
        self.send(
            Method::POST,
            &[
                ("mcp-session-id", session),
                ("authorization", "Bearer stolen"),
                ("context7_api_key", "x"),
            ],
            &body.to_string(),
        )
        .await
    }

    /// The tool names of a `tools/list`.
    pub(super) async fn tool_names(&self, session: &str) -> Vec<String> {
        let answer = self.rpc(session, "tools/list", json!({})).await;
        answer["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect()
    }
}

/// Serves one connection of the front: every request is the endpoint's, from [`POD`].
async fn serve_front_connection(
    stream: tokio::net::TcpStream,
    mcp: Arc<Mcp>,
    policy: Arc<Mutex<Option<Arc<Policy>>>>,
) {
    let service = service_fn(move |request: Request<Incoming>| {
        let (mcp, policy) = (mcp.clone(), policy.clone());
        async move {
            let current = policy.lock().unwrap().clone();
            let source = POD.parse().unwrap();
            Ok::<_, Infallible>(mcp.handle(current.as_deref(), source, request).await)
        }
    });
    http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await
        .ok();
}

impl Drop for Env {
    fn drop(&mut self) {
        self.task.abort();
    }
}
