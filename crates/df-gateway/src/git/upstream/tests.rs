use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Mutex;

use anyhow::Result;
use http_body_util::Full;
use hyper::header::{HeaderMap, WWW_AUTHENTICATE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use super::*;
use crate::git::server::{ForgeTarget, Service};
use df_proto::gateway::v1::GitRoute;

/// What the fake forge saw: path and query, and headers.
type Seen = Arc<Mutex<Vec<(String, HeaderMap)>>>;

/// A TLS forge answering every request with `status` and `extra` headers; returns its address,
/// an upstream trusting it, and what it saw. The task ends with the test runtime.
async fn forge(
    status: StatusCode,
    extra: &[(&str, &str)],
) -> Result<(SocketAddr, ForgeUpstream, Seen)> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()])?;
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der());
    let server = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], key.into())?;
    let mut roots = RootCertStore::empty();
    roots.add(cert.der().clone())?;
    let upstream = ForgeUpstream::with_roots(roots)?;

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let acceptor = TlsAcceptor::from(Arc::new(server));
    let seen = Seen::default();
    let recorded = seen.clone();
    let extra: Vec<(String, String)> = extra
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let Ok(tls) = acceptor.accept(stream).await else {
                continue;
            };
            let recorded = recorded.clone();
            let extra = extra.clone();
            let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                let path = request.uri().path_and_query().map(ToString::to_string);
                recorded
                    .lock()
                    .unwrap()
                    .push((path.unwrap_or_default(), request.headers().clone()));
                let mut response = Response::new(Full::new(Bytes::from_static(b"payload")));
                *response.status_mut() = status;
                for (name, value) in &extra {
                    response.headers_mut().insert(
                        hyper::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                        HeaderValue::from_str(value).unwrap(),
                    );
                }
                async move { Ok::<_, Infallible>(response) }
            });
            tokio::spawn(http1::Builder::new().serve_connection(TokioIo::new(tls), service));
        }
    });
    Ok((addr, upstream, seen))
}

fn route(url: &str) -> GitRoute {
    GitRoute {
        upstream_url: url.into(),
        credential_ref: "forge".into(),
        ..GitRoute::default()
    }
}

fn credential() -> ForgeCredential {
    ForgeCredential {
        username: "bot".into(),
        password: "s3cret".into(),
    }
}

fn request(operation: Operation) -> GitRequest {
    let mut headers = HeaderMap::new();
    headers.insert("git-protocol", HeaderValue::from_static("version=2"));
    let method = match operation {
        Operation::Advertise => Method::GET,
        Operation::Rpc => Method::POST,
    };
    GitRequest {
        task_key: "DF-7".parse().unwrap(),
        service: Service::UploadPack,
        operation,
        method,
        headers,
        body: Full::new(Bytes::new())
            .map_err(|never| match never {})
            .boxed_unsync(),
    }
}

async fn read(response: Response<Body>) -> (StatusCode, HeaderMap, String) {
    let status = response.status();
    let (parts, body) = response.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    (
        status,
        parts.headers,
        String::from_utf8(body.to_vec()).unwrap(),
    )
}

#[tokio::test]
async fn forwards_with_the_injected_credential_and_relays_the_answer() -> Result<()> {
    let (addr, upstream, seen) = forge(
        StatusCode::OK,
        &[
            (
                "content-type",
                "application/x-git-upload-pack-advertisement",
            ),
            ("connection", "keep-alive"),
        ],
    )
    .await?;
    let route = route(&format!("https://127.0.0.1:{}/acme/app.git/", addr.port()));

    let (status, headers, body) = read(
        upstream
            .forward(
                ForgeTarget::new(&route, &credential()),
                request(Operation::Advertise),
            )
            .await,
    )
    .await;
    let (rpc_status, _, _) = read(
        upstream
            .forward(
                ForgeTarget::new(&route, &credential()),
                request(Operation::Rpc),
            )
            .await,
    )
    .await;

    assert_eq!((status, rpc_status), (StatusCode::OK, StatusCode::OK));
    assert_eq!(body, "payload");
    assert_eq!(
        headers.get("content-type").unwrap(),
        "application/x-git-upload-pack-advertisement"
    );
    assert!(headers.get("connection").is_none());
    let seen = seen.lock().unwrap();
    let [(advertise, headers), (rpc, _)] = seen.as_slice() else {
        panic!("expected two requests, got {}", seen.len());
    };
    assert_eq!(advertise, "/acme/app.git/info/refs?service=git-upload-pack");
    assert_eq!(rpc, "/acme/app.git/git-upload-pack");
    // base64("bot:s3cret")
    assert_eq!(
        headers.get(AUTHORIZATION).unwrap(),
        "Basic Ym90OnMzY3JldA=="
    );
    assert_eq!(headers.get("git-protocol").unwrap(), "version=2");
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn idle_deadline_of_a_limited_body_starts_at_the_first_poll() {
    let stalled =
        http_body_util::StreamBody::new(tokio_stream::pending::<Result<Frame<Bytes>, BoxError>>())
            .boxed_unsync();
    let mut body = LimitedBody::new(stalled, 1024, Duration::from_secs(10));
    let mut cx = Context::from_waker(std::task::Waker::noop());

    tokio::time::advance(Duration::from_secs(60)).await;
    let first = hyper::body::Body::poll_frame(Pin::new(&mut body), &mut cx);
    tokio::time::advance(Duration::from_secs(11)).await;
    let second = hyper::body::Body::poll_frame(Pin::new(&mut body), &mut cx);

    assert!(first.is_pending());
    assert!(matches!(second, Poll::Ready(Some(Err(_)))));
    assert_eq!(body.limit().get(), Some(&BodyLimit::TimedOut));
}

#[test]
fn forge_url_accepts_plain_http_only_when_allowed() {
    let url = ForgeUrl::parse("http://forge.example/acme/app.git", true).unwrap();
    assert!(!url.is_https());
    assert_eq!(url.port(), 80);
    assert!(ForgeUrl::parse("http://forge.example/acme/app.git", false).is_none());
    let url = ForgeUrl::parse("https://forge.example/acme/app.git", true).unwrap();
    assert!(url.is_https());
    assert_eq!(url.port(), 443);
    assert!(ForgeUrl::parse("ssh://forge.example/acme/app.git", true).is_none());
}

#[test]
fn forge_url_accepts_only_https_without_userinfo_query_or_fragment() {
    assert!(ForgeUrl::parse("https://forge.example/acme/app.git", false).is_some());
    for url in [
        "http://forge.example/acme/app.git",
        "https://bot:pw@forge.example/acme/app.git",
        "https://forge.example/acme/app.git?x=1",
        "https://forge.example/acme/app.git#frag",
        "/acme/app.git",
    ] {
        assert!(ForgeUrl::parse(url, false).is_none(), "{url}");
    }
}

#[tokio::test]
async fn closed_forge_port_is_service_unavailable() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let upstream = ForgeUpstream::with_roots(RootCertStore::empty())?;

    let (status, _, body) = read(
        upstream
            .forward(
                ForgeTarget::new(
                    &route(&format!("https://127.0.0.1:{port}/a.git")),
                    &credential(),
                ),
                request(Operation::Advertise),
            )
            .await,
    )
    .await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, "df: forge unreachable\n");
    Ok(())
}

#[tokio::test]
async fn rejected_credential_is_a_bad_gateway_without_challenge() -> Result<()> {
    let (addr, upstream, _) = forge(
        StatusCode::UNAUTHORIZED,
        &[("www-authenticate", "Basic realm=\"x\"")],
    )
    .await?;

    let (status, headers, body) = read(
        upstream
            .forward(
                ForgeTarget::new(
                    &route(&format!("https://127.0.0.1:{}/a.git", addr.port())),
                    &credential(),
                ),
                request(Operation::Rpc),
            )
            .await,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body, "df: the forge rejected the gateway's credential\n");
    assert!(headers.get(WWW_AUTHENTICATE).is_none());
    Ok(())
}

#[tokio::test]
async fn other_statuses_are_relayed() -> Result<()> {
    let (addr, upstream, _) = forge(StatusCode::NOT_FOUND, &[]).await?;

    let (status, _, _) = read(
        upstream
            .forward(
                ForgeTarget::new(
                    &route(&format!("https://127.0.0.1:{}/a.git", addr.port())),
                    &credential(),
                ),
                request(Operation::Rpc),
            )
            .await,
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    Ok(())
}

#[tokio::test]
async fn non_https_upstream_urls_are_refused() -> Result<()> {
    let upstream = ForgeUpstream::with_roots(RootCertStore::empty())?;
    for url in [
        "http://127.0.0.1:1/a.git",
        "https://user@forge/a.git",
        "nonsense",
    ] {
        let (status, _, _) = read(
            upstream
                .forward(
                    ForgeTarget::new(&route(url), &credential()),
                    request(Operation::Rpc),
                )
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{url}");
    }
    Ok(())
}

/// A push body whose four frames come 10 s apart, driven like hyper drives a request body.
fn slow_upload() -> watch::Receiver<bool> {
    let frames = futures::stream::unfold(0, |sent| async move {
        if sent == 4 {
            return None;
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
        let frame: Result<Frame<Bytes>, BoxError> = Ok(Frame::data(Bytes::from_static(b"pack")));
        Some((frame, sent + 1))
    });
    let (upload, progress) = Upload::new(http_body_util::StreamBody::new(frames).boxed_unsync());
    tokio::spawn(upload.collect());
    progress
}

#[tokio::test(start_paused = true)]
async fn an_upload_longer_than_the_head_timeout_is_not_a_timeout() {
    let progress = slow_upload();
    let response = async {
        tokio::time::sleep(Duration::from_secs(45)).await;
        "head"
    };
    assert_eq!(response_head(response, progress).await, Some("head"));
}

#[tokio::test(start_paused = true)]
async fn no_response_head_after_the_upload_is_a_timeout() {
    let start = Instant::now();
    let head = response_head(std::future::pending::<()>(), slow_upload()).await;
    assert_eq!(head, None);
    assert_eq!(
        start.elapsed(),
        Duration::from_secs(40) + RESPONSE_HEAD_TIMEOUT
    );
}
