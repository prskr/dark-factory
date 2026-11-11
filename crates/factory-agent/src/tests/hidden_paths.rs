//! Hidden paths (iter3-hidden-paths): sparse patterns in `.git/config` handling, clone and reuse
//! of the workspace, removal of newly hidden files and the WIP snapshot.

use std::path::{Path, PathBuf};

use df_proto::agent::v1::{PushStatus, PushTarget, RunOutcome, Workspace};

use super::setup_test_remote_repo;
use crate::git::{Git, GitConfigParams, render_git_config, rewrite_git_config};
use crate::workspace::{FinalizeParams, finalize_workspace, prepare_workspace};

/// Runs `git args` in `dir` and returns its trimmed stdout.
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
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A fixture: an upstream with `README.md`, `talos/cp.yaml`, `talos/sub/x.yaml`,
/// `configs/top.txt`, `configs/nested/visible.txt` and a `.gitignore` for `*.local` on `main`.
struct Fixture {
    temp: tempfile::TempDir,
    upstream: PathBuf,
    hooks: PathBuf,
    local: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let upstream = setup_test_remote_repo(temp.path());
        git_in(&upstream, &["config", "uploadpack.allowFilter", "true"]);
        git_in(
            &upstream,
            &["config", "uploadpack.allowAnySHA1InWant", "true"],
        );
        let seed = temp.path().join("init_repo");
        for (path, content) in [
            ("talos/cp.yaml", "cert: A\n"),
            ("talos/sub/x.yaml", "cert: B\n"),
            ("configs/top.txt", "top\n"),
            ("configs/nested/visible.txt", "visible\n"),
            (".gitignore", "*.local\n"),
        ] {
            let file = seed.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, content).unwrap();
        }
        git_in(&seed, &["add", "."]);
        git_in(&seed, &["commit", "--no-gpg-sign", "-m", "content"]);
        git_in(&seed, &["push", "origin", "main"]);
        let hooks = temp.path().join("hooks-empty");
        std::fs::create_dir_all(&hooks).unwrap();
        let local = temp.path().join("data").join("repo");
        Self {
            temp,
            upstream,
            hooks,
            local,
        }
    }

    fn seed(&self) -> PathBuf {
        self.temp.path().join("init_repo")
    }

    fn git(&self) -> Git {
        Git::new(&self.local)
            .allow_protocols(["file"])
            .with_hooks_empty_dir(&self.hooks)
    }

    fn workspace(&self, hidden: &[&str]) -> Workspace {
        Workspace {
            repo_url: format!("file://{}", self.upstream.display()),
            base_branch: "main".to_owned(),
            task_branch: "factory/task/DF-1".to_owned(),
            wip_branch: "factory/wip/DF-1".to_owned(),
            hidden_paths: hidden.iter().map(|glob| (*glob).to_owned()).collect(),
            ..Workspace::default()
        }
    }

    async fn prepare(&self, hidden: &[&str]) -> crate::workspace::PrepareResult {
        prepare_workspace(
            &self.git(),
            &self.local,
            &self.workspace(hidden),
            &self.hooks,
            None,
        )
        .await
        .expect("prepare")
    }

    fn exists(&self, path: &str) -> bool {
        std::fs::symlink_metadata(self.local.join(path)).is_ok()
    }

    fn local_git(&self, args: &[&str]) -> String {
        git_in(&self.local, args)
    }
}

fn params(hidden_paths: &[String]) -> GitConfigParams<'_> {
    GitConfigParams {
        repo_url: "https://git.example.org/a.git",
        commit_author_name: "Bot",
        commit_author_email: "bot@example.org",
        hooks_path: Path::new("/opt/factory/hooks-empty"),
        ssl_ca_info: None,
        extra_config: &[],
        hidden_paths,
    }
}

#[test]
fn the_rendered_config_enables_non_cone_sparse_checkout_without_hidden_paths() {
    let config = render_git_config(&params(&[]));

    assert!(config.contains("\tsparseCheckout = true\n"), "{config}");
    assert!(
        config.contains("\tsparseCheckoutCone = false\n"),
        "{config}"
    );
}

#[test]
fn a_rewrite_replaces_the_sparse_patterns_the_agent_edited() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let hidden = vec!["talos/**".to_owned()];

    rewrite_git_config(repo, &params(&hidden)).unwrap();
    let sparse = repo.join(".git/info/sparse-checkout");
    assert_eq!(
        std::fs::read_to_string(&sparse).unwrap(),
        "/*\n!/talos/**\n"
    );

    // The agent disables sparse checkout and empties the patterns.
    std::fs::write(
        repo.join(".git/config"),
        "[core]\n\tsparseCheckout = false\n",
    )
    .unwrap();
    std::fs::write(&sparse, "").unwrap();
    rewrite_git_config(repo, &params(&hidden)).unwrap();

    let config = std::fs::read_to_string(repo.join(".git/config")).unwrap();
    assert!(config.contains("sparseCheckout = true"), "{config}");
    assert_eq!(
        std::fs::read_to_string(&sparse).unwrap(),
        "/*\n!/talos/**\n"
    );
}

#[test]
fn without_hidden_paths_the_sparse_patterns_include_everything() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".git")).unwrap();

    rewrite_git_config(temp.path(), &params(&[])).unwrap();

    let sparse = temp.path().join(".git/info/sparse-checkout");
    assert_eq!(std::fs::read_to_string(sparse).unwrap(), "/*\n");
}

#[test]
fn an_invalid_hidden_glob_fails_the_rewrite() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".git")).unwrap();
    let hidden = vec!["{a,b}/**".to_owned()];

    let result = rewrite_git_config(temp.path(), &params(&hidden));

    assert!(result.is_err());
}

#[tokio::test]
async fn a_fresh_clone_has_no_hidden_directory_and_fetched_none_of_its_blobs() {
    let fx = Fixture::new();
    let hidden_blob = git_in(&fx.upstream, &["rev-parse", "main:talos/cp.yaml"]);

    fx.prepare(&["talos/**"]).await;

    assert!(!fx.exists("talos"));
    assert!(fx.exists("README.md"));
    assert!(fx.exists("configs/top.txt"));
    let objects = fx.local_git(&[
        "cat-file",
        "--batch-all-objects",
        "--batch-check=%(objectname)",
    ]);
    assert!(!objects.contains(&hidden_blob), "blob was fetched");
    assert_eq!(fx.local_git(&["status", "--porcelain"]), "");
}

#[tokio::test]
async fn a_wildcard_in_the_last_component_hides_only_that_directory_level() {
    let fx = Fixture::new();

    fx.prepare(&["configs/*"]).await;

    assert!(!fx.exists("configs/top.txt"));
    assert!(fx.exists("configs/nested/visible.txt"));
}

#[tokio::test]
async fn a_path_dropped_from_the_list_comes_back_on_the_next_prepare() {
    let fx = Fixture::new();
    fx.prepare(&["talos/**", "configs/*"]).await;
    assert!(!fx.exists("talos"));

    fx.prepare(&["configs/*"]).await;
    assert_eq!(
        std::fs::read_to_string(fx.local.join("talos/cp.yaml")).unwrap(),
        "cert: A\n"
    );
    assert!(!fx.exists("configs/top.txt"));

    // The last hidden glob goes too.
    fx.prepare(&[]).await;
    assert!(fx.exists("configs/top.txt"));
    assert_eq!(fx.local_git(&["status", "--porcelain"]), "");
}

#[tokio::test]
async fn the_wip_restore_does_not_recreate_hidden_files() {
    let fx = Fixture::new();
    let seed = fx.seed();
    git_in(&seed, &["checkout", "-b", "wip"]);
    std::fs::write(seed.join("README.md"), "wip work\n").unwrap();
    std::fs::write(seed.join("talos/cp.yaml"), "cert: WIP\n").unwrap();
    git_in(&seed, &["commit", "--no-gpg-sign", "-am", "wip"]);
    git_in(&seed, &["push", "origin", "wip:factory/wip/DF-1"]);

    let blobs = [
        git_in(&seed, &["rev-parse", "wip:talos/cp.yaml"]),
        git_in(&seed, &["rev-parse", "main:talos/cp.yaml"]),
        git_in(&seed, &["rev-parse", "main:talos/sub/x.yaml"]),
    ];

    fx.prepare(&["talos/**"]).await;

    assert_eq!(
        std::fs::read_to_string(fx.local.join("README.md")).unwrap(),
        "wip work\n"
    );
    assert!(!fx.exists("talos"));
    let objects = fx.local_git(&[
        "cat-file",
        "--batch-all-objects",
        "--batch-check=%(objectname)",
    ]);
    for blob in blobs {
        assert!(!objects.contains(&blob), "blob {blob} was fetched");
    }
    assert_eq!(fx.local_git(&["status", "--porcelain"]), "M README.md");
}

#[tokio::test]
async fn a_wip_snapshot_restores_deletions_and_additions_outside_hidden_paths() {
    let fx = Fixture::new();
    let seed = fx.seed();
    git_in(&seed, &["checkout", "-b", "wip"]);
    git_in(&seed, &["rm", "-q", "configs/top.txt"]);
    std::fs::write(seed.join("added.txt"), "added\n").unwrap();
    git_in(&seed, &["add", "added.txt"]);
    git_in(&seed, &["commit", "--no-gpg-sign", "-m", "wip"]);
    git_in(&seed, &["push", "origin", "wip:factory/wip/DF-1"]);

    fx.prepare(&["talos/**"]).await;

    assert!(!fx.exists("configs/top.txt"));
    assert_eq!(
        std::fs::read_to_string(fx.local.join("added.txt")).unwrap(),
        "added\n"
    );
    assert!(!fx.exists("talos"));
}

#[tokio::test]
async fn a_commit_outside_hidden_paths_changes_only_the_edited_file() {
    let fx = Fixture::new();
    fx.prepare(&["talos/**"]).await;
    std::fs::write(fx.local.join("README.md"), "edited\n").unwrap();

    fx.local_git(&[
        "-c",
        "user.name=a",
        "-c",
        "user.email=a@b",
        "commit",
        "-q",
        "-a",
        "-m",
        "edit",
    ]);

    assert_eq!(
        fx.local_git(&["diff", "--name-status", "HEAD~1", "HEAD"]),
        "M\tREADME.md"
    );
}

#[tokio::test]
async fn a_visible_copy_of_a_hidden_file_does_not_hide_its_deletion_from_the_wip_reset() {
    let fx = Fixture::new();
    fx.prepare(&["talos/**"]).await;
    std::fs::write(fx.local.join("copy.yaml"), "cert: A\n").unwrap();

    let result = finalize(&fx, &["talos/**"]).await;

    let wip = result.run_report.workspace.expect("state").wip_sha;
    assert!(!wip.is_empty());
    let changed = git_in(&fx.upstream, &["diff", "--name-status", "main", &wip]);
    assert_eq!(changed, "A\tcopy.yaml");
}

#[tokio::test]
async fn a_hidden_file_with_a_name_that_is_not_utf8_stays_out_of_the_wip_snapshot() {
    use std::os::unix::ffi::OsStrExt;

    let fx = Fixture::new();
    fx.prepare(&["talos/**"]).await;
    std::fs::create_dir_all(fx.local.join("talos")).unwrap();
    std::fs::write(
        fx.local
            .join(std::ffi::OsStr::from_bytes(b"talos/caf\xe9.yaml")),
        "cert: NEW\n",
    )
    .unwrap();
    std::fs::write(fx.local.join("README.md"), "visible edit\n").unwrap();

    let result = finalize(&fx, &["talos/**"]).await;

    let wip = result.run_report.workspace.expect("state").wip_sha;
    assert!(!wip.is_empty());
    let changed = git_in(&fx.upstream, &["diff", "--name-status", "main", &wip]);
    assert_eq!(changed, "M\tREADME.md");
}

#[tokio::test]
async fn a_secret_file_with_a_name_that_is_not_utf8_stays_out_of_the_wip_snapshot() {
    use std::os::unix::ffi::OsStrExt;

    let fx = Fixture::new();
    fx.prepare(&[]).await;
    std::fs::write(
        fx.local.join(std::ffi::OsStr::from_bytes(b"caf\xe9.pem")),
        "-----BEGIN PRIVATE KEY-----\n",
    )
    .unwrap();
    std::fs::write(fx.local.join("README.md"), "visible edit\n").unwrap();

    let result = finalize(&fx, &[]).await;

    let wip = result.run_report.workspace.expect("state").wip_sha;
    assert!(!wip.is_empty());
    let changed = git_in(&fx.upstream, &["diff", "--name-status", "main", &wip]);
    assert_eq!(changed, "M\tREADME.md");
}

#[tokio::test]
async fn newly_hidden_files_are_removed_whatever_their_state() {
    let fx = Fixture::new();
    fx.prepare(&[]).await;
    std::fs::write(fx.local.join("talos/cp.yaml"), "cert: EDITED\n").unwrap();
    std::fs::write(fx.local.join("talos/new.yaml"), "new\n").unwrap();
    std::fs::write(fx.local.join("talos/secret.local"), "ignored\n").unwrap();
    std::fs::write(fx.local.join("README.md"), "kept\n").unwrap();

    let result = fx.prepare(&["talos/**"]).await;

    assert!(!fx.exists("talos"));
    assert_eq!(
        std::fs::read_to_string(fx.local.join("README.md")).unwrap(),
        "kept\n"
    );
    assert_eq!(fx.local_git(&["status", "--porcelain"]), "M README.md");
    assert_eq!(
        fx.local_git(&["ls-files", "-t", "talos/cp.yaml"]),
        "S talos/cp.yaml"
    );
    // `sparse-checkout reapply` must not write a worktree config next to the runtime-owned one
    assert!(!fx.exists(".git/config.worktree"));
    let config = std::fs::read_to_string(fx.local.join(".git/config")).unwrap();
    assert!(!config.contains("worktreeConfig"), "{config}");
    let notice = result
        .notices
        .iter()
        .find(|n| n.code == "workspace.hidden_paths_removed")
        .expect("notice");
    for path in ["talos/cp.yaml", "talos/new.yaml", "talos/secret.local"] {
        assert!(notice.message.contains(path), "{path}: {}", notice.message);
    }
}

#[tokio::test]
async fn a_symlinked_parent_directory_is_not_followed_during_removal() {
    let fx = Fixture::new();
    fx.prepare(&[]).await;
    let outside = fx.temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("x.yaml"), "precious\n").unwrap();
    std::fs::remove_dir_all(fx.local.join("talos/sub")).unwrap();
    std::os::unix::fs::symlink(&outside, fx.local.join("talos/sub")).unwrap();

    fx.prepare(&["talos/**"]).await;

    assert_eq!(
        std::fs::read_to_string(outside.join("x.yaml")).unwrap(),
        "precious\n"
    );
    assert!(!fx.exists("talos/cp.yaml"));
}

#[tokio::test]
async fn a_hidden_path_is_removed_again_after_the_agent_restored_it() {
    let fx = Fixture::new();
    fx.prepare(&["talos/**"]).await;
    std::fs::write(fx.local.join(".git/info/sparse-checkout"), "").unwrap();
    fx.local_git(&["sparse-checkout", "disable"]);
    assert!(fx.exists("talos/cp.yaml"));

    fx.prepare(&["talos/**"]).await;

    assert!(!fx.exists("talos"));
    assert_eq!(fx.local_git(&["status", "--porcelain"]), "");
}

fn finalize_params<'a>(
    fx: &'a Fixture,
    workspace: &'a Workspace,
    runtime_dir: &'a Path,
) -> FinalizeParams<'a> {
    FinalizeParams {
        repo_dir: &fx.local,
        runtime_dir,
        workspace,
        run_id: "run-1",
        outcome: RunOutcome::Completed,
        detail: "",
        harness_session_id: "",
        hooks_path: &fx.hooks,
        ssl_ca_info: None,
        wip_size_cap_bytes: 10 * 1024 * 1024,
        diverged: false,
    }
}

/// A remote WIP branch that a clean finalize deletes.
fn push_old_wip_branch(fx: &Fixture) {
    let seed = fx.seed();
    git_in(&seed, &["push", "origin", "main:factory/wip/DF-1"]);
}

async fn finalize(fx: &Fixture, hidden: &[&str]) -> crate::workspace::FinalizeResult {
    let git = fx.git();
    let runtime_dir = fx.temp.path().join("runtime");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let workspace = fx.workspace(hidden);
    let mut last_pushed = Some(git.current_head().await.unwrap());
    finalize_workspace(
        &git,
        &finalize_params(fx, &workspace, &runtime_dir),
        &mut last_pushed,
    )
    .await
    .expect("finalize")
}

fn deleted_wip(result: &crate::workspace::FinalizeResult) -> bool {
    result
        .push_reports
        .iter()
        .any(|r| r.target == PushTarget::WipBranch as i32 && r.status == PushStatus::Deleted as i32)
}

#[tokio::test]
async fn a_clean_worktree_with_hidden_paths_deletes_the_wip_branch() {
    let fx = Fixture::new();
    push_old_wip_branch(&fx);
    fx.prepare(&["talos/**"]).await;
    // The old WIP branch was restored at prepare; a clean tree is what the agent leaves.
    fx.local_git(&["checkout", "--", "."]);

    let result = finalize(&fx, &["talos/**"]).await;

    assert!(deleted_wip(&result), "{:?}", result.push_reports);
}

#[tokio::test]
async fn a_clean_worktree_whose_skip_worktree_bits_the_agent_cleared_deletes_the_wip_branch() {
    let fx = Fixture::new();
    push_old_wip_branch(&fx);
    fx.prepare(&["talos/**"]).await;
    fx.local_git(&[
        "update-index",
        "--no-skip-worktree",
        "talos/cp.yaml",
        "talos/sub/x.yaml",
    ]);

    let result = finalize(&fx, &["talos/**"]).await;

    assert!(deleted_wip(&result), "{:?}", result.push_reports);
}

#[tokio::test]
async fn the_wip_snapshot_changes_only_the_visible_files() {
    let fx = Fixture::new();
    fx.prepare(&["talos/**"]).await;
    std::fs::write(fx.local.join("README.md"), "edited\n").unwrap();

    let result = finalize(&fx, &["talos/**"]).await;

    let wip = result.run_report.workspace.expect("state").wip_sha;
    assert!(!wip.is_empty());
    let changed = git_in(&fx.upstream, &["diff", "--name-only", "main", &wip]);
    assert_eq!(changed, "README.md");
}

#[tokio::test]
async fn the_wip_snapshot_leaves_files_the_agent_created_under_hidden_paths_out() {
    let fx = Fixture::new();
    fx.prepare(&["talos/**"]).await;
    std::fs::create_dir_all(fx.local.join("talos")).unwrap();
    std::fs::write(fx.local.join("talos/new.yaml"), "cert: NEW\n").unwrap();
    std::fs::write(fx.local.join("README.md"), "edited\n").unwrap();

    let result = finalize(&fx, &["talos/**"]).await;

    let wip = result.run_report.workspace.expect("state").wip_sha;
    let changed = git_in(&fx.upstream, &["diff", "--name-only", "main", &wip]);
    assert_eq!(changed, "README.md");
}

#[tokio::test]
async fn a_newly_hidden_file_with_a_name_that_is_not_utf8_is_removed_and_reset() {
    use std::os::unix::ffi::OsStrExt;

    let fx = Fixture::new();
    let seed = fx.seed();
    let name = std::ffi::OsStr::from_bytes(b"talos/caf\xe9.yaml");
    std::fs::write(seed.join(name), "cert: C\n").unwrap();
    git_in(&seed, &["add", "."]);
    git_in(&seed, &["commit", "--no-gpg-sign", "-m", "latin1"]);
    git_in(&seed, &["push", "origin", "main"]);
    fx.prepare(&[]).await;
    std::fs::write(fx.local.join(name), "cert: EDITED\n").unwrap();

    fx.prepare(&["talos/**"]).await;

    assert!(!fx.exists("talos"));
    assert_eq!(fx.local_git(&["status", "--porcelain"]), "");
}

/// Pushes a WIP branch built by `change` on top of `main`.
fn push_wip(fx: &Fixture, change: impl FnOnce(&Path)) {
    let seed = fx.seed();
    git_in(&seed, &["checkout", "-b", "wip"]);
    change(&seed);
    git_in(&seed, &["add", "-A"]);
    git_in(&seed, &["commit", "--no-gpg-sign", "-m", "wip"]);
    git_in(&seed, &["push", "origin", "wip:factory/wip/DF-1"]);
}

#[tokio::test]
async fn a_file_renamed_in_the_wip_snapshot_is_moved_in_the_workspace() {
    let fx = Fixture::new();
    push_wip(&fx, |seed| {
        git_in(seed, &["mv", "configs/top.txt", "configs/moved.txt"]);
    });

    fx.prepare(&["talos/**"]).await;

    assert!(!fx.exists("configs/top.txt"));
    assert_eq!(
        std::fs::read_to_string(fx.local.join("configs/moved.txt")).unwrap(),
        "top\n"
    );
}

#[tokio::test]
async fn a_file_with_a_name_that_is_not_utf8_in_the_wip_snapshot_is_restored() {
    use std::os::unix::ffi::OsStrExt;

    let fx = Fixture::new();
    let name = std::ffi::OsStr::from_bytes(b"caf\xe9.txt");
    push_wip(&fx, |seed| {
        std::fs::write(seed.join(name), "latin1\n").unwrap();
    });

    fx.prepare(&["talos/**"]).await;

    assert_eq!(
        std::fs::read_to_string(fx.local.join(name)).unwrap(),
        "latin1\n"
    );
}

#[tokio::test]
async fn a_wip_snapshot_with_more_paths_than_one_call_takes_is_restored_completely() {
    let fx = Fixture::new();
    push_wip(&fx, |seed| {
        for n in 0..=crate::git::PATHSPEC_CHUNK * 2 {
            std::fs::write(seed.join(format!("added-{n}.txt")), format!("{n}\n")).unwrap();
        }
        std::fs::write(seed.join("talos/new.yaml"), "hidden\n").unwrap();
    });

    fx.prepare(&["talos/**"]).await;

    for n in 0..=crate::git::PATHSPEC_CHUNK * 2 {
        assert!(fx.exists(&format!("added-{n}.txt")), "added-{n}.txt");
    }
    assert!(!fx.exists("talos"));
}
