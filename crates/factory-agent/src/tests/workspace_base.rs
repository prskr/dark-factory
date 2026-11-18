//! Prepare follows the base branch (`workspace-lifecycle` "Following the base branch"): a task
//! branch without commits of its own is moved to `origin/<base>`.

use std::path::{Path, PathBuf};

use df_proto::agent::v1::Workspace;

use super::setup_test_remote_repo;
use crate::git::Git;
use crate::workspace::prepare_workspace;

const BASE: &str = "factory/task/DF-20";
const TASK: &str = "factory/task/DF-21";

struct Scenario {
    _temp: tempfile::TempDir,
    upstream: PathBuf,
    scratch: PathBuf,
    local: PathBuf,
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
    /// An upstream with `main` and the base branch `DF-20` one commit ahead of it.
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
        let hooks = temp.path().join("hooks-empty");
        std::fs::create_dir_all(&hooks).expect("hooks dir");
        let git = Git::new(&local)
            .allow_protocols(["https", "file"])
            .with_hooks_empty_dir(&hooks);
        let scenario = Self {
            _temp: temp,
            upstream,
            scratch,
            local,
            hooks,
            git,
        };
        scenario.commit_on(BASE, "main", "one.txt");
        scenario
    }

    /// Commits a new file on `branch` of the upstream, creating it from `from` if needed, and
    /// returns the new tip.
    fn commit_on(&self, branch: &str, from: &str, file: &str) -> String {
        let dir = &self.scratch;
        git_in(dir, &["fetch", "origin"]);
        let remote = format!("origin/{branch}");
        let exists = std::process::Command::new("git")
            .args(["rev-parse", "--verify", &remote])
            .current_dir(dir)
            .output()
            .expect("git")
            .status
            .success();
        let start = if exists {
            remote
        } else {
            format!("origin/{from}")
        };
        git_in(dir, &["checkout", "-B", branch, &start]);
        std::fs::write(dir.join(file), file).expect("write");
        git_in(dir, &["add", file]);
        git_in(dir, &["commit", "-m", file]);
        git_in(dir, &["push", "origin", &format!("{branch}:{branch}")]);
        git_in(dir, &["rev-parse", "HEAD"])
    }

    fn remote_tip(&self, branch: &str) -> String {
        git_in(
            &self.upstream,
            &["rev-parse", &format!("refs/heads/{branch}")],
        )
    }

    fn workspace(&self, task: &str, base: &str) -> Workspace {
        Workspace {
            repo_url: format!("file://{}", self.upstream.display()),
            base_branch: base.to_string(),
            task_branch: task.to_string(),
            wip_branch: "factory/wip/DF-21".to_string(),
            ..Default::default()
        }
    }

    async fn prepare(&self, workspace: &Workspace) {
        prepare_workspace(&self.git, &self.local, workspace, &self.hooks, None)
            .await
            .expect("prepare");
    }

    async fn head(&self) -> String {
        self.git.current_head().await.expect("head")
    }
}

// Scenario "Reviewer sees the new round".
#[tokio::test]
async fn an_existing_workspace_follows_the_base_while_the_task_has_no_commits() {
    let s = Scenario::new();
    let workspace = s.workspace(TASK, BASE);
    s.prepare(&workspace).await;
    assert_eq!(s.head().await, s.remote_tip(BASE));

    s.commit_on(BASE, "main", "two.txt");
    let tip = s.commit_on(BASE, "main", "three.txt");
    s.prepare(&workspace).await;

    assert_eq!(s.head().await, tip);
    assert_eq!(
        git_in(&s.local, &["rev-parse", "--abbrev-ref", "HEAD"]),
        TASK
    );
}

#[tokio::test]
async fn the_remote_task_branch_follows_the_base_as_well() {
    let s = Scenario::new();
    let workspace = s.workspace(TASK, BASE);
    // The task branch exists on the remote at the old base tip.
    git_in(
        &s.scratch,
        &[
            "push",
            "origin",
            &format!("origin/{BASE}:refs/heads/{TASK}"),
        ],
    );
    s.prepare(&workspace).await;

    let tip = s.commit_on(BASE, "main", "two.txt");
    s.prepare(&workspace).await;

    assert_eq!(s.head().await, tip);
    assert_eq!(s.remote_tip(TASK), tip);
}

// Scenario "Own commits".
#[tokio::test]
async fn a_task_branch_with_own_commits_is_left_alone() {
    let s = Scenario::new();
    let workspace = s.workspace(TASK, BASE);
    s.prepare(&workspace).await;
    for (key, value) in [("user.name", "Bot"), ("user.email", "b@example.com")] {
        s.git.run_cmd(&["config", key, value]).await.unwrap();
    }
    s.git
        .run_cmd(&["config", "commit.gpgsign", "false"])
        .await
        .unwrap();
    std::fs::write(s.local.join("own.txt"), "own").unwrap();
    s.git.run_cmd(&["add", "own.txt"]).await.unwrap();
    s.git.run_cmd(&["commit", "-m", "own"]).await.unwrap();
    let own = s.head().await;

    s.commit_on(BASE, "main", "two.txt");
    s.prepare(&workspace).await;

    assert_eq!(s.head().await, own);
}

// Scenario "Fresh volume of a task without own commits".
#[tokio::test]
async fn a_fresh_clone_starts_at_the_base_when_the_remote_task_branch_has_no_own_commits() {
    let s = Scenario::new();
    git_in(
        &s.scratch,
        &[
            "push",
            "origin",
            &format!("origin/{BASE}:refs/heads/{TASK}"),
        ],
    );
    let tip = s.commit_on(BASE, "main", "two.txt");
    assert_ne!(s.remote_tip(TASK), tip);

    s.prepare(&s.workspace(TASK, BASE)).await;

    assert_eq!(s.head().await, tip);
    assert_eq!(s.remote_tip(TASK), tip);
}

#[tokio::test]
async fn a_fresh_clone_keeps_the_remote_task_branch_with_own_commits() {
    let s = Scenario::new();
    git_in(
        &s.scratch,
        &[
            "push",
            "origin",
            &format!("origin/{BASE}:refs/heads/{TASK}"),
        ],
    );
    let own = s.commit_on(TASK, BASE, "own.txt");
    s.commit_on(BASE, "main", "two.txt");

    s.prepare(&s.workspace(TASK, BASE)).await;

    assert_eq!(s.head().await, own);
    assert_eq!(s.remote_tip(TASK), own);
}

// A third-party branch that happens to be an ancestor of its base is not moved.
#[tokio::test]
async fn a_task_on_an_existing_branch_is_never_moved() {
    let s = Scenario::new();
    git_in(
        &s.scratch,
        &["push", "origin", "origin/main:refs/heads/fix/login"],
    );
    let workspace = Workspace {
        existing_branch: true,
        ..s.workspace("fix/login", "main")
    };
    s.commit_on("main", "main", "two.txt");
    let before = s.remote_tip("fix/login");

    s.prepare(&workspace).await;

    assert_eq!(s.head().await, before);
    assert_eq!(s.remote_tip("fix/login"), before);
}
