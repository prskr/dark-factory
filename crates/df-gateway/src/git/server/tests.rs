use std::sync::Mutex;

use hyper::header::AUTHORIZATION;

use super::*;
use crate::git::mirror::Mirrors;
use crate::git::upstream::ForgeConnector;
use crate::policy::Sandbox;

const POD: &str = "10.1.0.7";
const OTHER_POD: &str = "10.1.0.8";

/// What the fake upstream saw.
struct Forwarded {
    route: GitRoute,
    task_key: String,
    service: Service,
    operation: Operation,
    headers: HeaderMap,
    /// `None` if reading the body failed.
    body: Option<Bytes>,
}

#[derive(Default)]
struct Recording(Mutex<Vec<Forwarded>>);

#[async_trait]
impl Upstream for Recording {
    async fn forward(&self, forge: ForgeTarget<'_>, request: GitRequest) -> Response<Body> {
        let route = forge.route;
        let body = request
            .body
            .collect()
            .await
            .ok()
            .map(http_body_util::Collected::to_bytes);
        let status = if body.is_some() {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        };
        self.0.lock().unwrap().push(Forwarded {
            route: route.clone(),
            task_key: request.task_key.to_string(),
            service: request.service,
            operation: request.operation,
            headers: request.headers,
            body,
        });
        text(status, "forwarded")
    }
}

fn route(credential_ref: &str) -> GitRoute {
    GitRoute {
        upstream_url: "https://forge.example/acme/app.git".into(),
        base_branch: "refs/heads/main".into(),
        task_branch: "refs/heads/factory/task/DF-1".into(),
        wip_branch: "refs/heads/factory/wip/DF-1".into(),
        credential_ref: credential_ref.into(),
        commit_name: "factory".into(),
        commit_email: "factory@example.com".into(),
    }
}

fn sandbox(task_key: &str) -> Sandbox {
    Sandbox {
        run_id: "run-1".into(),
        task_key: task_key.into(),
        run_number: 1,
        project_key: "DF".into(),
        persona_id: "dev".into(),
        hosts: Vec::new(),
    }
}

/// `DF-1` at [`POD`] with a repository, `DF-2` at [`OTHER_POD`] without one.
fn policy(credential_ref: &str) -> Policy {
    Policy::new(
        1,
        [
            (POD.parse().unwrap(), sandbox("DF-1")),
            (OTHER_POD.parse().unwrap(), sandbox("DF-2")),
        ],
    )
    .with_git(
        [(POD.parse().unwrap(), route(credential_ref))],
        [(
            "forge".to_string(),
            ForgeCredential {
                username: "bot".into(),
                password: "secret".into(),
            },
        )],
    )
}

fn push() -> Arc<Push> {
    let dir = Path::new("/nonexistent/df-gateway-test");
    let mirrors = Mirrors::new(dir, ForgeConnector::new().unwrap());
    Arc::new(Push::new(Arc::new(mirrors), dir, 1024))
}

fn endpoint(policy: Option<Policy>) -> (GitEndpoint, Arc<Recording>) {
    let upstream = Arc::new(Recording::default());
    let (_, rx) = watch::channel(policy.map(Arc::new));
    (GitEndpoint::new(rx, upstream.clone(), push()), upstream)
}

fn request(method: Method, uri: &str) -> Request<Full<Bytes>> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, "Basic c2FuZGJveDpzZWNyZXQ=")
        .header("git-protocol", "version=2")
        .header("x-other", "1")
        .body(Full::new(Bytes::new()))
        .unwrap()
}

async fn answer(
    endpoint: &GitEndpoint,
    source: &str,
    request: Request<Full<Bytes>>,
) -> (StatusCode, String) {
    let response = endpoint.handle(source.parse().unwrap(), request).await;
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn other_tasks_repository_path_is_forbidden_and_not_forwarded() {
    let (endpoint, upstream) = endpoint(Some(policy("forge")));
    let uri = "/git/DF-2.git/info/refs?service=git-upload-pack";

    let (status, body) = answer(&endpoint, POD, request(Method::GET, uri)).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, "df: this sandbox may only access DF-1\n");
    assert!(upstream.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unknown_pod_is_forbidden() {
    let (endpoint, upstream) = endpoint(Some(policy("forge")));
    let uri = "/git/DF-1.git/info/refs?service=git-upload-pack";

    let (status, body) = answer(&endpoint, "10.9.9.9", request(Method::GET, uri)).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, "df: unknown source\n");
    assert!(upstream.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn requests_before_the_first_snapshot_are_unavailable() {
    let (endpoint, upstream) = endpoint(None);
    let uri = "/git/DF-1.git/git-upload-pack";

    let (status, body) = answer(&endpoint, POD, request(Method::POST, uri)).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, "df: no policy snapshot yet\n");
    assert!(upstream.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn sandbox_without_repository_is_forbidden() {
    let (endpoint, _) = endpoint(Some(policy("forge")));
    let uri = "/git/DF-2.git/git-receive-pack";

    let (status, body) = answer(&endpoint, OTHER_POD, request(Method::POST, uri)).await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, "df: no repository for this task\n");
}

#[tokio::test]
async fn missing_forge_credential_is_a_bad_gateway() {
    let (endpoint, upstream) = endpoint(Some(policy("missing")));
    let uri = "/git/DF-1.git/git-upload-pack";

    let (status, body) = answer(&endpoint, POD, request(Method::POST, uri)).await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body, "df: forge credential unavailable\n");
    assert!(upstream.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn own_repository_is_forwarded_with_route_and_filtered_headers() {
    let (endpoint, upstream) = endpoint(Some(policy("forge")));

    let advertise = request(
        Method::GET,
        "/git/DF-1.git/info/refs?service=git-receive-pack",
    );
    let (status, _) = answer(&endpoint, POD, advertise).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = answer(
        &endpoint,
        POD,
        request(Method::POST, "/git/DF-1.git/git-upload-pack"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let calls = upstream.0.lock().unwrap();
    let [advertised, rpc] = calls.as_slice() else {
        panic!("expected two forwarded requests, got {}", calls.len());
    };
    assert_eq!(advertised.route, route("forge"));
    assert_eq!(advertised.task_key, "DF-1");
    assert_eq!(
        (advertised.service, advertised.operation),
        (Service::ReceivePack, Operation::Advertise)
    );
    assert_eq!(
        (rpc.service, rpc.operation),
        (Service::UploadPack, Operation::Rpc)
    );
    assert_eq!(advertised.headers.get("git-protocol").unwrap(), "version=2");
    assert!(advertised.headers.get(AUTHORIZATION).is_none());
    assert!(advertised.headers.get("x-other").is_none());
}

#[tokio::test]
async fn other_paths_are_not_found() {
    let (endpoint, upstream) = endpoint(Some(policy("forge")));
    let cases = [
        (Method::GET, "/"),
        (Method::GET, "/git/DF-1.git/info/refs"),
        (
            Method::GET,
            "/git/DF-1.git/info/refs?service=git-upload-archive",
        ),
        (
            Method::POST,
            "/git/DF-1.git/info/refs?service=git-upload-pack",
        ),
        (Method::GET, "/git/DF-1.git/git-upload-pack"),
        (Method::POST, "/git/DF-1.git/objects"),
        (Method::POST, "/git/df-1.git/git-upload-pack"),
        (Method::POST, "/git/DF-01.git/git-upload-pack"),
        (Method::POST, "/git/DF-1/git-upload-pack"),
        (Method::POST, "/git/../DF-1.git/git-upload-pack"),
    ];

    for (method, uri) in cases {
        let (status, _) = answer(&endpoint, POD, request(method.clone(), uri)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
    }
    assert!(upstream.0.lock().unwrap().is_empty());
}

#[test]
fn content_encoding_is_forwarded_for_fetches_but_not_for_pushes() {
    let mut headers = HeaderMap::new();
    headers.insert("content-encoding", HeaderValue::from_static("gzip"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/x"));

    let fetch = forwarded_headers(Service::UploadPack, &headers);
    let push = forwarded_headers(Service::ReceivePack, &headers);

    assert_eq!(fetch.get("content-encoding").unwrap(), "gzip");
    assert!(push.get("content-encoding").is_none());
    assert_eq!(push.get(CONTENT_TYPE).unwrap(), "application/x");
}

fn with_body<B>(method: Method, uri: &str, body: B) -> Request<B> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(body)
        .unwrap()
}

#[tokio::test]
async fn info_refs_is_forwarded_without_the_request_body() {
    let (endpoint, upstream) = endpoint(Some(policy("forge")));
    let uri = "/git/DF-1.git/info/refs?service=git-upload-pack";
    let request = with_body(Method::GET, uri, Full::new(Bytes::from_static(b"smuggled")));

    let response = endpoint.handle(POD.parse().unwrap(), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    let calls = upstream.0.lock().unwrap();
    assert_eq!(calls[0].body.as_deref(), Some(&b""[..]));
}

#[tokio::test]
async fn fetch_body_within_the_limit_is_forwarded() {
    let (endpoint, upstream) = endpoint(Some(policy("forge")));
    let endpoint = endpoint.with_fetch_max_bytes(8);
    let uri = "/git/DF-1.git/git-upload-pack";
    let request = with_body(Method::POST, uri, Full::new(Bytes::from_static(b"0000")));

    let response = endpoint.handle(POD.parse().unwrap(), request).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        upstream.0.lock().unwrap()[0].body.as_deref(),
        Some(&b"0000"[..])
    );
}

#[tokio::test]
async fn fetch_body_over_the_limit_is_payload_too_large() {
    let (endpoint, upstream) = endpoint(Some(policy("forge")));
    let endpoint = endpoint.with_fetch_max_bytes(8);
    let uri = "/git/DF-1.git/git-upload-pack";
    let request = with_body(
        Method::POST,
        uri,
        Full::new(Bytes::from_static(b"0123456789")),
    );

    let (status, body) = answer(&endpoint, POD, request).await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(body.starts_with("df:fetch-too-large"), "{body}");
    assert!(upstream.0.lock().unwrap()[0].body.is_none());
}

#[tokio::test(start_paused = true)]
async fn stalled_fetch_body_times_out() {
    let (endpoint, upstream) = endpoint(Some(policy("forge")));
    let uri = "/git/DF-1.git/git-upload-pack";
    let stalled = http_body_util::StreamBody::new(tokio_stream::pending::<
        Result<hyper::body::Frame<Bytes>, Infallible>,
    >());
    let request = with_body(Method::POST, uri, stalled);

    let response = endpoint.handle(POD.parse().unwrap(), request).await;

    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    assert!(upstream.0.lock().unwrap()[0].body.is_none());
}
