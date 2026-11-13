//! Workspace lifecycle management for Dark Factory tasks.
//!
//! Follows `design/agent-run.md` §6 and `specs/workspace-lifecycle/spec.md`.
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::Path;

use anyhow::Context as _;

use df_core::glob::{GlobSet, hidden_glob_set};
use df_proto::agent::v1::{
    PushReport, PushStatus, PushTarget, RewritePolicy, RunOutcome, RunReport, Workspace,
    WorkspaceState,
};
use df_proto::gateway::v1::PushCheck;
use df_proto::push_reasons::{NON_FAST_FORWARD, PUSH_TOO_LARGE};
use df_proto::transcript::v1::{Notice, NoticeLevel};

use crate::git::{
    DEFAULT_COMMIT_AUTHOR_EMAIL, DEFAULT_COMMIT_AUTHOR_NAME, Force, Git, GitConfigParams,
    nul_separated, proxy_ca_override, rewrite_git_config, shown_args,
};
use crate::hidden_paths;

/// Result of preparing a workspace.
#[derive(Debug, Clone)]
pub struct PrepareResult {
    /// True if the local and remote task branches have diverged.
    pub diverged: bool,
    /// Head commit SHA after prepare completes.
    pub head_sha: String,
    /// Push report emitted if unpushed commits from an earlier run were pushed during prepare.
    pub push_report: Option<PushReport>,
    /// True if the prepare-time push ran and failed, so `head_sha` is not on the remote.
    pub push_failed: bool,
    /// Notices generated during prepare (e.g. `workspace.diverged`).
    pub notices: Vec<Notice>,
}

/// Why prepare can't start a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    /// The task works on an existing branch that the remote doesn't have.
    BranchMissing(String),
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BranchMissing(branch) => {
                write!(f, "the branch `{branch}` does not exist on the remote")
            }
        }
    }
}

impl std::error::Error for PrepareError {}

/// File with the commit the task branch had on the remote when the runtime last fetched or
/// pushed it (`remote_sha`, design D7). It lives in the runtime directory next to the checkout,
/// which the harness can't write, and survives runs. The remote-tracking ref isn't used for the
/// lease: the agent's own `git fetch` moves it.
fn remote_sha_file(git: &Git) -> Option<std::path::PathBuf> {
    git.runtime_dir.as_ref().map(|dir| dir.join("remote_sha"))
}

fn load_remote_sha(git: &Git) -> Option<String> {
    let stored = std::fs::read_to_string(remote_sha_file(git)?).ok()?;
    Some(stored.trim().to_owned()).filter(|sha| !sha.is_empty())
}

/// Records `sha`, never following a link planted at the path; a failure only costs the lease
/// (the next push is plain and may be rejected).
fn store_remote_sha(git: &Git, sha: &str) {
    let Some(dir) = git.runtime_dir.as_deref() else {
        return;
    };
    let path = dir.join("remote_sha");
    let written =
        crate::fs_util::create_real_dir(dir).and_then(|()| crate::fs_util::write_fresh(&path, sha));
    if let Err(error) = written {
        tracing::warn!(error = %error, "failed to persist the remote sha of the task branch");
    }
}

fn rewrite_allowed(workspace: &Workspace) -> bool {
    workspace.rewrite_policy() == RewritePolicy::WithLease
}

/// How to push `head` to the task branch: with lease on the remote commit the runtime last saw
/// if the policy allows rewrites and `head` doesn't contain that commit, else plainly.
async fn force_for(git: &Git, workspace: &Workspace, head: &str) -> Force {
    if !rewrite_allowed(workspace) {
        return Force::No;
    }
    match load_remote_sha(git) {
        Some(remote) if !git.is_ancestor(&remote, head).await => Force::WithLease(remote),
        _ => Force::No,
    }
}

/// Builds the runtime-owned git config parameters for a workspace.
fn config_params<'a>(
    workspace: &'a Workspace,
    hooks_path: &'a Path,
    ssl_ca_info: Option<&'a Path>,
) -> GitConfigParams<'a> {
    let identity = workspace.commit_identity.as_ref();
    GitConfigParams {
        repo_url: &workspace.repo_url,
        commit_author_name: identity.map_or(DEFAULT_COMMIT_AUTHOR_NAME, |c| c.name.as_str()),
        commit_author_email: identity.map_or(DEFAULT_COMMIT_AUTHOR_EMAIL, |c| c.email.as_str()),
        hooks_path,
        ssl_ca_info,
        extra_config: &workspace.git_config,
        hidden_paths: &workspace.hidden_paths,
    }
}

/// Prepares the repository workspace before a run begins.
///
/// Implements `design/agent-run.md` §6.2:
/// - Fresh clone if `/data/repo/.git` doesn't exist (`--filter=blob:none`, no-checkout,
///   check out task branch, restore WIP files if available).
/// - If workspace already exists: rewrite `.git/config`, fetch from remote, push unpushed commits,
///   fast-forward merge if remote is ahead, or detect divergence and emit notice.
/// - Never runs `git clean`.
pub async fn prepare_workspace(
    git: &Git,
    repo_dir: &Path,
    workspace: &Workspace,
    hooks_path: &Path,
    ssl_ca_info: Option<&Path>,
) -> anyhow::Result<PrepareResult> {
    let config_params = config_params(workspace, hooks_path, ssl_ca_info);
    if repo_dir.join(".git").exists() {
        sync_existing_workspace(git, repo_dir, workspace, &config_params).await
    } else {
        clone_workspace(git, repo_dir, workspace, &config_params).await
    }
}

impl PrepareResult {
    /// A workspace that needs no follow-up: not diverged, nothing pushed.
    fn clean(head_sha: String) -> Self {
        Self {
            diverged: false,
            head_sha,
            push_report: None,
            push_failed: false,
            notices: Vec::new(),
        }
    }

    /// A workspace after the prepare-time push of `report`.
    fn pushed(head_sha: String, report: PushReport) -> Self {
        Self {
            push_failed: report.status() != PushStatus::Ok,
            push_report: Some(report),
            ..Self::clean(head_sha)
        }
    }
}

/// True if `git rev-parse --verify <rev>` succeeds.
async fn ref_exists(git: &Git, rev: &str) -> bool {
    git.run_cmd(&["rev-parse", "--verify", rev])
        .await
        .is_ok_and(|o| o.status.success())
}

/// Fresh workspace: blobless clone without checkout, runtime-owned config, task branch checkout
/// and restore of the WIP snapshot if there is one.
async fn clone_workspace(
    git: &Git,
    repo_dir: &Path,
    workspace: &Workspace,
    config_params: &GitConfigParams<'_>,
) -> anyhow::Result<PrepareResult> {
    let parent_dir = repo_dir.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent_dir)?;

    let repo_dir_str = repo_dir.to_str().ok_or_else(|| {
        anyhow::anyhow!("repository path {} is not valid UTF-8", repo_dir.display())
    })?;

    // blobless clone with --no-checkout run in parent_dir; `.git/config` with the scoped CA
    // does not exist yet
    let mut args = Vec::new();
    let ca = proxy_ca_override(&workspace.repo_url, config_params.ssl_ca_info);
    if let Some(ca) = &ca {
        args.extend(["-c", ca.as_str()]);
    }
    args.extend([
        "clone",
        "--filter=blob:none",
        "--no-checkout",
        &workspace.repo_url,
        repo_dir_str,
    ]);
    let clone_out = git.run_network_cmd_in(parent_dir, &args).await?;
    if !clone_out.status.success() {
        let stderr = String::from_utf8_lossy(&clone_out.stderr);
        anyhow::bail!("git clone failed: {}", stderr.trim());
    }

    // Write runtime-owned .git/config and remove config.worktree
    rewrite_git_config(repo_dir, config_params)?;

    // Start from origin/<task branch> if the remote has it, else from the base branch
    let task_remote_ref = format!("origin/{}", workspace.task_branch);
    let start_point = if ref_exists(git, &task_remote_ref).await {
        match git.git_output(&["rev-parse", &task_remote_ref]).await {
            Ok(sha) => store_remote_sha(git, &sha),
            // Only the lease is lost: pushes stay plain until the next fetch records a sha.
            Err(error) => tracing::warn!(
                error = %error,
                task_branch = %workspace.task_branch,
                "could not read the remote task branch; the lease is not tracked"
            ),
        }
        task_remote_ref
    } else if workspace.existing_branch {
        return Err(PrepareError::BranchMissing(workspace.task_branch.clone()).into());
    } else {
        format!("origin/{}", workspace.base_branch)
    };
    git.git_output(&["checkout", "-B", &workspace.task_branch, &start_point])
        .await?;

    restore_wip_snapshot(git, workspace).await?;

    let head_sha = git.current_head().await.unwrap_or_default();
    Ok(PrepareResult::clean(head_sha))
}

/// Restores the files of `origin/<wip branch>` into the work tree if that branch exists: the
/// paths that differ between HEAD and the snapshot, except those below a hidden glob. A restore
/// of the whole tree would fetch the blobs of hidden paths too.
async fn restore_wip_snapshot(git: &Git, workspace: &Workspace) -> anyhow::Result<()> {
    let wip_remote_ref = format!("origin/{}", workspace.wip_branch);
    if !ref_exists(git, &wip_remote_ref).await {
        return Ok(());
    }
    let hidden = hidden_glob_set(&workspace.hidden_paths)?;
    // A failure here must fail prepare: otherwise the tree looks clean and finalize deletes
    // the remote WIP branch, the only copy of the previous run's uncommitted work.
    let changed = git
        .run_ok(&[
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            "HEAD",
            &wip_remote_ref,
        ])
        .await
        .with_context(|| format!("listing the changes of WIP snapshot {wip_remote_ref}"))?;
    let paths: Vec<&OsStr> = nul_separated(&changed.stdout)
        .filter(|path| !hidden.is_match(Path::new(path)))
        .collect();
    let source = format!("--source={wip_remote_ref}");
    git.run_ok_chunked(
        &[
            "--literal-pathspecs",
            "restore",
            &source,
            "--worktree",
            "--",
        ],
        &paths,
    )
    .await
    .with_context(|| format!("restoring WIP snapshot {wip_remote_ref} failed"))?;
    Ok(())
}

/// Existing workspace: sanitize the config, fetch, check out the task branch and bring it in
/// line with the remote.
async fn sync_existing_workspace(
    git: &Git,
    repo_dir: &Path,
    workspace: &Workspace,
    config_params: &GitConfigParams<'_>,
) -> anyhow::Result<PrepareResult> {
    rewrite_git_config(repo_dir, config_params)?;
    let removed = hidden_paths::enforce(git, repo_dir, &workspace.hidden_paths).await?;

    // Fetch base and task branch from remote (Finding 17)
    let fetch_out = git.run_network_cmd(&["fetch", "origin"]).await?;
    if !fetch_out.status.success() {
        let stderr = String::from_utf8_lossy(&fetch_out.stderr);
        anyhow::bail!(
            "git fetch origin failed during workspace prepare: {}",
            stderr.trim()
        );
    }

    let task_remote_ref = format!("origin/{}", workspace.task_branch);
    let has_remote_task = ref_exists(git, &task_remote_ref).await;

    // Ensure task branch is checked out. If the local branch is missing (e.g. the previous
    // run crashed after the clone), recreate it the way the fresh-clone path does.
    let checkout_out = git.run_cmd(&["checkout", &workspace.task_branch]).await?;
    if !checkout_out.status.success() {
        if workspace.existing_branch && !has_remote_task {
            return Err(PrepareError::BranchMissing(workspace.task_branch.clone()).into());
        }
        let start_point = if has_remote_task {
            task_remote_ref.clone()
        } else {
            format!("origin/{}", workspace.base_branch)
        };
        git.git_output(&["checkout", "-B", &workspace.task_branch, &start_point])
            .await?;
    }

    let mut result = if has_remote_task {
        sync_with_remote_task_branch(git, workspace, &task_remote_ref).await?
    } else {
        push_new_task_branch(git, workspace).await?
    };
    result
        .notices
        .extend(hidden_paths::removed_notice(&removed));
    Ok(result)
}

/// The remote has the task branch: push local-only commits, fast-forward, or detect divergence.
async fn sync_with_remote_task_branch(
    git: &Git,
    workspace: &Workspace,
    task_remote_ref: &str,
) -> anyhow::Result<PrepareResult> {
    let local_sha = git.current_head().await?;
    let remote_sha = git.git_output(&["rev-parse", task_remote_ref]).await?;
    // What the runtime knew before this fetch: a remote still at it was not touched by others.
    let known_remote = load_remote_sha(git);
    if local_sha == remote_sha {
        // Synchronized, nothing to do
        store_remote_sha(git, &remote_sha);
        return Ok(PrepareResult::clean(local_sha));
    }

    let local_ahead = git
        .commits_ahead(task_remote_ref, &local_sha)
        .await
        .unwrap_or(0);
    let remote_ahead = git
        .commits_ahead(&local_sha, task_remote_ref)
        .await
        .unwrap_or(0);

    if remote_ahead == 0 && local_ahead > 0 {
        // Local has unpushed commits from a killed run: push them first
        let base_remote_ref = format!("origin/{}", workspace.base_branch);
        let ahead_of_base = git
            .commits_ahead(&base_remote_ref, &local_sha)
            .await
            .unwrap_or(local_ahead);
        let report = push_task_branch(
            git,
            workspace,
            &local_sha,
            ahead_of_base,
            "Pushed unpushed commits during workspace prepare",
            Force::No,
        )
        .await?;
        Ok(PrepareResult::pushed(local_sha, report))
    } else if local_ahead == 0 && remote_ahead > 0 {
        // Remote is ahead: fast-forward merge
        git.git_output(&["merge", "--ff-only", task_remote_ref])
            .await?;
        store_remote_sha(git, &remote_sha);
        let head_sha = git.current_head().await.unwrap_or_default();
        Ok(PrepareResult::clean(head_sha))
    } else if rewrite_allowed(workspace) && known_remote.as_deref() == Some(remote_sha.as_str()) {
        // A rewrite (rebase) from a killed run: the remote is where the runtime left it
        let base_remote_ref = format!("origin/{}", workspace.base_branch);
        let ahead_of_base = git
            .commits_ahead(&base_remote_ref, &local_sha)
            .await
            .unwrap_or(local_ahead);
        let report = push_task_branch(
            git,
            workspace,
            &local_sha,
            ahead_of_base,
            "Pushed the rewritten task branch with lease during workspace prepare",
            Force::WithLease(remote_sha),
        )
        .await?;
        Ok(PrepareResult::pushed(local_sha, report))
    } else {
        // Diverged: local and remote both have commits the other lacks
        // Leave both untouched, emit workspace.diverged notice. `remote_sha` keeps the commit
        // the runtime itself last saw: recording the fetched remote would let a later rewrite
        // push over the other side's commits with that lease.
        let notice = Notice {
            level: NoticeLevel::Warning as i32,
            code: "workspace.diverged".to_string(),
            message: "Task branch diverged from origin".to_string(),
        };
        Ok(PrepareResult {
            diverged: true,
            notices: vec![notice],
            ..PrepareResult::clean(local_sha)
        })
    }
}

/// The remote lacks the task branch: push the local commits ahead of base, if there are any.
async fn push_new_task_branch(git: &Git, workspace: &Workspace) -> anyhow::Result<PrepareResult> {
    let base_remote_ref = format!("origin/{}", workspace.base_branch);
    let local_sha = git.current_head().await?;
    let local_ahead = git
        .commits_ahead(&base_remote_ref, &local_sha)
        .await
        .unwrap_or(0);
    if local_ahead == 0 {
        return Ok(PrepareResult::clean(local_sha));
    }
    let report = push_task_branch(
        git,
        workspace,
        &local_sha,
        local_ahead,
        "Pushed task branch during workspace prepare",
        Force::No,
    )
    .await?;
    Ok(PrepareResult::pushed(local_sha, report))
}

/// Pushes the local task branch to the remote one and reports the result.
async fn push_task_branch(
    git: &Git,
    workspace: &Workspace,
    local_sha: &str,
    commits_ahead_of_base: u32,
    detail: &str,
    force: Force,
) -> anyhow::Result<PushReport> {
    let refspec = format!("{}:{}", workspace.task_branch, workspace.task_branch);
    let push_output = git.push("origin", &refspec, force).await?;
    let (status, detail) = if push_output.status.success() {
        store_remote_sha(git, local_sha);
        (PushStatus::Ok, detail.to_string())
    } else {
        rejected_push(&push_output)
    };
    Ok(PushReport {
        target: PushTarget::TaskBranch as i32,
        r#ref: format!("refs/heads/{}", workspace.task_branch),
        sha: local_sha.to_string(),
        status: status as i32,
        commits_ahead_of_base,
        detail,
    })
}

/// Returns true if `path_str` matches the secret-file denylist.
///
/// Follows `specs/workspace-lifecycle/spec.md`:
/// "drop paths matching the secret-file denylist (.env*, *.pem, *.key, id_*, ...)"
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "`lower` is lower case already; `Path::extension` misses names like `.pem`"
)]
pub fn is_secret_file(path_str: &str) -> bool {
    let file_name = match path_str.rfind('/') {
        Some(idx) => &path_str[idx + 1..],
        None => path_str,
    };
    let lower = file_name.to_ascii_lowercase();
    if lower.starts_with(".env") {
        return true;
    }
    // Match SSH private key files specifically (Finding 6)
    if lower == "id_rsa"
        || lower == "id_dsa"
        || lower == "id_ecdsa"
        || lower == "id_ed25519"
        || lower.starts_with("id_rsa.")
        || lower.starts_with("id_ed25519.")
        || lower.starts_with("id_ecdsa.")
        || lower.starts_with("id_dsa.")
    {
        return true;
    }
    if lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".pfx")
        || lower.ends_with(".p12")
        || lower.ends_with(".pkcs12")
    {
        return true;
    }
    false
}

/// Pushes the task branch if HEAD has advanced since `last_pushed_sha`.
pub async fn push_after_turn(
    git: &Git,
    workspace: &Workspace,
    last_pushed_sha: &mut Option<String>,
) -> anyhow::Result<Option<PushReport>> {
    let current_sha = git.current_head().await?;
    if last_pushed_sha.as_deref() == Some(&current_sha) {
        return Ok(None);
    }

    // Push HEAD to task branch ref (Finding 8)
    let refspec = format!("HEAD:refs/heads/{}", workspace.task_branch);
    let force = force_for(git, workspace, &current_sha).await;
    let push_out = git.push("origin", &refspec, force).await?;

    if push_out.status.success() {
        *last_pushed_sha = Some(current_sha.clone());
        store_remote_sha(git, &current_sha);
        let base_remote_ref = format!("origin/{}", workspace.base_branch);
        let ahead = git
            .commits_ahead(&base_remote_ref, &current_sha)
            .await
            .unwrap_or(1);
        let report = PushReport {
            target: PushTarget::TaskBranch as i32,
            r#ref: format!("refs/heads/{}", workspace.task_branch),
            sha: current_sha,
            status: PushStatus::Ok as i32,
            commits_ahead_of_base: ahead,
            detail: String::new(),
        };
        Ok(Some(report))
    } else {
        let (status, detail) = rejected_push(&push_out);
        let report = PushReport {
            target: PushTarget::TaskBranch as i32,
            r#ref: format!("refs/heads/{}", workspace.task_branch),
            sha: current_sha,
            status: status as i32,
            commits_ahead_of_base: 0,
            detail,
        };
        Ok(Some(report))
    }
}

/// Status and detail of a failed `git push --porcelain` (iter2-push-checks design D6). A ref the
/// Git proxy rejected with a `df:` reason maps by that reason, which is also the detail; any
/// `df:secret-scan` among joined findings wins. Other failures keep the whole output as detail:
/// a rejection is a non-fast-forward, anything else `FAILED`.
pub fn rejected_push(output: &std::process::Output) -> (PushStatus, String) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(reason) = porcelain_reasons(&stdout).find(|reason| reason.starts_with("df:")) {
        return (status_of_reason(reason), reason.to_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    let status = if combined.contains("non-fast-forward")
        || combined.contains("stale info")
        || combined.contains("[rejected]")
        || combined.contains("fetch first")
        || combined
            .lines()
            .any(|l| l.starts_with('!') && l.contains("rejected"))
    {
        PushStatus::RejectedNonFastForward
    } else {
        PushStatus::Failed
    };
    (status, combined.trim().to_string())
}

/// The reasons of the refs `git push --porcelain` rejected: `!<TAB><from>:<to><TAB><summary>
/// (<reason>)`.
fn porcelain_reasons(stdout: &str) -> impl Iterator<Item = &str> {
    stdout.lines().filter_map(|line| {
        let (_refs, summary) = line.strip_prefix("!\t")?.split_once('\t')?;
        summary.split_once(" (")?.1.strip_suffix(')')
    })
}

/// The push status of a `df:` reason; findings are joined with `; `.
fn status_of_reason(reason: &str) -> PushStatus {
    let mut codes = reason
        .split("; ")
        .filter_map(|finding| finding.split_whitespace().next());
    if codes
        .clone()
        .any(|code| PushCheck::from_reason_code(code) == Some(PushCheck::SecretScan))
    {
        return PushStatus::RejectedSecretDetected;
    }
    match codes.next() {
        Some(code) if code == PUSH_TOO_LARGE || PushCheck::from_reason_code(code).is_some() => {
            PushStatus::RejectedByCheck
        }
        Some(NON_FAST_FORWARD) => PushStatus::RejectedNonFastForward,
        _ => PushStatus::Failed,
    }
}

/// Result of finalizing a workspace.
#[derive(Debug, Clone)]
pub struct FinalizeResult {
    pub run_report: RunReport,
    pub push_reports: Vec<PushReport>,
    pub notices: Vec<Notice>,
}

/// Computes the total recursive disk size of a directory in bytes.
/// Does NOT follow symlinks to avoid directory traversal or infinite loops (Finding 13).
pub fn compute_dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    total += compute_dir_size(&path);
                } else if ft.is_file()
                    && let Ok(meta) = entry.metadata()
                {
                    total += meta.len();
                }
            }
        }
    }
    total
}

async fn run_with_wip_index(git: &Git, wip_index: &Path, args: &[&str]) -> anyhow::Result<String> {
    let stdout = wip_index_output(git, wip_index, args).await?;
    Ok(String::from_utf8_lossy(&stdout).trim().to_string())
}

/// Runs Git against the WIP index and returns its raw stdout, so `-z` path lists keep names
/// that are not UTF-8.
async fn wip_index_output<S: AsRef<OsStr>>(
    git: &Git,
    wip_index: &Path,
    args: &[S],
) -> anyhow::Result<Vec<u8>> {
    let mut cmd = git.build_command();
    cmd.env("GIT_INDEX_FILE", wip_index);
    // File names like `prod[1].key` are paths, not globs
    cmd.env("GIT_LITERAL_PATHSPECS", "1");
    cmd.args(args);
    let out = cmd
        .output()
        .await
        .with_context(|| format!("git {} failed", shown_args(args)))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("git {} failed: {}", shown_args(args), stderr.trim());
    }
    Ok(out.stdout)
}

/// Parameters for workspace finalization.
#[derive(Debug, Clone)]
pub struct FinalizeParams<'a> {
    pub repo_dir: &'a Path,
    pub runtime_dir: &'a Path,
    pub workspace: &'a Workspace,
    pub run_id: &'a str,
    pub outcome: RunOutcome,
    pub detail: &'a str,
    pub harness_session_id: &'a str,
    pub hooks_path: &'a Path,
    pub ssl_ca_info: Option<&'a Path>,
    pub wip_size_cap_bytes: u64,
    pub diverged: bool,
}

/// Finalizes the workspace at the end of a run.
///
/// Implements `design/agent-run.md` §6.5:
/// 1. Rewrites `.git/config` again.
/// 2. Pushes task branch if new commits were made.
/// 3. Writes WIP snapshot via separate index (`wip.index`), dropping secret files (.env*, *.pem, *.key, id_*),
///    verifying size cap, and force-pushing to `factory/wip/<key>`, or deletes remote WIP branch if worktree is clean.
/// 4. Leaves agent's index, branches, and `git status` completely unchanged.
/// 5. Assembles `RunReport`.
pub async fn finalize_workspace(
    git: &Git,
    params: &FinalizeParams<'_>,
    last_pushed_sha: &mut Option<String>,
) -> anyhow::Result<FinalizeResult> {
    let mut push_reports = Vec::new();
    let workspace = params.workspace;

    // 1. Rewrite .git/config
    let config_params = config_params(workspace, params.hooks_path, params.ssl_ca_info);
    rewrite_git_config(params.repo_dir, &config_params)?;

    // 2. Push task branch if needed
    if let Some(report) = push_after_turn(git, workspace, last_pushed_sha).await? {
        push_reports.push(report);
    }
    let task_branch_sha = git.current_head().await.unwrap_or_default();
    let task_branch_pushed = last_pushed_sha.as_deref() == Some(&task_branch_sha);

    // 3. WIP snapshot via separate index
    let wip_index = params.runtime_dir.join("wip.index");
    if wip_index.exists() {
        std::fs::remove_file(&wip_index).ok(); // best-effort cleanup
    }
    let wip = snapshot_wip(git, params, &wip_index).await?;
    push_reports.extend(wip.push_reports);
    if wip_index.exists() {
        std::fs::remove_file(&wip_index).ok(); // best-effort cleanup
    }

    let run_report = RunReport {
        outcome: params.outcome as i32,
        detail: params.detail.to_string(),
        retry_after: None,
        harness_session_id: params.harness_session_id.to_string(),
        workspace: Some(WorkspaceState {
            task_branch_sha,
            wip_sha: wip.sha,
            task_branch_pushed,
            diverged: params.diverged,
            size_bytes: compute_dir_size(params.repo_dir),
        }),
    };

    Ok(FinalizeResult {
        run_report,
        push_reports,
        notices: wip.notices,
    })
}

/// What the WIP snapshot step produced.
struct WipOutcome {
    /// SHA of the pushed snapshot commit; empty if none was pushed.
    sha: String,
    push_reports: Vec<PushReport>,
    notices: Vec<Notice>,
}

/// Snapshots uncommitted work into the WIP branch, or deletes the remote WIP branch if the
/// work tree is clean. Leaves the agent's index untouched by using `wip_index`.
async fn snapshot_wip(
    git: &Git,
    params: &FinalizeParams<'_>,
    wip_index: &Path,
) -> anyhow::Result<WipOutcome> {
    let mut wip = WipOutcome {
        sha: String::new(),
        push_reports: Vec::new(),
        notices: Vec::new(),
    };
    let hidden = hidden_glob_set(&params.workspace.hidden_paths)?;
    let tree = write_wip_tree(git, wip_index, &hidden).await?;
    let head_tree = git.git_output(&["rev-parse", "HEAD^{tree}"]).await?;
    if tree == head_tree {
        wip.push_reports
            .extend(delete_remote_wip(git, params.workspace).await?);
        return Ok(wip);
    }

    // Uncommitted work: check size cap
    let mut total_wip_bytes: u64 = 0;
    let diff_files = run_with_wip_index(
        git,
        wip_index,
        &["diff", "-z", "--name-only", "--no-renames", "HEAD"],
    )
    .await?;
    for f in diff_files.split('\0').filter(|f| !f.is_empty()) {
        if let Ok(meta) = params.repo_dir.join(f).metadata() {
            total_wip_bytes += meta.len();
        }
    }
    if total_wip_bytes > params.wip_size_cap_bytes {
        wip.notices.push(Notice {
            level: NoticeLevel::Warning as i32,
            code: "wip.size_cap_exceeded".to_string(),
            message: format!(
                "WIP snapshot size {total_wip_bytes} exceeds cap {}",
                params.wip_size_cap_bytes
            ),
        });
        return Ok(wip);
    }

    let report = push_wip_snapshot(git, params, wip_index, &tree).await?;
    if report.status() == PushStatus::Ok {
        wip.sha.clone_from(&report.sha);
    }
    wip.push_reports.push(report);
    Ok(wip)
}

/// Writes a tree of HEAD plus all modifications and untracked files (respecting .gitignore,
/// without secret files) using the separate WIP index.
async fn write_wip_tree(git: &Git, wip_index: &Path, hidden: &GlobSet) -> anyhow::Result<String> {
    // Read current HEAD into separate index
    let _ = run_with_wip_index(git, wip_index, &["read-tree", "HEAD"]).await?;
    // Add all modifications and untracked files respecting .gitignore. `--sparse`: without it
    // `add` fails for a file the agent created below a hidden path; the hidden paths are reset
    // below.
    let _ = run_with_wip_index(git, wip_index, &["add", "-A", "--sparse"]).await?;

    // Get list of tracked files in HEAD so we don't drop files already committed in HEAD (Finding 6).
    // `-z` keeps names unquoted and verbatim; the paths stay raw bytes so names that are not
    // UTF-8 reach Git unchanged.
    let head_files_out = git
        .run_ok(&["ls-tree", "-r", "-z", "--name-only", "HEAD"])
        .await
        .map(|out| out.stdout)
        .unwrap_or_default();
    let head_tracked_files: HashSet<&OsStr> = nul_separated(&head_files_out).collect();

    // Drop secret files from wip.index
    let ls_files = wip_index_output(git, wip_index, &["ls-files", "-z"]).await?;
    for f in nul_separated(&ls_files) {
        if is_secret_file(&f.to_string_lossy()) {
            reset_to_head(git, wip_index, &head_tracked_files, f).await?;
        }
    }

    // Leave hidden paths as they are in HEAD: the push check rejects any change to them, and
    // `add -A` stages files the agent created there and deletions of files that are absent.
    let staged = wip_index_output(
        git,
        wip_index,
        &[
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
            "-z",
            "HEAD",
        ],
    )
    .await?;
    for f in nul_separated(&staged) {
        if hidden.is_match(Path::new(f)) {
            reset_to_head(git, wip_index, &head_tracked_files, f).await?;
        }
    }

    run_with_wip_index(git, wip_index, &["write-tree"]).await
}

/// Sets `path` in the WIP index back to HEAD's version, or drops it if HEAD has none. A failure
/// must abort the snapshot, otherwise the path would be pushed.
async fn reset_to_head(
    git: &Git,
    wip_index: &Path,
    head_tracked_files: &HashSet<&OsStr>,
    path: &OsStr,
) -> anyhow::Result<()> {
    if head_tracked_files.contains(path) {
        // If it was already committed in HEAD, restore HEAD's version in the WIP index
        // rather than deleting it, so tracked files aren't wiped out on restore.
        // `reset` only touches the WIP index; `checkout` would overwrite the worktree.
        let args = ["reset", "-q", "HEAD", "--"].map(OsStr::new);
        wip_index_output(git, wip_index, &[&args[..], &[path]].concat()).await?;
    } else {
        // `--sparse`: the path may lie outside the sparse patterns (a hidden path)
        let args = ["rm", "--cached", "--sparse", "--quiet", "--"].map(OsStr::new);
        wip_index_output(git, wip_index, &[&args[..], &[path]].concat()).await?;
    }
    Ok(())
}

/// Clean work tree: deletes the remote WIP branch if it exists. The report is `None` if there was
/// nothing to delete or the deletion failed.
async fn delete_remote_wip(git: &Git, workspace: &Workspace) -> anyhow::Result<Option<PushReport>> {
    if !ref_exists(git, &format!("origin/{}", workspace.wip_branch)).await {
        return Ok(None);
    }
    let del_out = git
        .push("origin", &format!(":{}", workspace.wip_branch), Force::No)
        .await?;
    Ok(del_out.status.success().then(|| PushReport {
        target: PushTarget::WipBranch as i32,
        r#ref: format!("refs/heads/{}", workspace.wip_branch),
        sha: String::new(),
        status: PushStatus::Deleted as i32,
        commits_ahead_of_base: 0,
        detail: "Deleted remote WIP branch (clean worktree)".to_string(),
    }))
}

/// Commits `tree` on top of HEAD and force-pushes it to the WIP branch.
async fn push_wip_snapshot(
    git: &Git,
    params: &FinalizeParams<'_>,
    wip_index: &Path,
    tree: &str,
) -> anyhow::Result<PushReport> {
    let wip_branch = &params.workspace.wip_branch;
    let commit_msg = format!("WIP snapshot, run {}", params.run_id);
    let commit_sha = run_with_wip_index(
        git,
        wip_index,
        &["commit-tree", tree, "-p", "HEAD", "-m", &commit_msg],
    )
    .await?;
    let refspec = format!("{commit_sha}:refs/heads/{wip_branch}");
    let push_out = git.push("origin", &refspec, Force::Always).await?;
    let (status, detail) = if push_out.status.success() {
        (PushStatus::Ok, "Pushed WIP snapshot".to_string())
    } else {
        rejected_push(&push_out)
    };
    Ok(PushReport {
        target: PushTarget::WipBranch as i32,
        r#ref: format!("refs/heads/{wip_branch}"),
        sha: commit_sha,
        status: status as i32,
        commits_ahead_of_base: 0,
        detail,
    })
}
