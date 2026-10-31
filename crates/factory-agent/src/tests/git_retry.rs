//! Retry of transient Git failures (`specs/workspace-lifecycle/spec.md`).
//!
//! The scenarios run real git behind a fake `git` script that answers the first invocations of
//! an operation with scripted proxy failures (503, 502, connection reset, rejection).

use std::path::{Path, PathBuf};
use std::time::Duration;

use df_proto::agent::v1::{PushStatus, Workspace};

use super::{clone_for_workspace_test, setup_test_remote_repo};
use crate::git::{GIT_RETRY_DELAYS, Git, is_transient_git_failure};
use crate::workspace::{prepare_workspace, push_after_turn};

const PROXY_URL: &str = "https://git.df.internal:8443/acme/app.git/";

const FAKE_GIT: &str = r#"#!/bin/sh
d=$(dirname "$0")
op=
for a in "$@"; do case "$a" in clone|fetch|push|ls-remote) op=$a; break;; esac; done
[ -n "$op" ] && echo "$op" >> "$d/calls"
f="$d/script.$op"
if [ -n "$op" ] && [ -s "$f" ]; then
  step=$(head -n 1 "$f"); tail -n +2 "$f" > "$f.tmp"; mv "$f.tmp" "$f"
  case "$step" in
    503) echo "fatal: unable to access '__URL__': The requested URL returned error: 503" >&2; exit 128;;
    502) echo "fatal: unable to access '__URL__': The requested URL returned error: 502" >&2; exit 128;;
    unknown-source) echo "remote: df: unknown source" >&2; echo "fatal: unable to access '__URL__': The requested URL returned error: 403" >&2; exit 128;;
    refused) echo "fatal: unable to access '__URL__': Failed to connect to git.df.internal port 8443 after 0 ms: Connection refused" >&2; exit 128;;
    rejected) echo "remote: df:ref-not-allowed" >&2; echo " ! [remote rejected] HEAD -> refs/heads/main (df:ref-not-allowed)" >&2; exit 1;;
    lost) git "$@" >/dev/null 2>&1; echo "error: RPC failed; curl 56 Recv failure: Connection reset by peer" >&2; echo "fatal: the remote end hung up unexpectedly" >&2; exit 1;;
  esac
fi
exec git "$@"
"#;

/// A fake git in `dir` whose operations follow `scripts` (operation, steps).
fn fake_git(dir: &Path, scripts: &[(&str, &[&str])]) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let program = dir.join("fake-git");
    std::fs::write(&program, FAKE_GIT.replace("__URL__", PROXY_URL)).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    for (op, steps) in scripts {
        let mut content = steps.join("\n");
        content.push('\n');
        std::fs::write(dir.join(format!("script.{op}")), content).unwrap();
    }
    program
}

/// Invocations of `op` recorded by the fake git in `dir`.
fn calls(dir: &Path, op: &str) -> usize {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .filter(|l| *l == op)
        .count()
}

fn workspace(upstream: &Path) -> Workspace {
    Workspace {
        repo_url: format!("file://{}", upstream.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    }
}

/// A clone of a fresh upstream with one new commit, and a `Git` that runs `program` without
/// waiting between retries.
struct PushSetup {
    _temp: tempfile::TempDir,
    fake_dir: PathBuf,
    upstream: PathBuf,
    git: Git,
}

fn push_setup(scripts: &[(&str, &[&str])]) -> PushSetup {
    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());
    let local = temp.path().join("local");
    clone_for_workspace_test(&upstream, &local);
    std::fs::write(local.join("work.txt"), "work\n").unwrap();
    for args in [&["add", "work.txt"][..], &["commit", "-m", "work"][..]] {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&local)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let fake_dir = temp.path().join("fake");
    std::fs::create_dir_all(&fake_dir).unwrap();
    let hooks = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&hooks).unwrap();
    let git = Git::new(&local)
        .allow_protocols(["file"])
        .with_hooks_empty_dir(&hooks)
        .with_program(fake_git(&fake_dir, scripts))
        .with_retry_delays(vec![Duration::ZERO; GIT_RETRY_DELAYS.len()]);
    PushSetup {
        _temp: temp,
        fake_dir,
        upstream,
        git,
    }
}

fn remote_sha(upstream: &Path, reference: &str) -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", reference])
        .current_dir(upstream)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn test_retry_delays_are_one_five_and_fifteen_seconds() {
    assert_eq!(
        GIT_RETRY_DELAYS,
        [
            Duration::from_secs(1),
            Duration::from_secs(5),
            Duration::from_secs(15)
        ]
    );
}

#[test]
fn test_connection_errors_and_503_are_transient() {
    for output in [
        "fatal: unable to access 'https://git.df.internal:8443/a/b.git/': The requested URL returned error: 503",
        "fatal: unable to access 'x': Failed to connect to git.df.internal port 8443 after 3 ms: Couldn't connect to server",
        "fatal: unable to access 'x': Failed to connect to git.df.internal port 8443: Connection refused",
        "error: RPC failed; curl 56 Recv failure: Connection reset by peer",
        "error: RPC failed; curl 52 Empty reply from server",
    ] {
        assert!(is_transient_git_failure(output), "{output}");
    }
}

#[test]
fn test_mid_transfer_connection_failures_are_transient() {
    for output in [
        "error: RPC failed; curl 28 Operation timed out after 300000 milliseconds",
        "error: RPC failed; curl 18 transfer closed with outstanding read data remaining",
        "error: RPC failed; curl 56 OpenSSL SSL_read: SSL_ERROR_SYSCALL, errno 0",
        "error: RPC failed; curl 56 Recv failure: Operation timed out",
        "error: RPC failed; curl 92 HTTP/2 stream 5 was not closed cleanly: INTERNAL_ERROR (err 2)",
    ] {
        assert!(is_transient_git_failure(output), "{output}");
    }
}

#[test]
fn test_403_with_unknown_source_from_the_gateway_is_transient() {
    let output = "remote: df: unknown source\nfatal: unable to access 'x': The requested URL returned error: 403";
    assert!(is_transient_git_failure(output));
}

#[test]
fn test_transient_markers_in_remote_lines_are_not_retried() {
    for output in [
        "remote: Connection refused\nfatal: repository 'x' not found",
        "remote: returned error: 503\nerror: failed to push some refs",
        "remote: df: unknown source\nfatal: repository 'x' not found",
        "remote: other policy\nremote: Connection reset by peer\nfatal: unable to access 'x': The requested URL returned error: 403",
    ] {
        assert!(!is_transient_git_failure(output), "{output}");
    }
}

#[test]
fn test_rejections_403_502_and_other_failures_are_not_transient() {
    for output in [
        "fatal: unable to access 'x': The requested URL returned error: 403",
        "fatal: unable to access 'x': The requested URL returned error: 502",
        "!\tHEAD:refs/heads/main\t[remote rejected] (df:ref-not-allowed)",
        " ! [rejected]        HEAD -> main (fetch first)",
        // a rejection wins over a connection error in the same output
        " ! [remote rejected] HEAD -> main (df:ref-not-allowed)\nConnection reset by peer",
        "fatal: Could not resolve host: git.df.internal",
        "fatal: repository 'x' not found",
        "fatal: Authentication failed for 'x'",
        "",
    ] {
        assert!(!is_transient_git_failure(output), "{output}");
    }
}

#[tokio::test]
async fn test_clone_succeeds_after_a_503_from_a_gateway_pod_without_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());
    let fake_dir = temp.path().join("fake");
    std::fs::create_dir_all(&fake_dir).unwrap();
    let hooks = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&hooks).unwrap();
    let repo_dir = temp.path().join("data").join("repo");
    let git = Git::new(&repo_dir)
        .allow_protocols(["file"])
        .with_hooks_empty_dir(&hooks)
        .with_program(fake_git(&fake_dir, &[("clone", &["503"])]))
        .with_retry_delays(vec![Duration::ZERO; 3]);

    let result = prepare_workspace(&git, &repo_dir, &workspace(&upstream), &hooks, None)
        .await
        .expect("prepare succeeds after the retry");

    assert!(!result.diverged);
    assert_eq!(calls(&fake_dir, "clone"), 2);
    assert_eq!(
        git.current_head().await.unwrap(),
        remote_sha(&upstream, "main")
    );
}

#[tokio::test]
async fn test_first_clone_succeeds_after_the_gateway_did_not_know_the_pod_yet() {
    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());
    let fake_dir = temp.path().join("fake");
    std::fs::create_dir_all(&fake_dir).unwrap();
    let hooks = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&hooks).unwrap();
    let repo_dir = temp.path().join("data").join("repo");
    let git = Git::new(&repo_dir)
        .allow_protocols(["file"])
        .with_hooks_empty_dir(&hooks)
        .with_program(fake_git(&fake_dir, &[("clone", &["unknown-source"])]))
        .with_retry_delays(vec![Duration::ZERO; 3]);

    prepare_workspace(&git, &repo_dir, &workspace(&upstream), &hooks, None)
        .await
        .expect("prepare succeeds after the retry");

    assert_eq!(calls(&fake_dir, "clone"), 2);
    assert_eq!(
        git.current_head().await.unwrap(),
        remote_sha(&upstream, "main")
    );
}

#[tokio::test]
async fn test_fetch_is_retried_after_a_connection_error() {
    let setup = push_setup(&[("fetch", &["refused"])]);

    let out = setup
        .git
        .run_network_cmd(&["fetch", "origin"])
        .await
        .unwrap();

    assert!(out.status.success());
    assert_eq!(calls(&setup.fake_dir, "fetch"), 2);
}

#[tokio::test]
async fn test_push_succeeds_once_the_forge_is_back() {
    let setup = push_setup(&[("push", &["503", "503"])]);
    let mut last_pushed = None;

    let report = push_after_turn(&setup.git, &workspace(&setup.upstream), &mut last_pushed)
        .await
        .unwrap()
        .expect("a push report");

    assert_eq!(report.status, PushStatus::Ok as i32);
    assert_eq!(calls(&setup.fake_dir, "push"), 3);
    assert_eq!(remote_sha(&setup.upstream, "factory/task/DF-1"), report.sha);
}

#[tokio::test]
async fn test_push_fails_after_the_retries_are_used_up() {
    let setup = push_setup(&[("push", &["503", "503", "503", "503"])]);
    let mut last_pushed = None;

    let report = push_after_turn(&setup.git, &workspace(&setup.upstream), &mut last_pushed)
        .await
        .unwrap()
        .expect("a push report");

    assert_eq!(report.status, PushStatus::Failed as i32);
    assert_eq!(calls(&setup.fake_dir, "push"), 4);
}

#[tokio::test]
async fn test_push_rejected_upstream_credential_is_not_retried() {
    let setup = push_setup(&[("push", &["502"])]);
    let mut last_pushed = None;

    let report = push_after_turn(&setup.git, &workspace(&setup.upstream), &mut last_pushed)
        .await
        .unwrap()
        .expect("a push report");

    assert_eq!(report.status, PushStatus::Failed as i32);
    assert_eq!(calls(&setup.fake_dir, "push"), 1);
    assert!(last_pushed.is_none());
}

#[tokio::test]
async fn test_rejected_push_is_not_retried() {
    let setup = push_setup(&[("push", &["rejected"])]);
    let mut last_pushed = None;

    let report = push_after_turn(&setup.git, &workspace(&setup.upstream), &mut last_pushed)
        .await
        .unwrap()
        .expect("a push report");

    assert_ne!(report.status, PushStatus::Ok as i32);
    assert!(report.detail.contains("df:ref-not-allowed"), "{report:?}");
    assert_eq!(calls(&setup.fake_dir, "push"), 1);
}

#[tokio::test]
async fn test_push_with_response_lost_after_the_ref_update_counts_as_ok() {
    let setup = push_setup(&[("push", &["lost"])]);
    let mut last_pushed = None;

    let report = push_after_turn(&setup.git, &workspace(&setup.upstream), &mut last_pushed)
        .await
        .unwrap()
        .expect("a push report");

    assert_eq!(report.status, PushStatus::Ok as i32);
    assert_eq!(remote_sha(&setup.upstream, "factory/task/DF-1"), report.sha);
    assert_eq!(calls(&setup.fake_dir, "push"), 1);
    assert_eq!(calls(&setup.fake_dir, "ls-remote"), 1);
    assert_eq!(last_pushed.as_deref(), Some(report.sha.as_str()));
}
