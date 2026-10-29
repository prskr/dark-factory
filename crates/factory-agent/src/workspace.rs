//! Workspace lifecycle management for Dark Factory tasks.
//!
//! Follows `design/agent-run.md` §6 and `specs/workspace-lifecycle/spec.md`.
use std::path::Path;

use df_proto::agent::v1::{
    PushReport, PushStatus, PushTarget, RunOutcome, RunReport, Workspace, WorkspaceState,
};
use df_proto::transcript::v1::{Notice, NoticeLevel};

use crate::git::{
    DEFAULT_COMMIT_AUTHOR_EMAIL, DEFAULT_COMMIT_AUTHOR_NAME, Git, GitConfigParams,
    rewrite_git_config,
};

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

/// Builds the runtime-owned git config parameters for a workspace.
fn config_params<'a>(
    workspace: &'a Workspace,
    hooks_path: &'a Path,
    credential_helper: &'a str,
) -> GitConfigParams<'a> {
    let identity = workspace.commit_identity.as_ref();
    GitConfigParams {
        repo_url: &workspace.repo_url,
        commit_author_name: identity.map_or(DEFAULT_COMMIT_AUTHOR_NAME, |c| c.name.as_str()),
        commit_author_email: identity.map_or(DEFAULT_COMMIT_AUTHOR_EMAIL, |c| c.email.as_str()),
        hooks_path,
        credential_helper,
        extra_config: &workspace.git_config,
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
    credential_helper: &str,
) -> anyhow::Result<PrepareResult> {
    let config_params = config_params(workspace, hooks_path, credential_helper);
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
            push_failed: report.status() == PushStatus::Failed,
            push_report: Some(report),
            ..Self::clean(head_sha)
        }
    }
}

/// True if `git rev-parse --verify <rev>` succeeds.
async fn ref_exists(git: &Git, rev: &str) -> bool {
    git.run_cmd(&["rev-parse", "--verify", rev])
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
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

    // blobless clone with --no-checkout run in parent_dir
    let clone_out = git
        .run_cmd_in(
            parent_dir,
            &[
                "clone",
                "--filter=blob:none",
                "--no-checkout",
                &workspace.repo_url,
                repo_dir_str,
            ],
        )
        .await?;
    if !clone_out.status.success() {
        let stderr = String::from_utf8_lossy(&clone_out.stderr);
        anyhow::bail!("git clone failed: {}", stderr.trim());
    }

    // Write runtime-owned .git/config and remove config.worktree
    rewrite_git_config(repo_dir, config_params)?;

    // Start from origin/<task branch> if the remote has it, else from the base branch
    let task_remote_ref = format!("origin/{}", workspace.task_branch);
    let start_point = if ref_exists(git, &task_remote_ref).await {
        task_remote_ref
    } else {
        format!("origin/{}", workspace.base_branch)
    };
    git.git_output(&["checkout", "-B", &workspace.task_branch, &start_point])
        .await?;

    restore_wip_snapshot(git, workspace).await?;

    let head_sha = git.current_head().await.unwrap_or_default();
    Ok(PrepareResult::clean(head_sha))
}

/// Restores the files of `origin/<wip branch>` into the work tree if that branch exists.
async fn restore_wip_snapshot(git: &Git, workspace: &Workspace) -> anyhow::Result<()> {
    let wip_remote_ref = format!("origin/{}", workspace.wip_branch);
    if !ref_exists(git, &wip_remote_ref).await {
        return Ok(());
    }
    // Failing here must fail prepare: otherwise the tree looks clean and finalize deletes
    // the remote WIP branch, the only copy of the previous run's uncommitted work.
    let restore_out = git
        .run_cmd(&[
            "restore",
            &format!("--source={wip_remote_ref}"),
            "--worktree",
            "--",
            ".",
        ])
        .await?;
    if !restore_out.status.success() {
        anyhow::bail!(
            "restoring WIP snapshot {wip_remote_ref} failed: {}",
            String::from_utf8_lossy(&restore_out.stderr).trim()
        );
    }
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

    // Fetch base and task branch from remote (Finding 17)
    let fetch_out = git.run_cmd(&["fetch", "origin"]).await?;
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
        let start_point = if has_remote_task {
            task_remote_ref.clone()
        } else {
            format!("origin/{}", workspace.base_branch)
        };
        git.git_output(&["checkout", "-B", &workspace.task_branch, &start_point])
            .await?;
    }

    if has_remote_task {
        sync_with_remote_task_branch(git, workspace, &task_remote_ref).await
    } else {
        push_new_task_branch(git, workspace).await
    }
}

/// The remote has the task branch: push local-only commits, fast-forward, or detect divergence.
async fn sync_with_remote_task_branch(
    git: &Git,
    workspace: &Workspace,
    task_remote_ref: &str,
) -> anyhow::Result<PrepareResult> {
    let local_sha = git.current_head().await?;
    let remote_sha = git.git_output(&["rev-parse", task_remote_ref]).await?;
    if local_sha == remote_sha {
        // Synchronized, nothing to do
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
        )
        .await?;
        Ok(PrepareResult::pushed(local_sha, report))
    } else if local_ahead == 0 && remote_ahead > 0 {
        // Remote is ahead: fast-forward merge
        git.git_output(&["merge", "--ff-only", task_remote_ref])
            .await?;
        let head_sha = git.current_head().await.unwrap_or_default();
        Ok(PrepareResult::clean(head_sha))
    } else {
        // Diverged: local and remote both have commits the other lacks
        // Leave both untouched, emit workspace.diverged notice
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
) -> anyhow::Result<PushReport> {
    let refspec = format!("{}:{}", workspace.task_branch, workspace.task_branch);
    let push_output = git.push("origin", &refspec, false).await?;
    let status = if push_output.status.success() {
        PushStatus::Ok
    } else {
        PushStatus::Failed
    };
    Ok(PushReport {
        target: PushTarget::TaskBranch as i32,
        r#ref: format!("refs/heads/{}", workspace.task_branch),
        sha: local_sha.to_string(),
        status: status as i32,
        commits_ahead_of_base,
        detail: detail.to_string(),
    })
}

/// Returns true if `path_str` matches the secret-file denylist.
///
/// Follows `specs/workspace-lifecycle/spec.md`:
/// "drop paths matching the secret-file denylist (.env*, *.pem, *.key, id_*, ...)"
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
    let push_out = git.push("origin", &refspec, false).await?;

    if push_out.status.success() {
        *last_pushed_sha = Some(current_sha.clone());
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
        let stderr = String::from_utf8_lossy(&push_out.stderr);
        let stdout = String::from_utf8_lossy(&push_out.stdout);
        let combined = format!("{stdout}\n{stderr}");
        let status = if combined.contains("non-fast-forward")
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
        let report = PushReport {
            target: PushTarget::TaskBranch as i32,
            r#ref: format!("refs/heads/{}", workspace.task_branch),
            sha: current_sha,
            status: status as i32,
            commits_ahead_of_base: 0,
            detail: combined.trim().to_string(),
        };
        Ok(Some(report))
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
pub fn compute_dir_size(dir: &std::path::Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(ft) = entry.file_type() {
                if ft.is_symlink() {
                    continue;
                } else if ft.is_dir() {
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

async fn run_with_wip_index(
    git: &Git,
    wip_index: &std::path::Path,
    args: &[&str],
) -> anyhow::Result<String> {
    let mut cmd = git.build_command();
    cmd.env("GIT_INDEX_FILE", wip_index);
    // File names like `prod[1].key` are paths, not globs
    cmd.env("GIT_LITERAL_PATHSPECS", "1");
    cmd.args(args);
    let out = cmd
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("git {} failed: {e}", args.join(" ")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
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
    pub credential_helper: &'a str,
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
    let config_params = config_params(workspace, params.hooks_path, params.credential_helper);
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
        let _ = std::fs::remove_file(&wip_index);
    }
    let wip = snapshot_wip(git, params, &wip_index).await?;
    push_reports.extend(wip.push_reports);
    if wip_index.exists() {
        let _ = std::fs::remove_file(&wip_index);
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
    let tree = write_wip_tree(git, wip_index).await?;
    let head_tree = git.git_output(&["rev-parse", "HEAD^{tree}"]).await?;
    if tree == head_tree {
        wip.push_reports
            .extend(delete_remote_wip(git, params.workspace).await?);
        return Ok(wip);
    }

    // Uncommitted work: check size cap
    let mut total_wip_bytes: u64 = 0;
    let diff_files =
        run_with_wip_index(git, wip_index, &["diff", "-z", "--name-only", "HEAD"]).await?;
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
async fn write_wip_tree(git: &Git, wip_index: &Path) -> anyhow::Result<String> {
    // Read current HEAD into separate index
    let _ = run_with_wip_index(git, wip_index, &["read-tree", "HEAD"]).await?;
    // Add all modifications and untracked files respecting .gitignore
    let _ = run_with_wip_index(git, wip_index, &["add", "-A"]).await?;

    // Get list of tracked files in HEAD so we don't drop files already committed in HEAD (Finding 6).
    // `-z` keeps names unquoted; non-ASCII names would otherwise come back as `"cl\303\251.pem"`.
    let head_files_out = git
        .git_output(&["ls-tree", "-r", "-z", "--name-only", "HEAD"])
        .await
        .unwrap_or_default();
    let head_tracked_files: std::collections::HashSet<&str> = head_files_out
        .split('\0')
        .filter(|l| !l.is_empty())
        .collect();

    // Drop secret files from wip.index
    let ls_files = run_with_wip_index(git, wip_index, &["ls-files", "-z"]).await?;
    for f in ls_files.split('\0').filter(|f| !f.is_empty()) {
        if !is_secret_file(f) {
            continue;
        }
        if head_tracked_files.contains(f) {
            // If it was already committed in HEAD, restore HEAD's version in the WIP index
            // rather than deleting it, so tracked files aren't wiped out on restore.
            // `reset` only touches the WIP index; `checkout` would overwrite the worktree.
            run_with_wip_index(git, wip_index, &["reset", "-q", "HEAD", "--", f]).await?;
        } else {
            // Untracked secret file: drop completely from the WIP index. A failure must abort
            // the snapshot, otherwise the secret would be pushed.
            run_with_wip_index(git, wip_index, &["rm", "--cached", "--quiet", "--", f]).await?;
        }
    }

    run_with_wip_index(git, wip_index, &["write-tree"]).await
}

/// Clean work tree: deletes the remote WIP branch if it exists. The report is `None` if there was
/// nothing to delete or the deletion failed.
async fn delete_remote_wip(git: &Git, workspace: &Workspace) -> anyhow::Result<Option<PushReport>> {
    if !ref_exists(git, &format!("origin/{}", workspace.wip_branch)).await {
        return Ok(None);
    }
    let del_out = git
        .push("origin", &format!(":{}", workspace.wip_branch), false)
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
    let push_out = git.push("origin", &refspec, true).await?;
    let (status, detail) = if push_out.status.success() {
        (PushStatus::Ok, "Pushed WIP snapshot".to_string())
    } else {
        (
            PushStatus::Failed,
            String::from_utf8_lossy(&push_out.stderr).trim().to_string(),
        )
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
