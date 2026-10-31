//! Pushing through the Git endpoint: the real `git` CLI pushes to a `git http-backend` forge over
//! the gateway, which checks the ref rules against a quarantined pack before forwarding.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use bytes::Bytes;
use common::{Seen, certificate, forge, forge_repository, git, serve_connection};
use df_gateway::git::mirror::Mirrors;
use df_gateway::git::push::{NoRecorder, Push, PushRecord, PushRecorder, RecordError};
use df_gateway::git::server::{
    Body, ForgeTarget, GitEndpoint, GitRequest, Operation, ServerTls, Upstream,
};
use df_gateway::git::upstream::{ForgeConnector, ForgeUpstream};
use df_gateway::policy::{Policy, Sandbox};
use df_proto::gateway::v1::{ForgeCredential, GitRoute, PushVerdict};
use hyper::body::Incoming;
use rustls::RootCertStore;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

const LIMIT: Duration = Duration::from_secs(60);
const DEFAULT_MAX: u64 = 64 << 20;

/// A forge with `app.git`, plus the gateway's CA file.
struct Fixture {
    dir: tempfile::TempDir,
    forge_port: u16,
    forge_roots: RootCertStore,
    forge_task: JoinHandle<()>,
    ca: PathBuf,
    key: PathBuf,
    gateway_roots: RootCertStore,
    /// Idle timeout of the push body, the default if `None`.
    push_idle: Option<Duration>,
    /// Scheme of the route's upstream URL.
    scheme: &'static str,
}

impl Fixture {
    async fn new() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().join("forge");
        std::fs::create_dir_all(&root)?;
        forge_repository(&root)?;
        git(
            &root.join("app.git"),
            &["config", "http.receivepack", "true"],
        )?;
        let (addr, forge_roots, _seen, forge_task) = forge(root).await?;
        let (cert_pem, key_pem, _, gateway_roots) = certificate()?;
        let ca = dir.path().join("gateway.crt");
        let key = dir.path().join("gateway.key");
        std::fs::write(&ca, cert_pem)?;
        std::fs::write(&key, key_pem)?;
        Ok(Self {
            dir,
            forge_port: addr.port(),
            forge_roots,
            forge_task,
            ca,
            key,
            gateway_roots,
            push_idle: None,
            scheme: "https",
        })
    }

    /// Like [`Self::new`], but the forge serves plain HTTP.
    async fn with_plain_http_forge() -> Result<Self> {
        let mut fixture = Self::new().await?;
        fixture.forge_task.abort();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        fixture.forge_port = listener.local_addr()?.port();
        let root = fixture.dir.path().join("forge");
        fixture.forge_task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve_connection(stream, root.clone(), Seen::default()));
            }
        });
        fixture.scheme = "http";
        Ok(fixture)
    }

    fn forge_repo(&self) -> PathBuf {
        self.dir.path().join("forge/app.git")
    }

    fn mirrors(&self) -> Result<Arc<Mirrors>> {
        Ok(Arc::new(Mirrors::new(
            self.dir.path().join("mirrors"),
            ForgeConnector::with_roots(self.forge_roots.clone())?,
        )))
    }

    /// A gateway that maps 127.0.0.1 to the task `key`; returns its port.
    async fn gateway(
        &self,
        key: &str,
        mirrors: Arc<Mirrors>,
        max: u64,
    ) -> Result<(u16, JoinHandle<()>)> {
        let upstream = Arc::new(ForgeUpstream::with_roots(self.forge_roots.clone())?);
        self.gateway_with(key, mirrors, max, upstream, Arc::new(NoRecorder))
            .await
    }

    /// Like [`Self::gateway`] with the given upstream and push recorder.
    async fn gateway_with(
        &self,
        key: &str,
        mirrors: Arc<Mirrors>,
        max: u64,
        upstream: Arc<dyn Upstream>,
        recorder: Arc<dyn PushRecorder>,
    ) -> Result<(u16, JoinHandle<()>)> {
        self.gateway_at(key, mirrors, max, upstream, recorder, self.forge_port)
            .await
    }

    /// Like [`Self::gateway_with`] with the route's upstream URL on `forge_port`.
    async fn gateway_at(
        &self,
        key: &str,
        mirrors: Arc<Mirrors>,
        max: u64,
        upstream: Arc<dyn Upstream>,
        recorder: Arc<dyn PushRecorder>,
        forge_port: u16,
    ) -> Result<(u16, JoinHandle<()>)> {
        let route = GitRoute {
            upstream_url: format!("{}://127.0.0.1:{forge_port}/app.git", self.scheme),
            task_branch: format!("refs/heads/factory/task/{key}"),
            wip_branch: format!("refs/heads/factory/wip/{key}"),
            credential_ref: "forge".into(),
            ..GitRoute::default()
        };
        let sandbox = Sandbox {
            run_id: format!("run-{key}"),
            task_key: key.into(),
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
        let (policy_tx, policy_rx) = watch::channel(Some(Arc::new(policy)));
        let mut push =
            Push::new(mirrors, &self.dir.path().join("mirrors"), max).with_recorder(recorder);
        if let Some(idle) = self.push_idle {
            push = push.with_idle_timeout(idle);
        }
        let endpoint = Arc::new(GitEndpoint::new(policy_rx, upstream, Arc::new(push)));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let tls = Arc::new(ServerTls::new(self.ca.clone(), self.key.clone()));
        let task = tokio::spawn(async move {
            let _policy_tx = policy_tx;
            endpoint.serve(listener, tls, std::future::pending()).await;
        });
        Ok((port, task))
    }

    /// A local clone of the forge repository named `name`.
    fn client(&self, name: &str) -> Result<PathBuf> {
        let path = self.dir.path().join(name);
        git(
            self.dir.path(),
            &[
                "clone",
                "-q",
                &self.forge_repo().display().to_string(),
                name,
            ],
        )?;
        Ok(path)
    }
}

fn commit(work: &Path, file: &str) -> Result<()> {
    std::fs::write(work.join(file), format!("{file}\n"))?;
    git(work, &["add", file])?;
    git(
        work,
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
    Ok(())
}

fn head_of(repository: &Path, reference: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "-q", reference])
        .current_dir(repository)
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8(output.stdout)?.trim().to_owned()))
}

/// `git push` through the gateway at `port`; returns the output whatever the exit status.
async fn push(work: &Path, ca: &Path, port: u16, key: &str, refspec: &[&str]) -> Result<Output> {
    let mut command = Command::new("git");
    command
        .current_dir(work)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .arg("-c")
        .arg(format!("http.sslCAInfo={}", ca.display()))
        .arg("push")
        .arg(format!("https://127.0.0.1:{port}/git/{key}.git"))
        .args(refspec);
    let output = tokio::time::timeout(LIMIT, tokio::task::spawn_blocking(move || command.output()))
        .await
        .context("push hung")???;
    Ok(output)
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn succeeded(output: &Output) -> Result<()> {
    if !output.status.success() {
        bail!("push failed: {}", stderr(output));
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn fast_forward_pushes_to_the_task_branch_reach_the_forge() -> Result<()> {
    let fixture = Fixture::new().await?;
    let (port, gateway) = fixture
        .gateway("DF-7", fixture.mirrors()?, DEFAULT_MAX)
        .await?;
    let work = fixture.client("client")?;
    let branch = "refs/heads/factory/task/DF-7";

    commit(&work, "ONE")?;
    succeeded(
        &push(
            &work,
            &fixture.ca,
            port,
            "DF-7",
            &[&format!("HEAD:{branch}")],
        )
        .await?,
    )?;
    assert_eq!(
        head_of(&fixture.forge_repo(), branch)?,
        head_of(&work, "HEAD")?
    );

    commit(&work, "TWO")?;
    succeeded(
        &push(
            &work,
            &fixture.ca,
            port,
            "DF-7",
            &[&format!("HEAD:{branch}")],
        )
        .await?,
    )?;
    gateway.abort();
    fixture.forge_task.abort();
    assert_eq!(
        head_of(&fixture.forge_repo(), branch)?,
        head_of(&work, "HEAD")?
    );
    Ok(())
}

/// A gateway for `DF-7` whose forge connections may use plain HTTP if `allow_plain_http`.
async fn plain_http_gateway(
    fixture: &Fixture,
    allow_plain_http: bool,
) -> Result<(u16, JoinHandle<()>)> {
    let tls =
        ForgeConnector::with_roots(RootCertStore::empty())?.allowing_plain_http(allow_plain_http);
    let mirrors = Arc::new(Mirrors::new(
        fixture.dir.path().join("mirrors"),
        tls.clone(),
    ));
    let upstream = Arc::new(ForgeUpstream::from_connector(tls));
    fixture
        .gateway_with("DF-7", mirrors, DEFAULT_MAX, upstream, Arc::new(NoRecorder))
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn pushes_reach_a_plain_http_forge_when_plain_http_is_allowed() -> Result<()> {
    let fixture = Fixture::with_plain_http_forge().await?;
    let (port, gateway) = plain_http_gateway(&fixture, true).await?;
    let work = fixture.client("client")?;
    let branch = "refs/heads/factory/task/DF-7";

    commit(&work, "ONE")?;
    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &[&format!("HEAD:{branch}")],
    )
    .await?;
    gateway.abort();
    fixture.forge_task.abort();
    succeeded(&output)?;
    assert_eq!(
        head_of(&fixture.forge_repo(), branch)?,
        head_of(&work, "HEAD")?
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plain_http_forge_is_refused_unless_plain_http_is_allowed() -> Result<()> {
    let fixture = Fixture::with_plain_http_forge().await?;
    let (port, gateway) = plain_http_gateway(&fixture, false).await?;
    let work = fixture.client("client")?;
    let branch = "refs/heads/factory/task/DF-7";

    commit(&work, "ONE")?;
    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &[&format!("HEAD:{branch}")],
    )
    .await?;
    let receive_pack =
        post_receive_pack(&fixture, port, Bytes::from(create_task_branch_commands())).await?;
    gateway.abort();
    fixture.forge_task.abort();
    assert_eq!(receive_pack, hyper::StatusCode::BAD_GATEWAY);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("df: invalid forge URL"),
        "{}",
        stderr(&output)
    );
    assert_eq!(head_of(&fixture.forge_repo(), branch)?, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forced_non_fast_forward_push_is_rejected() -> Result<()> {
    let fixture = Fixture::new().await?;
    let (port, gateway) = fixture
        .gateway("DF-7", fixture.mirrors()?, DEFAULT_MAX)
        .await?;
    let work = fixture.client("client")?;
    let branch = "refs/heads/factory/task/DF-7";
    commit(&work, "ONE")?;
    succeeded(
        &push(
            &work,
            &fixture.ca,
            port,
            "DF-7",
            &[&format!("HEAD:{branch}")],
        )
        .await?,
    )?;
    let pushed = head_of(&fixture.forge_repo(), branch)?;

    git(&work, &["reset", "-q", "--hard", "HEAD~1"])?;
    commit(&work, "OTHER")?;
    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &["--force", &format!("HEAD:{branch}")],
    )
    .await?;
    gateway.abort();
    fixture.forge_task.abort();

    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("[remote rejected]"), "{stderr}");
    assert!(stderr.contains("df:non-fast-forward"), "{stderr}");
    assert_eq!(head_of(&fixture.forge_repo(), branch)?, pushed);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_to_main_is_rejected() -> Result<()> {
    let fixture = Fixture::new().await?;
    let recorder = Recorded::new(&fixture, "refs/heads/main", false);
    let upstream = Arc::new(ForgeUpstream::with_roots(fixture.forge_roots.clone())?);
    let (port, gateway) = fixture
        .gateway_with(
            "DF-7",
            fixture.mirrors()?,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
        )
        .await?;
    let work = fixture.client("client")?;
    let before = head_of(&fixture.forge_repo(), "refs/heads/main")?;
    commit(&work, "ONE")?;

    let output = push(&work, &fixture.ca, port, "DF-7", &["HEAD:refs/heads/main"]).await?;
    gateway.abort();
    fixture.forge_task.abort();

    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("[remote rejected]"), "{stderr}");
    assert!(stderr.contains("df:ref-not-allowed"), "{stderr}");
    assert_eq!(head_of(&fixture.forge_repo(), "refs/heads/main")?, before);
    assert_eq!(
        recorder.reasons(),
        vec![(
            PushVerdict::Rejected,
            vec![(
                "refs/heads/main".to_owned(),
                "df:ref-not-allowed".to_owned()
            )]
        )]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_with_push_options_is_rejected_and_not_forwarded() -> Result<()> {
    let fixture = Fixture::new().await?;
    git(
        &fixture.forge_repo(),
        &["config", "receive.advertisePushOptions", "true"],
    )?;
    let branch = "refs/heads/factory/task/DF-7";
    let recorder = Recorded::new(&fixture, branch, false);
    let upstream = Arc::new(ForgeUpstream::with_roots(fixture.forge_roots.clone())?);
    let (port, gateway) = fixture
        .gateway_with(
            "DF-7",
            fixture.mirrors()?,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
        )
        .await?;
    let work = fixture.client("client")?;
    commit(&work, "ONE")?;

    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &["-o", "repo.private=true", &format!("HEAD:{branch}")],
    )
    .await?;
    gateway.abort();
    fixture.forge_task.abort();

    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("df:push-options-not-allowed"), "{stderr}");
    assert_eq!(head_of(&fixture.forge_repo(), branch)?, None);
    assert_eq!(
        recorder.reasons(),
        vec![(
            PushVerdict::Rejected,
            vec![(branch.to_owned(), "df:push-options-not-allowed".to_owned())]
        )]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_oversized_push_is_rejected_and_nothing_reaches_the_forge() -> Result<()> {
    let fixture = Fixture::new().await?;
    let branch = "refs/heads/factory/task/DF-7";
    let recorder = Recorded::new(&fixture, branch, false);
    let upstream = Arc::new(ForgeUpstream::with_roots(fixture.forge_roots.clone())?);
    let (port, gateway) = fixture
        .gateway_with("DF-7", fixture.mirrors()?, 64, upstream, recorder.clone())
        .await?;
    let work = fixture.client("client")?;
    commit(&work, "ONE")?;

    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &[&format!("HEAD:{branch}")],
    )
    .await?;
    gateway.abort();
    fixture.forge_task.abort();

    assert!(!output.status.success());
    assert!(stderr(&output).contains("413"), "{}", stderr(&output));
    assert_eq!(head_of(&fixture.forge_repo(), branch)?, None);
    assert_eq!(
        recorder.reasons(),
        vec![(
            PushVerdict::Rejected,
            vec![(branch.to_owned(), "df:push-too-large".to_owned())]
        )]
    );
    assert!(recorder.bytes().iter().all(|bytes| *bytes > 64));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_pushes_of_two_tasks_to_one_repository_both_succeed() -> Result<()> {
    let fixture = Fixture::new().await?;
    let mirrors = fixture.mirrors()?;
    let (port_7, gateway_7) = fixture
        .gateway("DF-7", mirrors.clone(), DEFAULT_MAX)
        .await?;
    let (port_8, gateway_8) = fixture.gateway("DF-8", mirrors, DEFAULT_MAX).await?;
    let work_7 = fixture.client("client-7")?;
    let work_8 = fixture.client("client-8")?;
    commit(&work_7, "SEVEN")?;
    commit(&work_8, "EIGHT")?;

    let (seven, eight) = tokio::join!(
        push(
            &work_7,
            &fixture.ca,
            port_7,
            "DF-7",
            &["HEAD:refs/heads/factory/task/DF-7"]
        ),
        push(
            &work_8,
            &fixture.ca,
            port_8,
            "DF-8",
            &["HEAD:refs/heads/factory/task/DF-8"]
        ),
    );
    gateway_7.abort();
    gateway_8.abort();
    fixture.forge_task.abort();
    succeeded(&seven?)?;
    succeeded(&eight?)?;
    assert_eq!(
        head_of(&fixture.forge_repo(), "refs/heads/factory/task/DF-7")?,
        head_of(&work_7, "HEAD")?
    );
    assert_eq!(
        head_of(&fixture.forge_repo(), "refs/heads/factory/task/DF-8")?,
        head_of(&work_8, "HEAD")?
    );
    Ok(())
}

/// Keeps every push record together with the forge's task branch at the time it was recorded.
struct Recorded {
    forge_repo: PathBuf,
    branch: String,
    /// Fails `FORWARDING` records, as an unreachable control plane does.
    unavailable: bool,
    records: std::sync::Mutex<Vec<(PushVerdict, Option<String>)>>,
    full: std::sync::Mutex<Vec<PushRecord>>,
}

impl Recorded {
    fn new(fixture: &Fixture, branch: &str, unavailable: bool) -> Arc<Self> {
        Arc::new(Self {
            forge_repo: fixture.forge_repo(),
            branch: branch.into(),
            unavailable,
            records: std::sync::Mutex::default(),
            full: std::sync::Mutex::default(),
        })
    }

    fn verdicts(&self) -> Vec<PushVerdict> {
        let records = self.records.lock().expect("records lock");
        records.iter().map(|(verdict, _)| *verdict).collect()
    }

    /// Verdict and `(ref, reason)` pairs of every stored record.
    fn reasons(&self) -> Vec<(PushVerdict, Vec<(String, String)>)> {
        let full = self.full.lock().expect("records lock");
        full.iter()
            .map(|record| {
                let refs = record
                    .refs
                    .iter()
                    .map(|update| (update.name.clone(), update.reason.clone()))
                    .collect();
                (record.verdict, refs)
            })
            .collect()
    }

    fn bytes(&self) -> Vec<u64> {
        let full = self.full.lock().expect("records lock");
        full.iter().map(|record| record.bytes).collect()
    }
}

#[async_trait::async_trait]
impl PushRecorder for Recorded {
    async fn record(&self, record: PushRecord) -> Result<(), RecordError> {
        if self.unavailable && record.verdict == PushVerdict::Forwarding {
            return Err(RecordError("control plane unreachable".into()));
        }
        let forge_head = head_of(&self.forge_repo, &self.branch)
            .map_err(|error| RecordError(error.to_string()))?;
        let mut records = self.records.lock().expect("records lock");
        records.push((record.verdict, forge_head));
        self.full.lock().expect("records lock").push(record);
        Ok(())
    }
}

/// Forwards advertisements to the forge but sends pushes elsewhere.
struct PushesFail {
    forge: ForgeUpstream,
    /// Where pushes go instead: a closed port, or `None` for a canned answer.
    closed_port: Option<u16>,
    answer: hyper::StatusCode,
}

#[async_trait::async_trait]
impl Upstream for PushesFail {
    async fn forward(&self, forge: ForgeTarget<'_>, request: GitRequest) -> hyper::Response<Body> {
        if request.operation == Operation::Advertise {
            return self.forge.forward(forge, request).await;
        }
        if let Some(port) = self.closed_port {
            let route = GitRoute {
                upstream_url: format!("https://127.0.0.1:{port}/app.git"),
                ..forge.route.clone()
            };
            return self
                .forge
                .forward(ForgeTarget::new(&route, forge.credential), request)
                .await;
        }
        let body = http_body_util::Full::new(Bytes::from_static(b"df: rejected\n"));
        let mut response = hyper::Response::new(http_body_util::BodyExt::boxed_unsync(
            http_body_util::BodyExt::map_err(body, |never| match never {}),
        ));
        *response.status_mut() = self.answer;
        response
    }
}

async fn closed_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    Ok(listener.local_addr()?.port())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_push_is_recorded_as_rejected_without_refs() -> Result<()> {
    let fixture = Fixture::new().await?;
    let recorder = Recorded::new(&fixture, "refs/heads/factory/task/DF-7", false);
    let upstream = Arc::new(ForgeUpstream::with_roots(fixture.forge_roots.clone())?);
    let (port, gateway) = fixture
        .gateway_with(
            "DF-7",
            fixture.mirrors()?,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
        )
        .await?;
    let status = post_receive_pack(&fixture, port, Bytes::from_static(b"not a pkt-line")).await?;
    gateway.abort();
    fixture.forge_task.abort();

    assert_eq!(status, hyper::StatusCode::BAD_REQUEST);
    let records = recorder.full.lock().expect("records lock").clone();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].verdict, PushVerdict::Rejected);
    assert!(records[0].refs.is_empty());
    Ok(())
}

const TASK_BRANCH: &str = "refs/heads/factory/task/DF-7";

/// The command section of a push creating [`TASK_BRANCH`] at an arbitrary object, with flush.
fn create_task_branch_commands() -> Vec<u8> {
    let line = format!(
        "{} {} {TASK_BRANCH}\0report-status\n",
        "0".repeat(40),
        "1".repeat(40)
    );
    let mut body = format!("{:04x}{line}", line.len() + 4).into_bytes();
    body.extend_from_slice(b"0000");
    body
}

/// A TLS connection to the gateway at `port`.
async fn connect(
    fixture: &Fixture,
    port: u16,
) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>> {
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(fixture.gateway_roots.clone())
    .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    let name = rustls::pki_types::ServerName::try_from("127.0.0.1")?;
    Ok(connector.connect(name, tcp).await?)
}

/// Posts `body` as a `git-receive-pack` request of `DF-7`; returns the response status.
async fn post_receive_pack(fixture: &Fixture, port: u16, body: Bytes) -> Result<hyper::StatusCode> {
    let tls = connect(fixture, port).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(tls)).await?;
    let connection = tokio::spawn(connection);
    let request = hyper::Request::post("/git/DF-7.git/git-receive-pack")
        .header(hyper::header::HOST, "127.0.0.1")
        .header(
            hyper::header::CONTENT_TYPE,
            "application/x-git-receive-pack-request",
        )
        .body(http_body_util::Full::new(body))?;
    let response = tokio::time::timeout(LIMIT, sender.send_request(request))
        .await
        .context("request hung")??;
    connection.abort();
    Ok(response.status())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_with_an_invalid_pack_is_recorded_as_rejected() -> Result<()> {
    let fixture = Fixture::new().await?;
    let recorder = Recorded::new(&fixture, TASK_BRANCH, false);
    let upstream = Arc::new(ForgeUpstream::with_roots(fixture.forge_roots.clone())?);
    let (port, gateway) = fixture
        .gateway_with(
            "DF-7",
            fixture.mirrors()?,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
        )
        .await?;
    let mut body = create_task_branch_commands();
    body.extend_from_slice(b"PACK\0\0\0\x02\0\0\0\x01this is not an object");

    let status = post_receive_pack(&fixture, port, Bytes::from(body)).await?;
    gateway.abort();
    fixture.forge_task.abort();

    assert_eq!(status, hyper::StatusCode::BAD_REQUEST);
    assert_eq!(
        recorder.reasons(),
        vec![(
            PushVerdict::Rejected,
            vec![(TASK_BRANCH.to_owned(), "df:invalid-pack".to_owned())]
        )]
    );
    assert_eq!(head_of(&fixture.forge_repo(), TASK_BRANCH)?, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_body_that_stalls_is_recorded_as_rejected_with_its_refs() -> Result<()> {
    let mut fixture = Fixture::new().await?;
    fixture.push_idle = Some(Duration::from_millis(200));
    let recorder = Recorded::new(&fixture, TASK_BRANCH, false);
    let upstream = Arc::new(ForgeUpstream::with_roots(fixture.forge_roots.clone())?);
    let (port, gateway) = fixture
        .gateway_with(
            "DF-7",
            fixture.mirrors()?,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
        )
        .await?;
    let mut tls = connect(&fixture, port).await?;
    let head = "POST /git/DF-7.git/git-receive-pack HTTP/1.1\r\nHost: 127.0.0.1\r\n\
                Content-Type: application/x-git-receive-pack-request\r\n\
                Content-Length: 100000\r\n\r\n";
    tls.write_all(head.as_bytes()).await?;
    tls.write_all(&create_task_branch_commands()).await?;
    tls.write_all(b"PACK").await?;
    tls.flush().await?;

    // The rest of the body never comes; the gateway answers once its idle timeout passes.
    let mut response = Vec::new();
    while !response.windows(2).any(|pair| pair == b"\r\n") {
        let mut chunk = [0_u8; 256];
        let read = tokio::time::timeout(LIMIT, tls.read(&mut chunk))
            .await
            .context("no response")??;
        if read == 0 {
            break;
        }
        response.extend_from_slice(&chunk[..read]);
    }
    gateway.abort();
    fixture.forge_task.abort();

    let response = String::from_utf8_lossy(&response);
    assert!(response.starts_with("HTTP/1.1 408"), "{response}");
    assert_eq!(
        recorder.reasons(),
        vec![(
            PushVerdict::Rejected,
            vec![(TASK_BRANCH.to_owned(), "df:request-timeout".to_owned())]
        )]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_accepted_push_is_recorded_as_forwarding_before_the_forge_sees_it() -> Result<()> {
    let fixture = Fixture::new().await?;
    let branch = "refs/heads/factory/task/DF-7";
    let recorder = Recorded::new(&fixture, branch, false);
    let upstream = Arc::new(ForgeUpstream::with_roots(fixture.forge_roots.clone())?);
    let (port, gateway) = fixture
        .gateway_with(
            "DF-7",
            fixture.mirrors()?,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
        )
        .await?;
    let work = fixture.client("client")?;
    commit(&work, "ONE")?;

    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &[&format!("HEAD:{branch}")],
    )
    .await?;
    gateway.abort();
    fixture.forge_task.abort();

    succeeded(&output)?;
    let pushed = head_of(&work, "HEAD")?;
    let records = recorder.records.lock().expect("records lock").clone();
    assert_eq!(
        records,
        vec![
            (PushVerdict::Forwarding, None),
            (PushVerdict::Accepted, pushed),
        ]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_the_forge_refuses_is_recorded_as_upstream_failed() -> Result<()> {
    let fixture = Fixture::new().await?;
    let hook = fixture.forge_repo().join("hooks/pre-receive");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho 'protected by the forge' >&2\nexit 1\n",
    )?;
    std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    let branch = "refs/heads/factory/task/DF-7";
    let recorder = Recorded::new(&fixture, branch, false);
    let upstream = Arc::new(ForgeUpstream::with_roots(fixture.forge_roots.clone())?);
    let (port, gateway) = fixture
        .gateway_with(
            "DF-7",
            fixture.mirrors()?,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
        )
        .await?;
    let work = fixture.client("client")?;
    commit(&work, "ONE")?;

    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &[&format!("HEAD:{branch}")],
    )
    .await?;
    gateway.abort();
    fixture.forge_task.abort();

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("protected by the forge"),
        "{}",
        stderr(&output)
    );
    assert_eq!(
        recorder.reasons(),
        vec![
            (
                PushVerdict::Forwarding,
                vec![(branch.to_owned(), String::new())]
            ),
            (
                PushVerdict::UpstreamFailed,
                vec![(branch.to_owned(), "df:forge-refused".to_owned())]
            ),
        ]
    );
    assert_eq!(head_of(&fixture.forge_repo(), branch)?, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_is_rejected_when_its_record_cannot_be_stored() -> Result<()> {
    let fixture = Fixture::new().await?;
    let branch = "refs/heads/factory/task/DF-7";
    let recorder = Recorded::new(&fixture, branch, true);
    let upstream = Arc::new(ForgeUpstream::with_roots(fixture.forge_roots.clone())?);
    let (port, gateway) = fixture
        .gateway_with(
            "DF-7",
            fixture.mirrors()?,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
        )
        .await?;
    let work = fixture.client("client")?;
    commit(&work, "ONE")?;

    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &[&format!("HEAD:{branch}")],
    )
    .await?;
    gateway.abort();
    fixture.forge_task.abort();

    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("df:audit-unavailable"), "{stderr}");
    assert_eq!(head_of(&fixture.forge_repo(), branch)?, None);
    assert!(recorder.verdicts().is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_to_a_forge_that_is_down_fails_with_503_and_is_recorded() -> Result<()> {
    let fixture = Fixture::new().await?;
    let branch = "refs/heads/factory/task/DF-7";
    let recorder = Recorded::new(&fixture, branch, false);
    let upstream = Arc::new(PushesFail {
        forge: ForgeUpstream::with_roots(fixture.forge_roots.clone())?,
        closed_port: Some(closed_port().await?),
        answer: hyper::StatusCode::OK,
    });
    let (port, gateway) = fixture
        .gateway_with(
            "DF-7",
            fixture.mirrors()?,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
        )
        .await?;
    let work = fixture.client("client")?;
    commit(&work, "ONE")?;

    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &[&format!("HEAD:{branch}")],
    )
    .await?;
    gateway.abort();
    fixture.forge_task.abort();

    assert!(!output.status.success());
    assert!(stderr(&output).contains("503"), "{}", stderr(&output));
    assert_eq!(
        recorder.verdicts(),
        vec![PushVerdict::Forwarding, PushVerdict::UpstreamFailed]
    );
    Ok(())
}

/// Forwards every request to the forge on `port`, whatever the route's upstream URL.
struct ForgeAt {
    forge: ForgeUpstream,
    port: u16,
}

#[async_trait::async_trait]
impl Upstream for ForgeAt {
    async fn forward(&self, forge: ForgeTarget<'_>, request: GitRequest) -> hyper::Response<Body> {
        let route = GitRoute {
            upstream_url: format!("https://127.0.0.1:{}/app.git", self.port),
            ..forge.route.clone()
        };
        self.forge
            .forward(ForgeTarget::new(&route, forge.credential), request)
            .await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_fails_with_503_and_is_recorded_when_the_mirror_fetch_cannot_reach_the_forge()
-> Result<()> {
    let fixture = Fixture::new().await?;
    let branch = "refs/heads/factory/task/DF-7";
    let recorder = Recorded::new(&fixture, branch, false);
    let upstream = Arc::new(ForgeAt {
        forge: ForgeUpstream::with_roots(fixture.forge_roots.clone())?,
        port: fixture.forge_port,
    });
    let (port, gateway) = fixture
        .gateway_at(
            "DF-7",
            fixture.mirrors()?,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
            closed_port().await?,
        )
        .await?;
    let work = fixture.client("client")?;
    commit(&work, "ONE")?;

    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &[&format!("HEAD:{branch}")],
    )
    .await?;
    gateway.abort();
    fixture.forge_task.abort();

    assert!(!output.status.success());
    assert!(stderr(&output).contains("503"), "{}", stderr(&output));
    assert_eq!(head_of(&fixture.forge_repo(), branch)?, None);
    let reasons = recorder.reasons();
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert_eq!(reasons[0].0, PushVerdict::UpstreamFailed);
    assert_eq!(
        reasons[0]
            .1
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec![branch]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_with_a_rejected_forge_credential_fails_with_502_and_is_recorded() -> Result<()> {
    let fixture = Fixture::new().await?;
    let branch = "refs/heads/factory/task/DF-7";
    let recorder = Recorded::new(&fixture, branch, false);
    // What `ForgeUpstream` answers to a forge 401/403 (tested in its unit tests).
    let upstream = Arc::new(PushesFail {
        forge: ForgeUpstream::with_roots(fixture.forge_roots.clone())?,
        closed_port: None,
        answer: hyper::StatusCode::BAD_GATEWAY,
    });
    let (port, gateway) = fixture
        .gateway_with(
            "DF-7",
            fixture.mirrors()?,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
        )
        .await?;
    let work = fixture.client("client")?;
    commit(&work, "ONE")?;

    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &[&format!("HEAD:{branch}")],
    )
    .await?;
    gateway.abort();
    fixture.forge_task.abort();

    assert!(!output.status.success());
    assert!(stderr(&output).contains("502"), "{}", stderr(&output));
    assert_eq!(
        recorder.verdicts(),
        vec![PushVerdict::Forwarding, PushVerdict::UpstreamFailed]
    );
    Ok(())
}

/// A TLS forge that answers every request with `status` and a `WWW-Authenticate` challenge;
/// returns its port and the roots that trust it.
async fn status_forge(status: hyper::StatusCode) -> Result<(u16, RootCertStore, JoinHandle<()>)> {
    let (_, _, config, roots) = certificate()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let Ok(tls) = acceptor.accept(stream).await else {
                continue;
            };
            let service =
                hyper::service::service_fn(move |_: hyper::Request<Incoming>| async move {
                    let mut response = hyper::Response::new(http_body_util::Full::new(
                        Bytes::from_static(b"no\n"),
                    ));
                    *response.status_mut() = status;
                    response.headers_mut().insert(
                        hyper::header::WWW_AUTHENTICATE,
                        hyper::header::HeaderValue::from_static("Basic realm=\"forge\""),
                    );
                    Ok::<_, std::convert::Infallible>(response)
                });
            tokio::spawn(
                hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(tls), service),
            );
        }
    });
    Ok((port, roots, task))
}

/// Pushes to [`TASK_BRANCH`] through a gateway whose pre-push mirror fetch reaches a forge
/// answering `status`; advertisements and pushes go to the real forge.
async fn push_with_a_mirror_forge_answering(
    status: hyper::StatusCode,
) -> Result<(Fixture, Output, Arc<Recorded>)> {
    let fixture = Fixture::new().await?;
    let recorder = Recorded::new(&fixture, TASK_BRANCH, false);
    let (forge_port, roots, forge_task) = status_forge(status).await?;
    let mirrors = Arc::new(Mirrors::new(
        fixture.dir.path().join("mirrors"),
        ForgeConnector::with_roots(roots)?,
    ));
    let upstream = Arc::new(ForgeAt {
        forge: ForgeUpstream::with_roots(fixture.forge_roots.clone())?,
        port: fixture.forge_port,
    });
    let (port, gateway) = fixture
        .gateway_at(
            "DF-7",
            mirrors,
            DEFAULT_MAX,
            upstream,
            recorder.clone(),
            forge_port,
        )
        .await?;
    let work = fixture.client("client")?;
    commit(&work, "ONE")?;

    let output = push(
        &work,
        &fixture.ca,
        port,
        "DF-7",
        &[&format!("HEAD:{TASK_BRANCH}")],
    )
    .await?;
    gateway.abort();
    forge_task.abort();
    fixture.forge_task.abort();
    Ok((fixture, output, recorder))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_fails_with_502_and_is_recorded_when_the_forge_rejects_the_mirror_fetch_credential()
-> Result<()> {
    let (fixture, output, recorder) =
        push_with_a_mirror_forge_answering(hyper::StatusCode::UNAUTHORIZED).await?;

    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("502"), "{stderr}");
    // A relayed challenge would make git ask for a username.
    assert!(!stderr.contains("Username"), "{stderr}");
    assert_eq!(head_of(&fixture.forge_repo(), TASK_BRANCH)?, None);
    assert_eq!(
        recorder.reasons(),
        vec![(
            PushVerdict::UpstreamFailed,
            vec![(TASK_BRANCH.to_owned(), "df:forge-unavailable".to_owned())]
        )]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_fails_with_502_and_is_recorded_when_the_forge_answers_the_mirror_fetch_with_an_error()
-> Result<()> {
    let (fixture, output, recorder) =
        push_with_a_mirror_forge_answering(hyper::StatusCode::INTERNAL_SERVER_ERROR).await?;

    assert!(!output.status.success());
    let stderr = stderr(&output);
    assert!(stderr.contains("502"), "{stderr}");
    assert_eq!(head_of(&fixture.forge_repo(), TASK_BRANCH)?, None);
    assert_eq!(
        recorder.reasons(),
        vec![(
            PushVerdict::UpstreamFailed,
            vec![(TASK_BRANCH.to_owned(), "df:forge-unavailable".to_owned())]
        )]
    );
    Ok(())
}
