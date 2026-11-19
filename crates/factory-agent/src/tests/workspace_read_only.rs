//! The read-only workspace of a chat run (`workspace-lifecycle` "Read-only workspace"): a
//! detached checkout of `origin/<base>`, reset at every prepare, and no push of any kind.

use std::path::{Path, PathBuf};

use df_proto::agent::v1::{RunOutcome, Workspace};

use super::setup_test_remote_repo;
use crate::git::Git;
use crate::workspace::{FinalizeParams, finalize_workspace, prepare_workspace, push_after_turn};

struct Scenario {
    _temp: tempfile::TempDir,
    upstream: PathBuf,
    scratch: PathBuf,
    local: PathBuf,
    runtime: PathBuf,
    hooks: PathBuf,
    git: Git,
}

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

impl Scenario {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let upstream = setup_test_remote_repo(temp.path());
        let scratch = temp.path().join("scratch");
        git_in(
            temp.path(),
            &[
                "clone",
                upstream.to_str().unwrap(),
                scratch.to_str().unwrap(),
            ],
        );
        for (key, value) in [
            ("user.name", "Bot"),
            ("user.email", "bot@example.com"),
            ("commit.gpgsign", "false"),
        ] {
            git_in(&scratch, &["config", key, value]);
        }
        let local = temp.path().join("data").join("repo");
        let runtime = temp.path().join("runtime");
        let hooks = temp.path().join("hooks-empty");
        for dir in [&runtime, &hooks] {
            std::fs::create_dir_all(dir).expect("dir");
        }
        let git = Git::new(&local)
            .allow_protocols(["https", "file"])
            .with_hooks_empty_dir(&hooks);
        Self {
            _temp: temp,
            upstream,
            scratch,
            local,
            runtime,
            hooks,
            git,
        }
    }

    /// Commits `file` on `main` of the upstream and returns the new tip.
    fn commit_on_main(&self, file: &str) -> String {
        git_in(&self.scratch, &["pull", "-q", "origin", "main"]);
        std::fs::write(self.scratch.join(file), file).expect("write");
        git_in(&self.scratch, &["add", file]);
        git_in(&self.scratch, &["commit", "-m", file]);
        git_in(&self.scratch, &["push", "-q", "origin", "HEAD:main"]);
        git_in(&self.scratch, &["rev-parse", "HEAD"])
    }

    fn workspace() -> Workspace {
        Workspace {
            base_branch: "main".into(),
            read_only: true,
            ..Default::default()
        }
    }

    fn workspace_of(&self) -> Workspace {
        Workspace {
            repo_url: format!("file://{}", self.upstream.display()),
            ..Self::workspace()
        }
    }

    async fn prepare(&self) {
        prepare_workspace(
            &self.git,
            &self.local,
            &self.workspace_of(),
            &self.hooks,
            None,
        )
        .await
        .expect("prepare");
    }

    async fn head(&self) -> String {
        self.git.current_head().await.expect("head")
    }

    fn remote_branches(&self) -> String {
        git_in(
            &self.upstream,
            &["for-each-ref", "--format=%(refname)", "refs/heads"],
        )
    }
}

// Scenario "Prepare a chat workspace".
#[tokio::test]
async fn a_chat_workspace_is_a_detached_checkout_of_the_base_and_loses_edits_of_the_last_run() {
    let s = Scenario::new();
    s.prepare().await;
    let base = s.commit_on_main("one.txt");
    s.prepare().await;
    assert_eq!(s.head().await, base);
    assert_eq!(
        git_in(&s.local, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "HEAD",
        "detached"
    );

    std::fs::write(s.local.join("README.md"), "edited").expect("edit");
    std::fs::write(s.local.join("scratch.txt"), "untracked").expect("untracked file");
    std::fs::create_dir_all(s.local.join("notes")).expect("dir");
    std::fs::write(s.local.join("notes/plan.md"), "untracked").expect("untracked file");
    s.prepare().await;

    assert_eq!(
        std::fs::read_to_string(s.local.join("README.md")).expect("read"),
        "# Test Workspace\n"
    );
    assert!(!s.local.join("scratch.txt").exists());
    assert!(!s.local.join("notes").exists());
    assert!(git_in(&s.local, &["status", "--porcelain"]).is_empty());
}

// Scenario "Base branch moved between runs" of `chats`.
#[tokio::test]
async fn a_chat_workspace_starts_at_the_new_base_without_the_edit_of_the_earlier_run() {
    let s = Scenario::new();
    s.prepare().await;
    std::fs::write(s.local.join("README.md"), "edited").expect("edit");

    let moved = s.commit_on_main("two.txt");
    s.prepare().await;

    assert_eq!(s.head().await, moved);
    assert!(s.local.join("two.txt").exists());
    assert_eq!(
        std::fs::read_to_string(s.local.join("README.md")).expect("read"),
        "# Test Workspace\n"
    );
}

// Scenario "Commit in a chat".
#[tokio::test]
async fn a_chat_that_commits_pushes_nothing_per_turn_or_at_the_end() {
    let s = Scenario::new();
    s.prepare().await;
    for (key, value) in [
        ("user.name", "Bot"),
        ("user.email", "b@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        s.git
            .run_cmd(&["config", key, value])
            .await
            .expect("config");
    }
    std::fs::write(s.local.join("own.txt"), "own").expect("write");
    s.git.run_cmd(&["add", "own.txt"]).await.expect("add");
    s.git
        .run_cmd(&["commit", "-m", "own"])
        .await
        .expect("commit");
    let branches_before = s.remote_branches();
    let workspace = s.workspace_of();

    let mut last_pushed = None;
    let per_turn = push_after_turn(&s.git, &workspace, &mut last_pushed)
        .await
        .expect("per turn");
    std::fs::write(s.local.join("loose.txt"), "uncommitted").expect("write");
    let finalized = finalize_workspace(
        &s.git,
        &FinalizeParams {
            repo_dir: &s.local,
            runtime_dir: &s.runtime,
            workspace: &workspace,
            run_id: "run-1",
            outcome: RunOutcome::EndTurn,
            detail: "",
            harness_session_id: "session",
            hooks_path: &s.hooks,
            ssl_ca_info: None,
            wip_size_cap_bytes: 10 * 1024 * 1024,
            diverged: false,
        },
        &mut last_pushed,
    )
    .await
    .expect("finalize");

    assert!(per_turn.is_none());
    assert!(finalized.push_reports.is_empty());
    let state = finalized.run_report.workspace.expect("workspace state");
    assert_eq!(
        (state.task_branch_sha.as_str(), state.wip_sha.as_str()),
        ("", "")
    );
    assert!(!state.task_branch_pushed);
    assert_eq!(s.remote_branches(), branches_before);
    assert_eq!(s.remote_branches(), "refs/heads/main");
}
