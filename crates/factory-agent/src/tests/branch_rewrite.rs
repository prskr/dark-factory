//! Push with lease, rewritten branches in prepare and existing branches (`workspace-lifecycle`),
//! against a local bare remote.

use std::path::{Path, PathBuf};

use df_proto::agent::v1::{PushReport, PushStatus, RewritePolicy, Workspace};

use crate::git::{Force, Git};
use crate::workspace::{PrepareError, prepare_workspace, push_after_turn};

const BRANCH: &str = "factory/task/DF-1";

/// Runs git in `dir` and returns its trimmed stdout.
fn sh(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=T", "-c"])
        .arg("user.email=t@example.com")
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn commit(dir: &Path, file: &str) {
    std::fs::write(dir.join(file), file).expect("write");
    sh(dir, &["add", file]);
    sh(dir, &["commit", "-q", "-m", file]);
}

/// A remote with `main` and `branch`, a seed clone to act as another user, and the runtime's
/// checkout path.
struct Fixture {
    _dir: tempfile::TempDir,
    remote: PathBuf,
    seed: PathBuf,
    local: PathBuf,
    hooks: PathBuf,
    runtime: PathBuf,
    git: Git,
}

impl Fixture {
    fn new(branch: Option<&str>) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let remote = dir.path().join("remote.git");
        let seed = dir.path().join("seed");
        let local = dir.path().join("data").join("repo");
        let hooks = dir.path().join("hooks-empty");
        let runtime = dir.path().join("data").join("runtime");
        std::fs::create_dir_all(&hooks).expect("hooks");
        sh(
            dir.path(),
            &["init", "-q", "--bare", "-b", "main", "remote.git"],
        );
        sh(dir.path(), &["clone", "-q", "remote.git", "seed"]);
        commit(&seed, "main.txt");
        sh(&seed, &["push", "-q", "origin", "HEAD:main"]);
        if let Some(branch) = branch {
            sh(&seed, &["checkout", "-q", "-b", branch]);
            commit(&seed, "branch.txt");
            sh(&seed, &["push", "-q", "origin", &format!("HEAD:{branch}")]);
        }
        let git = Git::new(&local)
            .allow_protocols(["https", "file"])
            .with_hooks_empty_dir(&hooks)
            .with_runtime_dir(&runtime);
        Self {
            _dir: dir,
            remote,
            seed,
            local,
            hooks,
            runtime,
            git,
        }
    }

    fn workspace(&self, branch: &str, policy: RewritePolicy, existing: bool) -> Workspace {
        Workspace {
            repo_url: format!("file://{}", self.remote.display()),
            base_branch: "main".into(),
            task_branch: branch.into(),
            wip_branch: "factory/wip/DF-1".into(),
            rewrite_policy: policy.into(),
            existing_branch: existing,
            ..Default::default()
        }
    }

    async fn prepare(
        &self,
        workspace: &Workspace,
    ) -> anyhow::Result<crate::workspace::PrepareResult> {
        prepare_workspace(&self.git, &self.local, workspace, &self.hooks, None).await
    }

    fn remote_head(&self, branch: &str) -> String {
        sh(
            &self.remote,
            &["rev-parse", &format!("refs/heads/{branch}")],
        )
    }

    /// Another user pushes a commit to `branch`.
    fn human_pushes(&self, branch: &str) -> String {
        sh(&self.seed, &["fetch", "-q", "origin"]);
        sh(
            &self.seed,
            &["checkout", "-q", "-B", branch, &format!("origin/{branch}")],
        );
        commit(&self.seed, "human.txt");
        sh(
            &self.seed,
            &["push", "-q", "origin", &format!("HEAD:{branch}")],
        );
        sh(&self.seed, &["rev-parse", "HEAD"])
    }

    /// Rewrites the last commit of the checkout, like a rebase would.
    fn rewrite_last_commit(&self) {
        sh(&self.local, &["commit", "-q", "--amend", "-m", "rewritten"]);
    }

    async fn push_turn(&self, workspace: &Workspace) -> PushReport {
        push_after_turn(&self.git, workspace, &mut None)
            .await
            .expect("push")
            .expect("a report")
    }
}

#[tokio::test]
async fn a_lease_on_a_stale_commit_is_rejected_and_the_current_one_pushes() {
    let f = Fixture::new(Some(BRANCH));
    let ws = f.workspace(BRANCH, RewritePolicy::WithLease, false);
    f.prepare(&ws).await.expect("prepare");
    let current = f.remote_head(BRANCH);
    commit(&f.local, "a.txt");
    f.rewrite_last_commit();
    let refspec = format!("HEAD:refs/heads/{BRANCH}");

    let stale = f
        .git
        .push("origin", &refspec, Force::WithLease("0".repeat(40)))
        .await
        .expect("push");
    assert!(!stale.status.success());
    assert_eq!(f.remote_head(BRANCH), current);

    let ok = f
        .git
        .push("origin", &refspec, Force::WithLease(current))
        .await
        .expect("push");
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
}

#[tokio::test]
async fn a_rebase_is_pushed_with_lease_under_with_lease() {
    let f = Fixture::new(Some(BRANCH));
    let ws = f.workspace(BRANCH, RewritePolicy::WithLease, false);
    f.prepare(&ws).await.expect("prepare");
    commit(&f.local, "a.txt");
    assert_eq!(f.push_turn(&ws).await.status(), PushStatus::Ok);
    f.rewrite_last_commit();

    let report = f.push_turn(&ws).await;

    assert_eq!(report.status(), PushStatus::Ok);
    assert_eq!(f.remote_head(BRANCH), sh(&f.local, &["rev-parse", "HEAD"]));
}

#[tokio::test]
async fn a_rebase_is_not_forced_under_never() {
    let f = Fixture::new(Some(BRANCH));
    let ws = f.workspace(BRANCH, RewritePolicy::Never, false);
    f.prepare(&ws).await.expect("prepare");
    commit(&f.local, "a.txt");
    assert_eq!(f.push_turn(&ws).await.status(), PushStatus::Ok);
    let pushed = f.remote_head(BRANCH);
    f.rewrite_last_commit();

    let report = f.push_turn(&ws).await;

    assert_eq!(report.status(), PushStatus::RejectedNonFastForward);
    assert_eq!(f.remote_head(BRANCH), pushed);
}

#[tokio::test]
async fn a_rewrite_after_the_remote_moved_is_rejected_and_keeps_the_remote_commit() {
    let f = Fixture::new(Some(BRANCH));
    let ws = f.workspace(BRANCH, RewritePolicy::WithLease, false);
    f.prepare(&ws).await.expect("prepare");
    commit(&f.local, "a.txt");
    assert_eq!(f.push_turn(&ws).await.status(), PushStatus::Ok);
    let human = f.human_pushes(BRANCH);
    f.rewrite_last_commit();

    let report = f.push_turn(&ws).await;

    assert_eq!(report.status(), PushStatus::RejectedNonFastForward);
    assert_eq!(f.remote_head(BRANCH), human);
}

#[tokio::test]
async fn a_fetch_by_the_agent_does_not_move_the_lease() {
    let f = Fixture::new(Some(BRANCH));
    let ws = f.workspace(BRANCH, RewritePolicy::WithLease, false);
    f.prepare(&ws).await.expect("prepare");
    commit(&f.local, "a.txt");
    assert_eq!(f.push_turn(&ws).await.status(), PushStatus::Ok);
    let human = f.human_pushes(BRANCH);
    // The agent fetches itself, then rewrites its branch without the human's commit.
    sh(&f.local, &["fetch", "-q", "origin"]);
    f.rewrite_last_commit();

    let report = f.push_turn(&ws).await;

    assert_eq!(report.status(), PushStatus::RejectedNonFastForward);
    assert_eq!(f.remote_head(BRANCH), human);
}

#[tokio::test]
async fn prepare_pushes_an_unpushed_rebase_with_lease_while_the_remote_is_unchanged() {
    let f = Fixture::new(Some(BRANCH));
    let ws = f.workspace(BRANCH, RewritePolicy::WithLease, false);
    f.prepare(&ws).await.expect("prepare");
    commit(&f.local, "a.txt");
    assert_eq!(f.push_turn(&ws).await.status(), PushStatus::Ok);
    // The run was killed after the rebase, before the push.
    f.rewrite_last_commit();

    let result = f.prepare(&ws).await.expect("prepare again");

    assert!(!result.diverged);
    assert_eq!(result.push_report.map(|r| r.status()), Some(PushStatus::Ok));
    assert_eq!(f.remote_head(BRANCH), sh(&f.local, &["rev-parse", "HEAD"]));
}

#[tokio::test]
async fn prepare_reports_an_unpushed_rebase_as_diverged_under_never() {
    let f = Fixture::new(Some(BRANCH));
    let ws = f.workspace(BRANCH, RewritePolicy::Never, false);
    f.prepare(&ws).await.expect("prepare");
    commit(&f.local, "a.txt");
    assert_eq!(f.push_turn(&ws).await.status(), PushStatus::Ok);
    let pushed = f.remote_head(BRANCH);
    f.rewrite_last_commit();

    let result = f.prepare(&ws).await.expect("prepare again");

    assert!(result.diverged);
    assert!(result.push_report.is_none());
    assert_eq!(f.remote_head(BRANCH), pushed);
}

#[tokio::test]
async fn prepare_keeps_a_diverged_branch_when_the_remote_moved_under_with_lease() {
    let f = Fixture::new(Some(BRANCH));
    let ws = f.workspace(BRANCH, RewritePolicy::WithLease, false);
    f.prepare(&ws).await.expect("prepare");
    commit(&f.local, "a.txt");
    assert_eq!(f.push_turn(&ws).await.status(), PushStatus::Ok);
    let human = f.human_pushes(BRANCH);
    f.rewrite_last_commit();

    let result = f.prepare(&ws).await.expect("prepare again");

    assert!(result.diverged);
    assert_eq!(f.remote_head(BRANCH), human);
}

#[tokio::test]
async fn a_fresh_workspace_on_an_existing_branch_checks_it_out() {
    let f = Fixture::new(Some("renovate/serde-1.x"));
    let ws = f.workspace("renovate/serde-1.x", RewritePolicy::WithLease, true);

    f.prepare(&ws).await.expect("prepare");

    assert_eq!(
        sh(&f.local, &["branch", "--show-current"]),
        "renovate/serde-1.x"
    );
    assert_eq!(
        sh(&f.local, &["rev-parse", "HEAD"]),
        f.remote_head("renovate/serde-1.x")
    );
}

#[tokio::test]
async fn a_missing_existing_branch_fails_prepare_naming_it_and_pushes_nothing() {
    let f = Fixture::new(None);
    let ws = f.workspace("fix/login", RewritePolicy::WithLease, true);

    let error = f.prepare(&ws).await.expect_err("prepare must fail");

    assert_eq!(
        error.downcast_ref::<PrepareError>(),
        Some(&PrepareError::BranchMissing("fix/login".into()))
    );
    assert!(error.to_string().contains("fix/login"));
    let refs = sh(&f.remote, &["for-each-ref", "--format=%(refname)"]);
    assert_eq!(refs, "refs/heads/main");
}

#[tokio::test]
async fn a_fresh_workspace_on_a_new_task_branch_still_starts_from_the_base() {
    let f = Fixture::new(None);
    let ws = f.workspace(BRANCH, RewritePolicy::WithLease, false);

    f.prepare(&ws).await.expect("prepare");

    assert_eq!(sh(&f.local, &["branch", "--show-current"]), BRANCH);
}

#[tokio::test]
async fn the_remote_sha_is_never_written_through_a_planted_symlink() {
    let f = Fixture::new(Some(BRANCH));
    let ws = f.workspace(BRANCH, RewritePolicy::WithLease, false);
    std::fs::create_dir_all(&f.runtime).unwrap();
    let victim = f.runtime.parent().unwrap().join("victim.txt");
    std::fs::write(&victim, "untouched").unwrap();
    std::os::unix::fs::symlink(&victim, f.runtime.join("remote_sha")).unwrap();

    f.prepare(&ws).await.expect("prepare");

    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "untouched");
    let stored = std::fs::read_to_string(f.runtime.join("remote_sha")).unwrap();
    assert_eq!(stored, f.remote_head(BRANCH));
    assert!(
        !std::fs::symlink_metadata(f.runtime.join("remote_sha"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[tokio::test]
async fn after_a_diverged_prepare_a_rewrite_without_the_humans_commit_is_rejected() {
    let f = Fixture::new(Some(BRANCH));
    let ws = f.workspace(BRANCH, RewritePolicy::WithLease, false);
    f.prepare(&ws).await.expect("prepare");
    commit(&f.local, "a.txt");
    assert_eq!(f.push_turn(&ws).await.status(), PushStatus::Ok);
    let human = f.human_pushes(BRANCH);
    f.rewrite_last_commit();
    assert!(f.prepare(&ws).await.expect("prepare again").diverged);

    f.rewrite_last_commit();
    let report = f.push_turn(&ws).await;

    assert_eq!(report.status(), PushStatus::RejectedNonFastForward);
    assert_eq!(f.remote_head(BRANCH), human);
}

#[tokio::test]
async fn a_second_prepare_of_a_diverged_branch_pushes_nothing_and_reports_it_again() {
    let f = Fixture::new(Some(BRANCH));
    let ws = f.workspace(BRANCH, RewritePolicy::WithLease, false);
    f.prepare(&ws).await.expect("prepare");
    commit(&f.local, "a.txt");
    assert_eq!(f.push_turn(&ws).await.status(), PushStatus::Ok);
    let human = f.human_pushes(BRANCH);
    f.rewrite_last_commit();
    assert!(f.prepare(&ws).await.expect("prepare").diverged);

    let result = f.prepare(&ws).await.expect("prepare again");

    assert!(result.diverged);
    assert!(result.push_report.is_none());
    assert_eq!(f.remote_head(BRANCH), human);
}
