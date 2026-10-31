//! Fetching through the Git endpoint: the real `git` CLI clones from a `git http-backend` forge
//! over the gateway, which injects the forge credential.

mod common;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use common::{certificate, forge, forge_repository, git};
use df_gateway::git::mirror::Mirrors;
use df_gateway::git::push::Push;
use df_gateway::git::server::{GitEndpoint, ServerTls};
use df_gateway::git::upstream::{ForgeConnector, ForgeUpstream};
use df_gateway::policy::{Policy, Sandbox};
use df_proto::gateway::v1::{ForgeCredential, GitRoute};
use tokio::net::TcpListener;
use tokio::sync::watch;

const INJECTED: &str = "Basic Ym90OnMzY3JldA=="; // bot:s3cret
const CLIENT_SENT: &str = "Basic b3RoZXI6b3RoZXI="; // other:other

#[tokio::test(flavor = "multi_thread")]
async fn git_clone_through_the_gateway_uses_protocol_v2_and_the_injected_credential() -> Result<()>
{
    let dir = tempfile::tempdir()?;
    let root = dir.path().join("forge");
    std::fs::create_dir_all(&root)?;
    forge_repository(&root)?;
    let (forge_addr, forge_roots, seen, forge_task) = forge(root).await?;

    let (cert_pem, key_pem, _, _) = certificate()?;
    let cert_file = dir.path().join("gateway.crt");
    let key_file = dir.path().join("gateway.key");
    std::fs::write(&cert_file, &cert_pem)?;
    std::fs::write(&key_file, &key_pem)?;
    let route = GitRoute {
        upstream_url: format!("https://127.0.0.1:{}/app.git", forge_addr.port()),
        credential_ref: "forge".into(),
        ..GitRoute::default()
    };
    let sandbox = Sandbox {
        run_id: "run-1".into(),
        task_key: "DF-7".into(),
        run_number: 1,
        project_key: "DF".into(),
        persona_id: "dev".into(),
        hosts: Vec::new(),
    };
    let localhost = "127.0.0.1".parse()?;
    let policy = Policy::new(1, [(localhost, sandbox)]).with_git(
        [(localhost, route)],
        [(
            "forge".to_string(),
            ForgeCredential {
                username: "bot".into(),
                password: "s3cret".into(),
            },
        )],
    );
    let (_policy_tx, policy_rx) = watch::channel(Some(Arc::new(policy)));
    let mirrors = Mirrors::new(
        dir.path().join("mirrors"),
        ForgeConnector::with_roots(forge_roots.clone())?,
    );
    let push = Push::new(Arc::new(mirrors), &dir.path().join("mirrors"), 1 << 20);
    let endpoint = Arc::new(GitEndpoint::new(
        policy_rx,
        Arc::new(ForgeUpstream::with_roots(forge_roots)?),
        Arc::new(push),
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let gateway = tokio::spawn(endpoint.serve(
        listener,
        Arc::new(ServerTls::new(cert_file.clone(), key_file)),
        std::future::pending(),
    ));

    let clone_dir = dir.path().to_owned();
    let ca = cert_file.display().to_string();
    let cloned = tokio::time::timeout(
        Duration::from_secs(60),
        tokio::task::spawn_blocking(move || -> Result<()> {
            clone_blobless(&clone_dir, &ca, port, &[], "clone")?;
            clone_blobless(&clone_dir, &ca, port, &["--no-checkout"], "partial")
        }),
    )
    .await;
    gateway.abort();
    forge_task.abort();
    cloned???;

    assert_eq!(
        std::fs::read_to_string(dir.path().join("clone/README"))?,
        "hello\n"
    );
    assert_partial_clone(&dir.path().join("partial"))?;
    let seen = seen
        .lock()
        .map_err(|_poisoned| anyhow::anyhow!("poisoned"))?;
    assert!(!seen.is_empty());
    for headers in seen.iter() {
        assert_eq!(headers.get_all("authorization").iter().count(), 1);
        assert_eq!(headers.get("authorization").context("auth")?, INJECTED);
    }
    assert!(seen.iter().any(|headers| {
        headers
            .get("git-protocol")
            .is_some_and(|v| v == "version=2")
    }));
    Ok(())
}

/// `git clone --filter=blob:none` of DF-7 through the gateway on `port` into `dir/target`.
fn clone_blobless(
    dir: &std::path::Path,
    ca: &str,
    port: u16,
    extra: &[&str],
    target: &str,
) -> Result<()> {
    let ca_config = format!("http.sslCAInfo={ca}");
    let header = format!("http.extraHeader=Authorization: {CLIENT_SENT}");
    let url = format!("https://127.0.0.1:{port}/git/DF-7.git");
    let mut args = vec![
        "-c",
        ca_config.as_str(),
        "-c",
        "protocol.version=2",
        "-c",
        header.as_str(),
        "clone",
        "-q",
        "--filter=blob:none",
    ];
    args.extend_from_slice(extra);
    args.extend([url.as_str(), target]);
    git(dir, &args).map(drop)
}

/// The clone keeps the blob filter and, without checkout, misses at least one blob.
fn assert_partial_clone(clone: &std::path::Path) -> Result<()> {
    let filter = git(clone, &["config", "remote.origin.partialclonefilter"])?;
    assert_eq!(String::from_utf8(filter.stdout)?.trim(), "blob:none");
    let objects = git(
        clone,
        &["rev-list", "--objects", "--missing=print", "--all"],
    )?;
    assert!(
        String::from_utf8(objects.stdout)?
            .lines()
            .any(|line| line.starts_with('?')),
        "no missing blob in the partial clone"
    );
    Ok(())
}
