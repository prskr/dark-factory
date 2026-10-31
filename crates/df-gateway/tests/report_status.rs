//! `git push` against a stub smart-HTTP server that answers with the gateway's `report-status`
//! prints the rejection reason of every ref.

use std::net::SocketAddr;
use std::path::Path;
use std::process::Output;

use anyhow::{Context, Result};
use bytes::Bytes;
use df_gateway::git::protocol::{parse_receive_pack_request, report_status, side_band_requested};
use df_gateway::git::refs::{Reason, RefVerdict};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{CONTENT_TYPE, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::task::JoinHandle;

const ZERO_ID: &str = "0000000000000000000000000000000000000000";
const WITH_SIDE_BAND: &str = "report-status report-status-v2 side-band-64k delete-refs ofs-delta";
const WITHOUT_SIDE_BAND: &str = "report-status report-status-v2 delete-refs ofs-delta";

#[tokio::test]
async fn git_prints_side_band_rejection() -> Result<()> {
    let output = push_to_stub(WITH_SIDE_BAND).await?;

    assert_rejected(&output);
    Ok(())
}

#[tokio::test]
async fn git_prints_plain_rejection() -> Result<()> {
    let output = push_to_stub(WITHOUT_SIDE_BAND).await?;

    assert_rejected(&output);
    Ok(())
}

fn assert_rejected(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "push succeeded: {stderr}");
    assert!(stderr.contains("[remote rejected]"), "stderr: {stderr}");
    assert!(stderr.contains("(df:ref-not-allowed"), "stderr: {stderr}");
}

/// Commits once in a fresh repository and pushes it to a stub that rejects every ref.
async fn push_to_stub(capabilities: &'static str) -> Result<Output> {
    let (addr, server) = start_stub(capabilities).await?;
    let home = tempfile::tempdir()?;
    let repo = home.path().join("repo");

    let home = home.path();
    git(
        home,
        home,
        &["-c", "init.defaultBranch=main", "init", "-q", "repo"],
    )
    .await?;
    git(
        home,
        &repo,
        &["commit", "-q", "--allow-empty", "-m", "init"],
    )
    .await?;
    let url = format!("http://{addr}/repo.git");
    let output = git_command(home, &repo)
        .args(["push", &url, "HEAD:refs/heads/main"])
        .output()
        .await?;

    server.abort();
    Ok(output)
}

async fn git(home: &Path, dir: &Path, args: &[&str]) -> Result<()> {
    let output = git_command(home, dir).args(args).output().await?;
    anyhow::ensure!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

/// A `git` command in `dir`, isolated from the user's and the system's configuration.
fn git_command(home: &Path, dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"]);
    command
}

/// Serves a receive-pack ref advertisement with `capabilities` and rejects every pushed ref.
async fn start_stub(capabilities: &'static str) -> Result<(SocketAddr, JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let service = service_fn(move |request| handle(request, capabilities));
                // A broken connection only fails the push under test.
                http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await
                    .ok();
            });
        }
    });
    Ok((addr, server))
}

async fn handle(
    request: Request<Incoming>,
    capabilities: &'static str,
) -> Result<Response<Full<Bytes>>> {
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or_default().to_owned();
    match (request.method().clone(), path.as_str()) {
        (Method::GET, "/repo.git/info/refs") if query == "service=git-receive-pack" => {
            Ok(response(
                advertisement(capabilities),
                "application/x-git-receive-pack-advertisement",
            ))
        }
        (Method::POST, "/repo.git/git-receive-pack") => {
            let body = request.into_body().collect().await?.to_bytes();
            let push = parse_receive_pack_request(&body, u64::try_from(body.len())?)
                .context("parse push request")?;
            let verdicts: Vec<RefVerdict> = push
                .commands
                .into_iter()
                .map(|command| RefVerdict {
                    name: command.name,
                    reason: Some(Reason::RefNotAllowed),
                })
                .collect();
            let report = report_status(&verdicts, side_band_requested(&push.capabilities));
            Ok(response(report, "application/x-git-receive-pack-result"))
        }
        _ => {
            let mut not_found = Response::new(Full::new(Bytes::new()));
            *not_found.status_mut() = StatusCode::NOT_FOUND;
            Ok(not_found)
        }
    }
}

fn response(body: Vec<u8>, content_type: &'static str) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(body)));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

/// The smart-HTTP ref advertisement of an empty repository.
fn advertisement(capabilities: &str) -> Vec<u8> {
    let mut out = pkt_line("# service=git-receive-pack\n");
    out.extend_from_slice(b"0000");
    out.extend(pkt_line(&format!(
        "{ZERO_ID} capabilities^{{}}\0{capabilities}\n"
    )));
    out.extend_from_slice(b"0000");
    out
}

fn pkt_line(data: &str) -> Vec<u8> {
    format!("{:04x}{data}", data.len() + 4).into_bytes()
}
