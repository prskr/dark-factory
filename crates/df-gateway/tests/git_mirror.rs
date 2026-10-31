//! The gateway's bare mirrors fetch from a `git http-backend` forge over TLS with gix.

mod common;

use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use bytes::Bytes;
use common::{certificate, forge, forge_repository, git};
use df_gateway::git::mirror::{MirrorError, Mirrors};
use df_gateway::git::server::ForgeTarget;
use df_gateway::git::upstream::ForgeConnector;
use df_proto::gateway::v1::{ForgeCredential, GitRoute};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

const FETCH_LIMIT: Duration = Duration::from_secs(60);

fn credential() -> ForgeCredential {
    ForgeCredential {
        username: "bot".into(),
        password: "s3cret".into(),
    }
}

fn route(port: u16) -> GitRoute {
    GitRoute {
        upstream_url: format!("https://127.0.0.1:{port}/app.git"),
        credential_ref: "forge".into(),
        ..GitRoute::default()
    }
}

fn head_of(repository: &Path, reference: &str) -> Result<String> {
    let output = git(repository, &["rev-parse", reference])?;
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn commit_upstream(forge_root: &Path, file: &str) -> Result<()> {
    let work = forge_root.join("work");
    std::fs::write(work.join(file), "more\n")?;
    git(&work, &["add", file])?;
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
            file,
        ],
    )?;
    git(&work, &["push", "-q", "../app.git", "main"])?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_creates_the_mirror_and_picks_up_new_upstream_commits() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("forge");
    std::fs::create_dir_all(&root)?;
    forge_repository(&root)?;
    let (addr, roots, seen, task) = forge(root.clone()).await?;
    let mirrors = Mirrors::new(
        dir.path().join("mirrors"),
        ForgeConnector::with_roots(roots)?,
    );
    let route = route(addr.port());

    let mirror = tokio::time::timeout(
        FETCH_LIMIT,
        mirrors.fetch(ForgeTarget::new(&route, &credential())),
    )
    .await??;
    assert_eq!(
        head_of(mirror.path(), "refs/heads/main")?,
        head_of(&root.join("work"), "main")?
    );
    assert_eq!(
        mirror.path().parent(),
        Some(dir.path().join("mirrors").as_path())
    );

    commit_upstream(&root, "NEXT")?;
    let again = tokio::time::timeout(
        FETCH_LIMIT,
        mirrors.fetch(ForgeTarget::new(&route, &credential())),
    )
    .await??;
    task.abort();
    assert_eq!(again.path(), mirror.path());
    assert_eq!(
        head_of(again.path(), "refs/heads/main")?,
        head_of(&root.join("work"), "main")?
    );
    let seen = seen
        .lock()
        .map_err(|_poisoned| anyhow::anyhow!("poisoned"))?;
    assert!(seen.iter().all(|headers| {
        headers
            .get("authorization")
            .is_some_and(|v| v == "Basic Ym90OnMzY3JldA==")
    }));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_fetches_of_one_mirror_both_succeed() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("forge");
    std::fs::create_dir_all(&root)?;
    forge_repository(&root)?;
    let (addr, roots, _seen, task) = forge(root.clone()).await?;
    let mirrors = Mirrors::new(
        dir.path().join("mirrors"),
        ForgeConnector::with_roots(roots)?,
    );
    let route = route(addr.port());
    let credential = credential();

    let (first, second) = tokio::time::timeout(FETCH_LIMIT, async {
        tokio::join!(
            mirrors.fetch(ForgeTarget::new(&route, &credential)),
            mirrors.fetch(ForgeTarget::new(&route, &credential))
        )
    })
    .await?;
    task.abort();
    let (first, second) = (first?, second?);
    assert_eq!(first.path(), second.path());
    assert_eq!(
        head_of(first.path(), "refs/heads/main")?,
        head_of(&root.join("work"), "main")?
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_credential_fails_the_fetch_without_prompting() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let (_, _, config, roots) = certificate()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let Ok(tls) = acceptor.accept(stream).await else {
                continue;
            };
            let service = service_fn(|_request: Request<Incoming>| async {
                let mut response = Response::new(Full::new(Bytes::new()));
                *response.status_mut() = StatusCode::UNAUTHORIZED;
                Ok::<_, Infallible>(response)
            });
            tokio::spawn(http1::Builder::new().serve_connection(TokioIo::new(tls), service));
        }
    });
    let mirrors = Mirrors::new(
        dir.path().join("mirrors"),
        ForgeConnector::with_roots(roots)?,
    );

    let result = tokio::time::timeout(
        FETCH_LIMIT,
        mirrors.fetch(ForgeTarget::new(&route(port), &credential())),
    )
    .await
    .context("fetch hung")?;
    task.abort();
    let error = result.err().context("fetch succeeded")?;
    assert!(matches!(error, MirrorError::Rejected(_)), "{error}");
    assert!(!error.to_string().contains("s3cret"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_fetch_keeps_the_mirror_locked_until_it_ends() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let (_, _, config, roots) = certificate()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let (started_tx, mut started) = tokio::sync::mpsc::unbounded_channel::<()>();
    let (release, released) = tokio::sync::watch::channel(false);
    let log = Arc::new(std::sync::Mutex::new(Vec::<&str>::new()));
    let server_log = log.clone();
    // Each request is logged, then held until the test releases the forge.
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let Ok(tls) = acceptor.accept(stream).await else {
                continue;
            };
            let (started_tx, released, log) =
                (started_tx.clone(), released.clone(), server_log.clone());
            let service = service_fn(move |_request: Request<Incoming>| {
                held_answer(started_tx.clone(), released.clone(), log.clone())
            });
            tokio::spawn(http1::Builder::new().serve_connection(TokioIo::new(tls), service));
        }
    });
    let mirrors = Arc::new(Mirrors::new(
        dir.path().join("mirrors"),
        ForgeConnector::with_roots(roots)?,
    ));
    let fetch = |mirrors: Arc<Mirrors>| {
        tokio::spawn(async move {
            mirrors
                .fetch(ForgeTarget::new(&route(port), &credential()))
                .await
        })
    };

    let first = fetch(mirrors.clone());
    tokio::time::timeout(FETCH_LIMIT, started.recv()).await?;
    first.abort();
    let second = fetch(mirrors.clone());
    // A second fetch that ignored the lock would reach the forge right away.
    let early = tokio::time::timeout(Duration::from_millis(300), started.recv()).await;
    assert!(
        early.is_err(),
        "second fetch ran while the first one was blocked"
    );
    release.send_replace(true);
    let result = tokio::time::timeout(FETCH_LIMIT, second).await??;
    task.abort();

    assert!(matches!(result, Err(MirrorError::Rejected(_))));
    assert_eq!(
        *log.lock().expect("log"),
        vec!["start", "end", "start", "end"]
    );
    Ok(())
}

#[tokio::test]
async fn fetch_refuses_a_url_with_userinfo_or_without_https() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mirrors = Mirrors::new(dir.path(), ForgeConnector::new()?);
    for url in ["http://forge/app.git", "https://u:p@forge/app.git"] {
        let route = GitRoute {
            upstream_url: url.into(),
            ..GitRoute::default()
        };
        let result = mirrors.fetch(ForgeTarget::new(&route, &credential())).await;
        assert!(matches!(result, Err(MirrorError::InvalidUrl)), "{url}");
    }
    Ok(())
}

/// Logs the request, holds it until `released` turns true, then answers `401`.
async fn held_answer(
    started_tx: tokio::sync::mpsc::UnboundedSender<()>,
    mut released: tokio::sync::watch::Receiver<bool>,
    log: Arc<std::sync::Mutex<Vec<&'static str>>>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    log.lock().expect("log").push("start");
    started_tx.send(()).ok();
    released.wait_for(|released| *released).await.ok();
    log.lock().expect("log").push("end");
    let mut response = Response::new(Full::new(Bytes::new()));
    *response.status_mut() = StatusCode::UNAUTHORIZED;
    Ok(response)
}
