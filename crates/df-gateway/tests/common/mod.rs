//! Test forge: `git http-backend` behind TLS with a self-signed certificate.

use std::convert::Infallible;
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, bail};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::HeaderMap;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rustls::RootCertStore;
use rustls::pki_types::PrivatePkcs8KeyDer;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

pub(crate) type Seen = Arc<Mutex<Vec<HeaderMap>>>;

/// A bare repository `app.git` with one commit under `root`.
pub(crate) fn forge_repository(root: &Path) -> Result<()> {
    let work = root.join("work");
    std::fs::create_dir_all(&work)?;
    std::fs::write(work.join("README"), "hello\n")?;
    git(&work, &["init", "-q", "-b", "main"])?;
    git(&work, &["add", "README"])?;
    git(
        &work,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "-m",
            "init",
        ],
    )?;
    git(root, &["clone", "-q", "--bare", "work", "app.git"])?;
    git(
        &root.join("app.git"),
        &["config", "uploadpack.allowFilter", "true"],
    )?;
    Ok(())
}

pub(crate) fn git(dir: &Path, args: &[&str]) -> Result<std::process::Output> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() {
        bail!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}

/// A self-signed certificate for 127.0.0.1: (cert PEM, key PEM, rustls server config).
pub(crate) fn certificate() -> Result<(String, String, rustls::ServerConfig, RootCertStore)> {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()])?;
    let key = PrivatePkcs8KeyDer::from(signing_key.serialize_der());
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(vec![cert.der().clone()], key.into())?;
    let mut roots = RootCertStore::empty();
    roots.add(cert.der().clone())?;
    Ok((cert.pem(), signing_key.serialize_pem(), config, roots))
}

/// Runs one request through `git http-backend` (CGI) for the repositories under `root`.
async fn http_backend(root: PathBuf, request: Request<Incoming>) -> Result<Response<Full<Bytes>>> {
    let (parts, body) = request.into_parts();
    let body = body.collect().await?.to_bytes();
    let header = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let mut command = Command::new("git");
    command
        .arg("http-backend")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("GIT_PROJECT_ROOT", &root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("REQUEST_METHOD", parts.method.as_str())
        .env("PATH_INFO", parts.uri.path())
        .env("QUERY_STRING", parts.uri.query().unwrap_or_default())
        .env("CONTENT_TYPE", header("content-type"))
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("GIT_PROTOCOL", header("git-protocol"))
        .env("HTTP_GIT_PROTOCOL", header("git-protocol"))
        .env("HTTP_CONTENT_ENCODING", header("content-encoding"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = tokio::task::spawn_blocking(move || -> Result<_> {
        let mut child = command.spawn()?;
        child.stdin.take().context("stdin")?.write_all(&body)?;
        Ok(child.wait_with_output()?)
    })
    .await??;
    let stdout = output.stdout;
    let split = stdout
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|at| (at, 4))
        .or_else(|| {
            stdout
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|at| (at, 2))
        })
        .context("CGI output without header end")?;
    let (head, rest) = stdout.split_at(split.0);
    let mut response = Response::new(Full::new(Bytes::copy_from_slice(
        rest.get(split.1..).unwrap_or_default(),
    )));
    for line in String::from_utf8_lossy(head).lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("status") {
            let code = value.split(' ').next().unwrap_or("500").parse()?;
            *response.status_mut() = StatusCode::from_u16(code)?;
        } else {
            response.headers_mut().append(
                hyper::header::HeaderName::from_bytes(name.as_bytes())?,
                value.parse()?,
            );
        }
    }
    Ok(response)
}

/// The forge: `git http-backend` over TLS. Returns its address, its trust roots, what it saw
/// and the accept task.
pub(crate) async fn forge(
    root: PathBuf,
) -> Result<(SocketAddr, RootCertStore, Seen, JoinHandle<()>)> {
    let (_, _, config, roots) = certificate()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let seen = Seen::default();
    let recorded = seen.clone();
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let Ok(tls) = acceptor.accept(stream).await else {
                continue;
            };
            tokio::spawn(serve_connection(tls, root.clone(), recorded.clone()));
        }
    });
    Ok((addr, roots, seen, task))
}

/// Serves one forge connection: every request goes through `git http-backend` for the
/// repositories under `root`, and its headers are added to `recorded`.
pub(crate) async fn serve_connection<S>(stream: S, root: PathBuf, recorded: Seen)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |request: Request<Incoming>| {
        if let Ok(mut seen) = recorded.lock() {
            seen.push(request.headers().clone());
        }
        let root = root.clone();
        async move { Ok::<_, Infallible>(respond(root, request).await) }
    });
    // A client that hangs up mid-request ends the connection with an error; tests check results.
    http1::Builder::new()
        .serve_connection(TokioIo::new(stream), service)
        .await
        .ok();
}

/// The `git http-backend` answer to `request`, or a 500 with the error.
async fn respond(root: PathBuf, request: Request<Incoming>) -> Response<Full<Bytes>> {
    http_backend(root, request).await.unwrap_or_else(|error| {
        let mut response = Response::new(Full::new(Bytes::from(error.to_string())));
        *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        response
    })
}
