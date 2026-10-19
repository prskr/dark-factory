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

    let git_dir = repo_dir.join(".git");

    if !git_dir.exists() {
        // --- CASE 1: Fresh workspace ---
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
        rewrite_git_config(repo_dir, &config_params)?;

        // Determine if remote origin has factory/task/<key>
        let task_remote_ref = format!("origin/{}", workspace.task_branch);
        let has_task_branch = git
            .run_cmd(&["rev-parse", "--verify", &task_remote_ref])
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        if has_task_branch {
            git.git_output(&["checkout", "-B", &workspace.task_branch, &task_remote_ref])
                .await?;
        } else {
            let base_remote_ref = format!("origin/{}", workspace.base_branch);
            git.git_output(&["checkout", "-B", &workspace.task_branch, &base_remote_ref])
                .await?;
        }

        // Restore files from origin/wip/<key> if it exists on remote
        let wip_remote_ref = format!("origin/{}", workspace.wip_branch);
        let has_wip = git
            .run_cmd(&["rev-parse", "--verify", &wip_remote_ref])
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        if has_wip {
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
        }

        let head_sha = git.current_head().await.unwrap_or_default();
        Ok(PrepareResult {
            diverged: false,
            head_sha,
            push_report: None,
            push_failed: false,
            notices: Vec::new(),
        })
    } else {
        // --- CASE 2: Existing workspace ---
        // 1. Sanitize .git/config
        rewrite_git_config(repo_dir, &config_params)?;

        // 2. Fetch base and task branch from remote (Finding 17)
        let fetch_out = git.run_cmd(&["fetch", "origin"]).await?;
        if !fetch_out.status.success() {
            let stderr = String::from_utf8_lossy(&fetch_out.stderr);
            anyhow::bail!(
                "git fetch origin failed during workspace prepare: {}",
                stderr.trim()
            );
        }

        let task_remote_ref = format!("origin/{}", workspace.task_branch);
        let has_remote_task = git
            .run_cmd(&["rev-parse", "--verify", &task_remote_ref])
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

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
            let local_sha = git.current_head().await?;
            let remote_sha = git.git_output(&["rev-parse", &task_remote_ref]).await?;

            if local_sha == remote_sha {
                // Synchronized, nothing to do
                Ok(PrepareResult {
                    diverged: false,
                    head_sha: local_sha,
                    push_report: None,
                    push_failed: false,
                    notices: Vec::new(),
                })
            } else {
                let local_ahead = git
                    .commits_ahead(&task_remote_ref, &local_sha)
                    .await
                    .unwrap_or(0);
                let remote_ahead = git
                    .commits_ahead(&local_sha, &task_remote_ref)
                    .await
                    .unwrap_or(0);

                if remote_ahead == 0 && local_ahead > 0 {
                    // Local has unpushed commits from a killed run: push them first
                    let refspec = format!("{}:{}", workspace.task_branch, workspace.task_branch);
                    let push_output = git.push("origin", &refspec, false).await?;
                    let status = if push_output.status.success() {
                        PushStatus::Ok
                    } else {
                        PushStatus::Failed
                    };

                    let base_remote_ref = format!("origin/{}", workspace.base_branch);
                    let ahead_of_base = git
                        .commits_ahead(&base_remote_ref, &local_sha)
                        .await
                        .unwrap_or(local_ahead);

                    let report = PushReport {
                        target: PushTarget::TaskBranch as i32,
                        r#ref: format!("refs/heads/{}", workspace.task_branch),
                        sha: local_sha.clone(),
                        status: status as i32,
                        commits_ahead_of_base: ahead_of_base,
                        detail: "Pushed unpushed commits during workspace prepare".to_string(),
                    };

                    Ok(PrepareResult {
                        diverged: false,
                        head_sha: local_sha,
                        push_report: Some(report),
                        push_failed: status == PushStatus::Failed,
                        notices: Vec::new(),
                    })
                } else if local_ahead == 0 && remote_ahead > 0 {
                    // Remote is ahead: fast-forward merge
                    git.git_output(&["merge", "--ff-only", &task_remote_ref])
                        .await?;
                    let head_sha = git.current_head().await.unwrap_or_default();
                    Ok(PrepareResult {
                        diverged: false,
                        head_sha,
                        push_report: None,
                        push_failed: false,
                        notices: Vec::new(),
                    })
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
                        head_sha: local_sha,
                        push_report: None,
                        push_failed: false,
                        notices: vec![notice],
                    })
                }
            }
        } else {
            // Remote task branch does not exist yet; check if local has commits ahead of base
            let base_remote_ref = format!("origin/{}", workspace.base_branch);
            let local_sha = git.current_head().await?;
            let local_ahead = git
                .commits_ahead(&base_remote_ref, &local_sha)
                .await
                .unwrap_or(0);

            if local_ahead > 0 {
                // Push local commits to remote task branch
                let refspec = format!("{}:{}", workspace.task_branch, workspace.task_branch);
                let push_output = git.push("origin", &refspec, false).await?;
                let status = if push_output.status.success() {
                    PushStatus::Ok
                } else {
                    PushStatus::Failed
                };

                let report = PushReport {
                    target: PushTarget::TaskBranch as i32,
                    r#ref: format!("refs/heads/{}", workspace.task_branch),
                    sha: local_sha.clone(),
                    status: status as i32,
                    commits_ahead_of_base: local_ahead,
                    detail: "Pushed task branch during workspace prepare".to_string(),
                };

                Ok(PrepareResult {
                    diverged: false,
                    head_sha: local_sha,
                    push_report: Some(report),
                    push_failed: status == PushStatus::Failed,
                    notices: Vec::new(),
                })
            } else {
                Ok(PrepareResult {
                    diverged: false,
                    head_sha: local_sha,
                    push_report: None,
                    push_failed: false,
                    notices: Vec::new(),
                })
            }
        }
    }
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
    let mut notices = Vec::new();

    let workspace = params.workspace;
    let repo_dir = params.repo_dir;
    let runtime_dir = params.runtime_dir;
    let run_id = params.run_id;
    let outcome = params.outcome;
    let detail = params.detail;
    let harness_session_id = params.harness_session_id;
    let wip_size_cap_bytes = params.wip_size_cap_bytes;

    let config_params = config_params(workspace, params.hooks_path, params.credential_helper);

    // 1. Rewrite .git/config
    rewrite_git_config(repo_dir, &config_params)?;

    // 2. Push task branch if needed
    if let Some(report) = push_after_turn(git, workspace, last_pushed_sha).await? {
        push_reports.push(report);
    }

    let task_branch_sha = git.current_head().await.unwrap_or_default();
    let task_branch_pushed = last_pushed_sha.as_deref() == Some(&task_branch_sha);

    // 3. WIP snapshot via separate index
    let wip_index = runtime_dir.join("wip.index");
    if wip_index.exists() {
        let _ = std::fs::remove_file(&wip_index);
    }

    let mut wip_sha = String::new();

    // Read current HEAD into separate index
    let _ = run_with_wip_index(git, &wip_index, &["read-tree", "HEAD"]).await?;
    // Add all modifications and untracked files respecting .gitignore
    let _ = run_with_wip_index(git, &wip_index, &["add", "-A"]).await?;

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
    let ls_files = run_with_wip_index(git, &wip_index, &["ls-files", "-z"]).await?;
    for f in ls_files.split('\0').filter(|f| !f.is_empty()) {
        if !is_secret_file(f) {
            continue;
        }
        if head_tracked_files.contains(f) {
            // If it was already committed in HEAD, restore HEAD's version in the WIP index
            // rather than deleting it, so tracked files aren't wiped out on restore.
            // `reset` only touches the WIP index; `checkout` would overwrite the worktree.
            run_with_wip_index(git, &wip_index, &["reset", "-q", "HEAD", "--", f]).await?;
        } else {
            // Untracked secret file: drop completely from the WIP index. A failure must abort
            // the snapshot, otherwise the secret would be pushed.
            run_with_wip_index(git, &wip_index, &["rm", "--cached", "--quiet", "--", f]).await?;
        }
    }

    let tree = run_with_wip_index(git, &wip_index, &["write-tree"]).await?;
    let head_tree = git.git_output(&["rev-parse", "HEAD^{tree}"]).await?;

    if tree == head_tree {
        // Clean worktree: delete remote factory/wip/<key> if it exists
        let wip_remote_ref = format!("origin/{}", workspace.wip_branch);
        let has_remote_wip = git
            .run_cmd(&["rev-parse", "--verify", &wip_remote_ref])
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        if has_remote_wip {
            let del_out = git
                .push("origin", &format!(":{}", workspace.wip_branch), false)
                .await?;
            if del_out.status.success() {
                push_reports.push(PushReport {
                    target: PushTarget::WipBranch as i32,
                    r#ref: format!("refs/heads/{}", workspace.wip_branch),
                    sha: String::new(),
                    status: PushStatus::Deleted as i32,
                    commits_ahead_of_base: 0,
                    detail: "Deleted remote WIP branch (clean worktree)".to_string(),
                });
            }
        }
    } else {
        // Uncommitted work: check size cap
        let mut total_wip_bytes: u64 = 0;
        let diff_files =
            run_with_wip_index(git, &wip_index, &["diff", "-z", "--name-only", "HEAD"]).await?;
        for f in diff_files.split('\0').filter(|f| !f.is_empty()) {
            let p = repo_dir.join(f);
            if let Ok(meta) = p.metadata() {
                total_wip_bytes += meta.len();
            }
        }

        if total_wip_bytes > wip_size_cap_bytes {
            notices.push(Notice {
                level: NoticeLevel::Warning as i32,
                code: "wip.size_cap_exceeded".to_string(),
                message: format!(
                    "WIP snapshot size {total_wip_bytes} exceeds cap {wip_size_cap_bytes}"
                ),
            });
        } else {
            // Commit tree and force push to factory/wip/<key>
            let commit_msg = format!("WIP snapshot, run {run_id}");
            let commit_sha = run_with_wip_index(
                git,
                &wip_index,
                &["commit-tree", &tree, "-p", "HEAD", "-m", &commit_msg],
            )
            .await?;
            let refspec = format!("{commit_sha}:refs/heads/{}", workspace.wip_branch);
            let push_out = git.push("origin", &refspec, true).await?;
            if push_out.status.success() {
                wip_sha = commit_sha.clone();
                push_reports.push(PushReport {
                    target: PushTarget::WipBranch as i32,
                    r#ref: format!("refs/heads/{}", workspace.wip_branch),
                    sha: commit_sha,
                    status: PushStatus::Ok as i32,
                    commits_ahead_of_base: 0,
                    detail: "Pushed WIP snapshot".to_string(),
                });
            } else {
                push_reports.push(PushReport {
                    target: PushTarget::WipBranch as i32,
                    r#ref: format!("refs/heads/{}", workspace.wip_branch),
                    sha: commit_sha,
                    status: PushStatus::Failed as i32,
                    commits_ahead_of_base: 0,
                    detail: String::from_utf8_lossy(&push_out.stderr).trim().to_string(),
                });
            }
        }
    }

    if wip_index.exists() {
        let _ = std::fs::remove_file(&wip_index);
    }

    let workspace_size = compute_dir_size(repo_dir);

    let run_report = RunReport {
        outcome: outcome as i32,
        detail: detail.to_string(),
        retry_after: None,
        harness_session_id: harness_session_id.to_string(),
        workspace: Some(WorkspaceState {
            task_branch_sha,
            wip_sha,
            task_branch_pushed,
            diverged: params.diverged,
            size_bytes: workspace_size,
        }),
    };

    Ok(FinalizeResult {
        run_report,
        push_reports,
        notices,
    })
}
