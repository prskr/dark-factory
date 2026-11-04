//! Push checks end to end: the real `git` CLI pushes through the gateway to a `git http-backend`
//! forge, and the gateway checks the pushed commits with the project's push policy and the task's
//! exceptions from its snapshot.

mod common;

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context as _, Result};
use common::{certificate, forge, forge_repository, git};
use df_gateway::git::mirror::Mirrors;
use df_gateway::git::policy_checks::PolicyChecks;
use df_gateway::git::push::{Push, PushRecord, PushRecorder, RecordError};
use df_gateway::git::secret_rules::SecretRules;
use df_gateway::git::server::{GitEndpoint, ServerTls};
use df_gateway::git::upstream::ForgeUpstream;
use df_gateway::policy::{Policy, Sandbox};
use df_gateway::upstream::UpstreamConnector;
use df_proto::gateway::v1::{
    CheckMode, ForgeCredential, GitRoute, PushCheck, PushPolicy, PushVerdict,
};
use df_telemetry::testing::{Captured, buffer};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

const LIMIT: Duration = Duration::from_secs(60);
const KEY: &str = "DF-7";
const BRANCH: &str = "refs/heads/factory/task/DF-7";
const WIP: &str = "refs/heads/factory/wip/DF-7";
const BOT: &str = "bot@factory.test";

/// A forge with `app.git` and a gateway in front of it that runs the push checks for task `DF-7`
/// of project `DF`.
struct Gateway {
    dir: tempfile::TempDir,
    ca: PathBuf,
    port: u16,
    forge_port: u16,
    snapshot: watch::Sender<Option<Arc<Policy>>>,
    records: Arc<Recorded>,
    tasks: Vec<JoinHandle<()>>,
}

impl Gateway {
    async fn start() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path().join("forge");
        std::fs::create_dir_all(&root)?;
        forge_repository(&root)?;
        git(
            &root.join("app.git"),
            &["config", "http.receivepack", "true"],
        )?;
        let (forge_addr, forge_roots, _seen, forge_task) = forge(root).await?;
        let (cert_pem, key_pem, _, _) = certificate()?;
        let ca = dir.path().join("gateway.crt");
        let key = dir.path().join("gateway.key");
        std::fs::write(&ca, cert_pem)?;
        std::fs::write(&key, key_pem)?;

        let mirrors = Mirrors::new(
            dir.path().join("mirrors"),
            UpstreamConnector::with_roots(forge_roots.clone())?,
        );
        let upstream = ForgeUpstream::with_roots(forge_roots)?;
        let (snapshot, snapshots) = watch::channel(Some(Arc::new(policy(forge_addr.port(), &[])?)));
        let records = Arc::new(Recorded::default());
        let push = Push::new(Arc::new(mirrors), &dir.path().join("mirrors"), 64 << 20)
            .with_checks(Arc::new(PolicyChecks::new(SecretRules::builtin()?)))
            .with_recorder(records.clone());
        let endpoint = Arc::new(GitEndpoint::new(
            snapshots,
            Arc::new(upstream),
            Arc::new(push),
        ));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let tls = Arc::new(ServerTls::new(ca.clone(), key));
        let gateway_task = tokio::spawn(async move {
            endpoint.serve(listener, tls, std::future::pending()).await;
        });
        Ok(Self {
            dir,
            ca,
            port,
            forge_port: forge_addr.port(),
            snapshot,
            records,
            tasks: vec![forge_task, gateway_task],
        })
    }

    /// Publishes a snapshot in which the task has the exceptions `fingerprints`.
    fn except(&self, fingerprints: &[String]) -> Result<()> {
        self.snapshot
            .send_replace(Some(Arc::new(policy(self.forge_port, fingerprints)?)));
        Ok(())
    }

    fn forge_head(&self) -> Result<Option<String>> {
        head_of(&self.dir.path().join("forge/app.git"), BRANCH)
    }

    /// A clone of the forge repository.
    fn client(&self) -> Result<PathBuf> {
        let forge = self.dir.path().join("forge/app.git");
        git(
            self.dir.path(),
            &["clone", "-q", &forge.display().to_string(), "client"],
        )?;
        Ok(self.dir.path().join("client"))
    }

    /// `git push HEAD:<task branch>` through the gateway; the output whatever the exit status.
    async fn push(&self, work: &Path) -> Result<Output> {
        self.push_to(work, BRANCH).await
    }

    /// `git push HEAD:<branch>` through the gateway; the output whatever the exit status.
    async fn push_to(&self, work: &Path, branch: &str) -> Result<Output> {
        let mut command = Command::new("git");
        command
            .current_dir(work)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .arg("-c")
            .arg(format!("http.sslCAInfo={}", self.ca.display()))
            .arg("push")
            .arg(format!("https://127.0.0.1:{}/git/{KEY}.git", self.port))
            .arg(format!("HEAD:{branch}"));
        let output =
            tokio::time::timeout(LIMIT, tokio::task::spawn_blocking(move || command.output()))
                .await
                .context("push hung")???;
        Ok(output)
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// The snapshot: task `DF-7` of project `DF` at 127.0.0.1, with the spec's default push policy
/// and the exceptions `fingerprints`.
fn policy(forge_port: u16, fingerprints: &[String]) -> Result<Policy> {
    let route = GitRoute {
        upstream_url: format!("https://127.0.0.1:{forge_port}/app.git"),
        task_branch: BRANCH.into(),
        wip_branch: WIP.into(),
        credential_ref: "forge".into(),
        commit_name: "Bot".into(),
        commit_email: BOT.into(),
        ..GitRoute::default()
    };
    let sandbox = Sandbox {
        run_id: "run-DF-7".into(),
        task_key: KEY.into(),
        run_number: 1,
        project_key: "DF".into(),
        persona_id: "dev".into(),
        hosts: Vec::new(),
    };
    let push_policy = PushPolicy {
        protected_globs: [
            ".forgejo/workflows/**",
            ".github/workflows/**",
            ".gitea/workflows/**",
            "CODEOWNERS",
        ]
        .map(str::to_owned)
        .to_vec(),
        protected_mode: CheckMode::Approval.into(),
        secret_mode: CheckMode::Approval.into(),
        size_mode: CheckMode::Approval.into(),
        max_blob_bytes: 5 << 20,
        linear_history: false,
    };
    let localhost: IpAddr = "127.0.0.1".parse()?;
    let credential = ForgeCredential {
        username: "bot".into(),
        password: "s3cret".into(),
    };
    Ok(Policy::new(1, [(localhost, sandbox)])
        .with_git([(localhost, route)], [("forge".to_owned(), credential)])
        .with_push_checks(
            [("DF".to_owned(), push_policy)],
            [(localhost, fingerprints.to_vec())],
        ))
}

/// Writes `content` to `file` and commits it with the instance's commit identity.
fn commit(work: &Path, file: &str, content: &str) -> Result<()> {
    let path = work.join(file);
    std::fs::create_dir_all(path.parent().context("file without a parent")?)?;
    std::fs::write(path, content)?;
    git(work, &["add", file])?;
    git(
        work,
        &[
            "-c",
            "user.name=Bot",
            "-c",
            &format!("user.email={BOT}"),
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

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A GitHub token, assembled at runtime so this file holds none.
fn github_token(seed: &str) -> String {
    ["ghp_", &seed.repeat(9)].concat()
}

/// Every push record.
#[derive(Default)]
struct Recorded(Mutex<Vec<PushRecord>>);

impl Recorded {
    fn all(&self) -> Vec<PushRecord> {
        self.0.lock().expect("records lock").clone()
    }

    fn last(&self) -> Result<PushRecord> {
        self.all().pop().context("no push record")
    }
}

#[async_trait::async_trait]
impl PushRecorder for Recorded {
    async fn record(&self, record: PushRecord) -> Result<(), RecordError> {
        self.0.lock().expect("records lock").push(record);
        Ok(())
    }
}

/// The log output of this test binary at every level: a global subscriber, since the checks log
/// on the blocking pool.
fn logs() -> &'static Captured {
    static LOGS: OnceLock<Captured> = OnceLock::new();
    LOGS.get_or_init(|| {
        let (captured, writer) = buffer();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(writer)
            .finish();
        // Another global subscriber would be one of an earlier test of this binary.
        tracing::subscriber::set_global_default(subscriber).ok();
        captured
    })
}

fn logged() -> String {
    logs().text()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_approved_protected_path_exception_lets_the_same_commits_through() -> Result<()> {
    let gateway = Gateway::start().await?;
    let work = gateway.client()?;
    commit(&work, ".forgejo/workflows/ci.yml", "on: push\n")?;

    let rejected = gateway.push(&work).await?;

    assert!(!rejected.status.success());
    assert!(
        stderr(&rejected).contains(
            "df:protected-path .forgejo/workflows/ci.yml (matches .forgejo/workflows/**)"
        ),
        "{}",
        stderr(&rejected)
    );
    assert_eq!(gateway.forge_head()?, None);
    let record = gateway.records.last()?;
    assert_eq!(record.verdict, PushVerdict::Rejected);
    assert!(record.approval_requested);
    let fingerprints: Vec<String> = record
        .findings
        .iter()
        .filter_map(|found| found.fingerprint.clone())
        .collect();
    assert_eq!(fingerprints, ["path:.forgejo/workflows/ci.yml"]);

    gateway.except(&fingerprints)?;
    let accepted = gateway.push(&work).await?;

    assert!(accepted.status.success(), "{}", stderr(&accepted));
    assert_eq!(gateway.forge_head()?, head_of(&work, "HEAD")?);
    let record = gateway.records.last()?;
    assert_eq!(record.verdict, PushVerdict::Accepted);
    assert!(record.findings.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_pushed_to_the_wip_branch_is_checked_again_on_the_task_branch() -> Result<()> {
    let gateway = Gateway::start().await?;
    let work = gateway.client()?;
    commit(&work, ".dark-factory/config.toml", "checks = false\n")?;

    let wip = gateway.push_to(&work, WIP).await?;

    assert!(wip.status.success(), "{}", stderr(&wip));
    let task = gateway.push(&work).await?;

    assert!(!task.status.success());
    assert!(
        stderr(&task).contains("df:protected-path .dark-factory/config.toml"),
        "{}",
        stderr(&task)
    );
    assert_eq!(gateway.forge_head()?, None);
    assert_eq!(gateway.records.last()?.verdict, PushVerdict::Rejected);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_from_another_tasks_branch_is_scanned_on_the_task_branch() -> Result<()> {
    let gateway = Gateway::start().await?;
    let work = gateway.client()?;
    let token = github_token("E5f6");
    commit(&work, "config.py", &format!("TOKEN = \"{token}\"\n"))?;
    git(
        &work,
        &["push", "-q", "origin", "HEAD:refs/heads/factory/task/DF-8"],
    )?;

    let task = gateway.push(&work).await?;

    assert!(!task.status.success());
    assert!(
        stderr(&task).contains("df:secret-scan config.py:1"),
        "{}",
        stderr(&task)
    );
    assert_eq!(gateway.forge_head()?, None);
    assert_eq!(gateway.records.last()?.verdict, PushVerdict::Rejected);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn commits_already_on_the_task_branch_are_not_checked_again() -> Result<()> {
    let gateway = Gateway::start().await?;
    let work = gateway.client()?;
    commit(&work, ".forgejo/workflows/ci.yml", "on: push\n")?;
    gateway.except(&["path:.forgejo/workflows/ci.yml".to_owned()])?;
    let approved = gateway.push(&work).await?;
    assert!(approved.status.success(), "{}", stderr(&approved));

    gateway.except(&[])?;
    commit(&work, "src/lib.rs", "// more work\n")?;
    let next = gateway.push(&work).await?;

    assert!(next.status.success(), "{}", stderr(&next));
    assert_eq!(gateway.forge_head()?, head_of(&work, "HEAD")?);
    assert!(gateway.records.last()?.findings.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn merging_the_base_branch_does_not_report_the_humans_changes() -> Result<()> {
    let gateway = Gateway::start().await?;
    let work = gateway.client()?;
    git(&work, &["checkout", "-q", "-b", "task"])?;
    commit(&work, "src/lib.rs", "// agent work\n")?;
    let first = gateway.push(&work).await?;
    assert!(first.status.success(), "{}", stderr(&first));
    git(&work, &["checkout", "-q", "main"])?;
    for (file, content) in [
        (".dark-factory/config.toml", "persona = \"dev\"\n"),
        (".forgejo/workflows/ci.yml", "on: push\n"),
    ] {
        std::fs::create_dir_all(work.join(file).parent().context("no parent")?)?;
        std::fs::write(work.join(file), content)?;
    }
    git(&work, &["add", "-A"])?;
    git(
        &work,
        &[
            "-c",
            "user.name=Peter",
            "-c",
            "user.email=peter@example.org",
            "commit",
            "-q",
            "-m",
            "human",
        ],
    )?;
    git(&work, &["push", "-q", "origin", "HEAD:main"])?;
    git(&work, &["checkout", "-q", "task"])?;
    git(
        &work,
        &[
            "-c",
            "user.name=Bot",
            "-c",
            &format!("user.email={BOT}"),
            "merge",
            "-q",
            "--no-edit",
            "main",
        ],
    )?;

    let merged = gateway.push(&work).await?;

    assert!(merged.status.success(), "{}", stderr(&merged));
    assert_eq!(gateway.forge_head()?, head_of(&work, "HEAD")?);
    let record = gateway.records.last()?;
    assert_eq!(record.verdict, PushVerdict::Accepted);
    assert!(record.findings.is_empty(), "{:?}", record.findings);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn an_exception_for_one_secret_does_not_cover_a_new_one() -> Result<()> {
    logs();
    let gateway = Gateway::start().await?;
    let work = gateway.client()?;
    let first = github_token("A1b2");
    let second = github_token("C3d4");
    commit(
        &work,
        "config.py",
        &format!("import os\n\nGITHUB_TOKEN = \"{first}\"\n"),
    )?;

    let rejected = gateway.push(&work).await?;

    assert!(!rejected.status.success());
    assert!(
        stderr(&rejected)
            .contains("df:secret-scan config.py:3 (betterleaks.github-pat ghp_********)"),
        "{}",
        stderr(&rejected)
    );
    let record = gateway.records.last()?;
    assert!(record.approval_requested);
    assert_eq!(record.findings.len(), 1);
    assert_eq!(record.findings[0].check(), PushCheck::SecretScan);
    let excepted = record.findings[0]
        .fingerprint
        .clone()
        .context("a secret finding without a fingerprint")?;

    gateway.except(std::slice::from_ref(&excepted))?;
    commit(&work, "settings.py", &format!("TOKEN = \"{second}\"\n"))?;
    let again = gateway.push(&work).await?;

    assert!(!again.status.success());
    assert!(
        stderr(&again).contains("df:secret-scan settings.py:1"),
        "{}",
        stderr(&again)
    );
    assert!(!stderr(&again).contains("config.py"), "{}", stderr(&again));
    assert_eq!(gateway.forge_head()?, None);
    let record = gateway.records.last()?;
    assert_eq!(record.verdict, PushVerdict::Rejected);
    let paths: Vec<&str> = record
        .findings
        .iter()
        .map(|found| found.path.as_str())
        .collect();
    assert_eq!(paths, ["settings.py"]);
    assert_ne!(record.findings[0].fingerprint, Some(excepted));

    let output = [stderr(&rejected), stderr(&again)].join("\n");
    let recorded = format!("{:?}", gateway.records.all());
    let logged = logged();
    assert!(logged.contains("push checks rejected a ref"), "{logged}");
    for secret in [&first[4..], &second[4..]] {
        assert!(!output.contains(secret), "{output}");
        assert!(!recorded.contains(secret), "{recorded}");
        assert!(!logged.contains(secret), "{logged}");
    }
    Ok(())
}
