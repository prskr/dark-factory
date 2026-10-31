//! Workspace push, finalize, secret files and token handling.

#![expect(
    clippy::too_many_lines,
    reason = "scenario tests set up and check a whole run"
)]

use super::{clone_for_workspace_test, setup_test_remote_repo};

#[test]
fn test_is_secret_file_denylist() {
    use crate::workspace::is_secret_file;

    // Denied files
    assert!(is_secret_file(".env"));
    assert!(is_secret_file(".env.local"));
    assert!(is_secret_file(".env.production"));
    assert!(is_secret_file("config/.env"));
    assert!(is_secret_file("id_rsa"));
    assert!(is_secret_file("id_ed25519"));
    assert!(is_secret_file(".ssh/id_rsa"));
    assert!(is_secret_file("cert.pem"));
    assert!(is_secret_file("server.key"));
    assert!(is_secret_file("bundle.pfx"));
    assert!(is_secret_file("client.p12"));
    assert!(is_secret_file("secret.pkcs12"));

    // Allowed files
    assert!(!is_secret_file("src/main.rs"));
    assert!(!is_secret_file("README.md"));
    assert!(!is_secret_file("package.json"));
    assert!(!is_secret_file(".gitignore"));
    assert!(!is_secret_file("Cargo.toml"));
    assert!(!is_secret_file("environment.rs"));
    assert!(!is_secret_file("keyboard.js"));
}

#[tokio::test]
async fn test_scenario_push_after_turn_with_commits_and_rejection() {
    use crate::git::Git;
    use crate::workspace::push_after_turn;
    use df_proto::agent::v1::{PushStatus, PushTarget, Workspace};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("local");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // 1. Initialize bare remote repository
    tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await
        .unwrap();

    // Make initial commit on main and factory/task/DF-1
    let seed_dir = temp_dir.path().join("seed");
    tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await
        .unwrap();

    std::fs::write(seed_dir.join("main.txt"), "Main").expect("write main");
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "main"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await
        .unwrap();

    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/task/DF-1"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await
        .unwrap();

    // Clone locally
    tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            local_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);
    git.run_cmd(&["config", "commit.gpgsign", "false"])
        .await
        .unwrap();
    git.run_cmd(&["config", "user.name", "Bot"]).await.unwrap();
    git.run_cmd(&["config", "user.email", "bot@example.com"])
        .await
        .unwrap();
    git.run_cmd(&["checkout", "factory/task/DF-1"])
        .await
        .unwrap();

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    let mut last_pushed_sha = Some(git.current_head().await.unwrap());

    // 2. Turn 1 produces 2 new commits
    std::fs::write(local_dir.join("c1.txt"), "1").unwrap();
    git.run_cmd(&["add", "c1.txt"]).await.unwrap();
    git.run_cmd(&["commit", "-m", "commit 1"]).await.unwrap();

    std::fs::write(local_dir.join("c2.txt"), "2").unwrap();
    git.run_cmd(&["add", "c2.txt"]).await.unwrap();
    git.run_cmd(&["commit", "-m", "commit 2"]).await.unwrap();

    let res = push_after_turn(&git, &workspace, &mut last_pushed_sha)
        .await
        .expect("push after turn");

    assert!(res.is_some(), "turn with commits must emit PushReport");
    let report = res.unwrap();
    assert_eq!(report.target, PushTarget::TaskBranch as i32);
    assert_eq!(report.status, PushStatus::Ok as i32);
    assert_eq!(report.commits_ahead_of_base, 2);

    // Calling again without new commits returns None
    let res2 = push_after_turn(&git, &workspace, &mut last_pushed_sha)
        .await
        .expect("push after turn no commits");
    assert!(res2.is_none());

    // 3. Remote moves ahead non-fast-forward:
    // Seed repo pushes a commit
    std::fs::write(seed_dir.join("seed_change.txt"), "seed").unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "seed_change.txt"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "seed pushed"])
        .output()
        .await
        .unwrap();
    let seed_push = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "--force", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await
        .expect("seed push");
    assert!(seed_push.status.success(), "seed force push must succeed");

    // Local makes another commit
    std::fs::write(local_dir.join("c3.txt"), "3").unwrap();
    git.run_cmd(&["add", "c3.txt"]).await.unwrap();
    git.run_cmd(&["commit", "-m", "commit 3"]).await.unwrap();

    // Push after turn must be rejected as non-fast-forward
    let res3 = push_after_turn(&git, &workspace, &mut last_pushed_sha)
        .await
        .expect("push after turn rejected");

    assert!(res3.is_some());
    let report3 = res3.unwrap();
    assert_eq!(
        report3.status,
        PushStatus::RejectedNonFastForward as i32,
        "push must be rejected as non-fast-forward"
    );
}

#[tokio::test]
async fn test_scenario_finalize_uncommitted_work_and_secret_exclusion() {
    use crate::git::Git;
    use crate::workspace::finalize_workspace;
    use df_proto::agent::v1::{RunOutcome, Workspace};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("local");
    let runtime_dir = temp_dir.path().join("runtime");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // Initialize bare remote repo with main and factory/task/DF-1
    tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await
        .unwrap();

    let seed_dir = temp_dir.path().join("seed");
    tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await
        .unwrap();

    std::fs::write(seed_dir.join("main.txt"), "Initial main").unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "init"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/task/DF-1"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await
        .unwrap();

    // Clone into local_dir
    tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            local_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);
    git.run_cmd(&["config", "commit.gpgsign", "false"])
        .await
        .unwrap();
    git.run_cmd(&["config", "user.name", "Bot"]).await.unwrap();
    git.run_cmd(&["config", "user.email", "bot@example.com"])
        .await
        .unwrap();
    git.run_cmd(&["checkout", "factory/task/DF-1"])
        .await
        .unwrap();

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    // 1. Create uncommitted modifications, an untracked normal file, and untracked secret files (.env, id_rsa)
    std::fs::write(local_dir.join("main.txt"), "Modified in turn").unwrap();
    std::fs::write(local_dir.join("feature.txt"), "New feature uncommitted").unwrap();
    std::fs::write(local_dir.join(".env"), "SECRET_API_KEY=xyz").unwrap();
    std::fs::write(local_dir.join("id_rsa"), "-----BEGIN RSA PRIVATE KEY-----").unwrap();

    let status_before = git.git_status().await.unwrap();

    let mut last_pushed_sha = Some(git.current_head().await.unwrap());

    // 2. Finalize workspace
    let finalize_params = crate::workspace::FinalizeParams {
        repo_dir: &local_dir,
        runtime_dir: &runtime_dir,
        workspace: &workspace,
        run_id: "run-101",
        outcome: RunOutcome::Completed,
        detail: "Task completed",
        harness_session_id: "session-1",
        hooks_path: &empty_hooks,
        ssl_ca_info: None,
        wip_size_cap_bytes: 10 * 1024 * 1024,
        diverged: false,
    };
    let res = finalize_workspace(&git, &finalize_params, &mut last_pushed_sha)
        .await
        .expect("finalize");

    // RunReport workspace check
    let state = res.run_report.workspace.expect("workspace state");
    assert!(
        !state.wip_sha.is_empty(),
        "wip_sha must be present when worktree had changes"
    );

    // 3. Verify git status in workspace is completely UNCHANGED
    let status_after = git.git_status().await.unwrap();
    assert_eq!(
        status_before, status_after,
        "git status in workspace must not be modified by finalize"
    );

    // 4. Verify contents of the WIP commit tree
    let wip_files = git
        .git_output(&["ls-tree", "-r", "--name-only", &state.wip_sha])
        .await
        .unwrap();
    let file_list: Vec<&str> = wip_files.lines().map(str::trim).collect();

    assert!(file_list.contains(&"main.txt"));
    assert!(file_list.contains(&"feature.txt"));
    assert!(
        !file_list.contains(&".env"),
        ".env MUST be excluded from WIP snapshot"
    );
    assert!(
        !file_list.contains(&"id_rsa"),
        "id_rsa MUST be excluded from WIP snapshot"
    );
}

#[tokio::test]
async fn test_scenario_finalize_clean_worktree_deletes_remote_wip() {
    use crate::git::Git;
    use crate::workspace::finalize_workspace;
    use df_proto::agent::v1::{PushStatus, PushTarget, RunOutcome, Workspace};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("local");
    let runtime_dir = temp_dir.path().join("runtime");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // Bare remote repo
    tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await
        .unwrap();

    let seed_dir = temp_dir.path().join("seed");
    tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await
        .unwrap();

    std::fs::write(seed_dir.join("main.txt"), "Main").unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "init"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/task/DF-1"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await
        .unwrap();

    // Push an existing factory/wip/DF-1 branch to remote
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/wip/DF-1"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/wip/DF-1"])
        .output()
        .await
        .unwrap();

    // Clone into local_dir
    tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            local_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);
    git.run_cmd(&["config", "commit.gpgsign", "false"])
        .await
        .unwrap();
    git.run_cmd(&["config", "user.name", "Bot"]).await.unwrap();
    git.run_cmd(&["config", "user.email", "bot@example.com"])
        .await
        .unwrap();
    git.run_cmd(&["checkout", "factory/task/DF-1"])
        .await
        .unwrap();

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    let mut last_pushed_sha = Some(git.current_head().await.unwrap());

    // Worktree is clean!
    assert!(git.is_clean().await.unwrap());

    let finalize_params = crate::workspace::FinalizeParams {
        repo_dir: &local_dir,
        runtime_dir: &runtime_dir,
        workspace: &workspace,
        run_id: "run-102",
        outcome: RunOutcome::Completed,
        detail: "Clean finish",
        harness_session_id: "session-2",
        hooks_path: &empty_hooks,
        ssl_ca_info: None,
        wip_size_cap_bytes: 10 * 1024 * 1024,
        diverged: false,
    };
    let res = finalize_workspace(&git, &finalize_params, &mut last_pushed_sha)
        .await
        .expect("finalize clean");

    let state = res.run_report.workspace.expect("state");
    assert!(state.wip_sha.is_empty(), "clean worktree has no wip_sha");

    // Deleted report emitted
    let deleted_report = res.push_reports.iter().find(|r| {
        r.target == PushTarget::WipBranch as i32 && r.status == PushStatus::Deleted as i32
    });
    assert!(
        deleted_report.is_some(),
        "must emit PushReport with DELETED"
    );

    // Remote WIP branch should be gone
    let has_remote_wip = git
        .run_cmd(&["rev-parse", "--verify", "origin/factory/wip/DF-1"])
        .await
        .is_ok_and(|o| o.status.success());
    assert!(
        !has_remote_wip,
        "remote WIP branch must be deleted on clean worktree"
    );
}

#[tokio::test]
async fn test_scenario_finalize_wip_size_cap_exceeded() {
    use crate::git::Git;
    use crate::workspace::finalize_workspace;
    use df_proto::agent::v1::{RunOutcome, Workspace};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("local");
    let runtime_dir = temp_dir.path().join("runtime");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await
        .unwrap();

    let seed_dir = temp_dir.path().join("seed");
    tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await
        .unwrap();

    std::fs::write(seed_dir.join("main.txt"), "Main").unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "init"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/task/DF-1"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await
        .unwrap();

    tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            local_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .unwrap();
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);
    git.run_cmd(&["config", "commit.gpgsign", "false"])
        .await
        .unwrap();
    git.run_cmd(&["config", "user.name", "Bot"]).await.unwrap();
    git.run_cmd(&["config", "user.email", "bot@example.com"])
        .await
        .unwrap();
    git.run_cmd(&["checkout", "factory/task/DF-1"])
        .await
        .unwrap();

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    // Create a large uncommitted file exceeding size cap of 500 bytes
    let large_data = vec![b'x'; 5000];
    std::fs::write(local_dir.join("large_file.bin"), large_data).unwrap();

    let mut last_pushed_sha = Some(git.current_head().await.unwrap());

    let finalize_params = crate::workspace::FinalizeParams {
        repo_dir: &local_dir,
        runtime_dir: &runtime_dir,
        workspace: &workspace,
        run_id: "run-103",
        outcome: RunOutcome::Completed,
        detail: "Size cap test",
        harness_session_id: "session-3",
        hooks_path: &empty_hooks,
        ssl_ca_info: None,
        wip_size_cap_bytes: 500, // 500 bytes cap!
        diverged: false,
    };
    let res = finalize_workspace(&git, &finalize_params, &mut last_pushed_sha)
        .await
        .expect("finalize size cap");

    let state = res.run_report.workspace.expect("state");
    assert!(
        state.wip_sha.is_empty(),
        "wip_sha should be empty when size cap is exceeded"
    );
    assert_eq!(res.notices.len(), 1);
    assert_eq!(res.notices[0].code, "wip.size_cap_exceeded");
}

fn scan_dir_for_secret(dir: &std::path::Path, secret: &str) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_dir_for_secret(&path, secret);
            } else if path.is_file() {
                let content = std::fs::read(&path).unwrap_or_default();
                let content_str = String::from_utf8_lossy(&content);
                assert!(
                    !content_str.contains(secret),
                    "Secret token found in file: {}",
                    path.display()
                );
            }
        }
    }
}

#[tokio::test]
async fn test_scenario_token_stays_off_the_volume() {
    use crate::harness::prepare_harness_environment;
    use df_proto::agent::v1::{AcpHarness, Assign, Harness, HarnessCredentials, harness::Kind};
    use std::collections::HashMap;

    let temp = tempfile::tempdir().unwrap();
    let data_dir = temp.path().join("data");
    let home_dir = data_dir.join("home");
    let repo_dir = data_dir.join("repo");
    let runtime_dir = data_dir.join("runtime");

    std::fs::create_dir_all(&home_dir).unwrap();
    std::fs::create_dir_all(&repo_dir).unwrap();
    std::fs::create_dir_all(&runtime_dir).unwrap();

    let secret_token = "CLAUDE_SECRET_TOKEN_XYZ_1234567890_NEVER_PERSIST";
    let mut creds_env = HashMap::new();
    creds_env.insert(
        "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
        secret_token.to_string(),
    );
    let credentials = HarnessCredentials { env: creds_env };

    let mut acp_env = HashMap::new();
    acp_env.insert("FOO".to_string(), "bar".to_string());

    let assign = Assign {
        run_id: "run-token-test".to_string(),
        task: None,
        workspace: None,
        harness: Some(Harness {
            system_prompt: "You are Dark Factory Bot".to_string(),
            kind: Some(Kind::Acp(AcpHarness {
                command: vec!["fake-acp-agent".to_string()],
                env: acp_env,
                model: None,
                settings_json: r#"{"theme": "dark"}"#.to_string(),
            })),
        }),
        tools: vec![],
        budgets: None,
        continuation: None,
    };

    let env = prepare_harness_environment(&assign, Some(&credentials), &home_dir).unwrap();

    // Environment contains the credentials
    assert_eq!(
        env.get("CLAUDE_CODE_OAUTH_TOKEN").map(String::as_str),
        Some(secret_token)
    );
    assert_eq!(env.get("FOO").map(String::as_str), Some("bar"));

    // Settings written to /data/home/.claude/settings.json
    let settings_path = home_dir.join(".claude").join("settings.json");
    assert!(settings_path.exists());
    assert_eq!(
        std::fs::read_to_string(&settings_path).unwrap(),
        r#"{"theme": "dark"}"#
    );

    // System prompt written to /data/home/.claude/CLAUDE.md
    let claude_md_path = home_dir.join(".claude").join("CLAUDE.md");
    assert!(claude_md_path.exists());
    assert_eq!(
        std::fs::read_to_string(&claude_md_path).unwrap(),
        "You are Dark Factory Bot"
    );

    // Verify token NEVER lands on disk under /data

    scan_dir_for_secret(&data_dir, secret_token);
}

#[test]
fn test_is_secret_file_matches_ssh_keys_not_ordinary_files() {
    use crate::workspace::is_secret_file;

    // Ordinary source files must NOT be detected as secrets
    assert!(!is_secret_file("id_utils.rs"));
    assert!(!is_secret_file("src/id_generator.rs"));
    assert!(!is_secret_file("tests/id_test.rs"));

    // SSH private keys must be detected
    assert!(is_secret_file("id_rsa"));
    assert!(is_secret_file("id_ed25519"));
    assert!(is_secret_file("id_ecdsa"));
    assert!(is_secret_file("id_dsa"));
    assert!(is_secret_file("sub/id_rsa.bak"));
}

#[tokio::test]
async fn test_compute_dir_size_skips_symlinks() {
    use crate::workspace::compute_dir_size;

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    std::fs::write(root.join("file1.txt"), vec![b'a'; 100]).unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub").join("file2.txt"), vec![b'b'; 200]).unwrap();

    // Create a circular symlink pointing back to root
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root, root.join("sub").join("loop_link")).unwrap();
        std::os::unix::fs::symlink(root.join("file1.txt"), root.join("link_file.txt")).unwrap();
    }

    let size = compute_dir_size(root);
    // Size should only include actual files (100 + 200 = 300), and NOT follow circular symlink
    assert_eq!(size, 300);
}

#[tokio::test]
async fn test_wip_snapshot_keeps_tracked_secret_file_edits() {
    use crate::git::Git;
    use crate::workspace::{FinalizeParams, finalize_workspace};
    use df_proto::agent::v1::{RunOutcome, Workspace};

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());
    let local = temp.path().join("local");
    let runtime_dir = temp.path().join("runtime");
    let hooks = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::create_dir_all(&hooks).unwrap();
    clone_for_workspace_test(&upstream, &local);

    let git = Git::new(&local)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&hooks);
    std::fs::write(local.join(".env.example"), "KEY=\n").unwrap();
    git.git_output(&["add", ".env.example"]).await.unwrap();
    git.git_output(&["commit", "-m", "add env example"])
        .await
        .unwrap();

    // The agent edits the tracked secret-named file without committing
    std::fs::write(local.join(".env.example"), "KEY=edited\n").unwrap();
    std::fs::write(local.join("notes.txt"), "uncommitted\n").unwrap();
    let status_before = git.git_status().await.unwrap();

    let workspace = Workspace {
        repo_url: format!("file://{}", upstream.display()),
        base_branch: "main".to_string(),
        task_branch: "main".to_string(),
        wip_branch: "factory/wip/DF-3".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };
    let mut last_pushed_sha = Some(git.current_head().await.unwrap());
    let params = FinalizeParams {
        repo_dir: &local,
        runtime_dir: &runtime_dir,
        workspace: &workspace,
        run_id: "run-finalize-a",
        outcome: RunOutcome::Completed,
        detail: "",
        harness_session_id: "",
        hooks_path: &hooks,
        ssl_ca_info: None,
        wip_size_cap_bytes: 10 * 1024 * 1024,
        diverged: false,
    };
    let res = finalize_workspace(&git, &params, &mut last_pushed_sha)
        .await
        .expect("finalize");
    assert_ne!(res.run_report.workspace.unwrap().wip_sha, "");

    assert_eq!(
        std::fs::read_to_string(local.join(".env.example")).unwrap(),
        "KEY=edited\n",
        "finalize must not overwrite the agent's edits"
    );
    assert_eq!(git.git_status().await.unwrap(), status_before);
}

#[tokio::test]
async fn test_wip_snapshot_excludes_quoted_and_glob_like_secret_names() {
    use crate::git::Git;
    use crate::workspace::{FinalizeParams, finalize_workspace};
    use df_proto::agent::v1::{RunOutcome, Workspace};

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());
    let local = temp.path().join("local");
    let runtime_dir = temp.path().join("runtime");
    let hooks = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::create_dir_all(&hooks).unwrap();
    clone_for_workspace_test(&upstream, &local);
    let git = Git::new(&local)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&hooks);

    std::fs::write(local.join("clé.pem"), "secret").unwrap();
    std::fs::write(local.join("prod[1].key"), "secret").unwrap();
    std::fs::write(local.join("prod1.key"), "secret").unwrap();
    std::fs::write(local.join("notes.txt"), "keep me").unwrap();

    let workspace = Workspace {
        repo_url: format!("file://{}", upstream.display()),
        base_branch: "main".to_string(),
        task_branch: "main".to_string(),
        wip_branch: "factory/wip/DF-41".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };
    let mut last_pushed_sha = Some(git.current_head().await.unwrap());
    let params = FinalizeParams {
        repo_dir: &local,
        runtime_dir: &runtime_dir,
        workspace: &workspace,
        run_id: "run-finalize-b",
        outcome: RunOutcome::Completed,
        detail: "",
        harness_session_id: "",
        hooks_path: &hooks,
        ssl_ca_info: None,
        wip_size_cap_bytes: 10 * 1024 * 1024,
        diverged: false,
    };
    let res = finalize_workspace(&git, &params, &mut last_pushed_sha)
        .await
        .expect("finalize");
    let wip_sha = res.run_report.workspace.unwrap().wip_sha;
    assert_ne!(wip_sha, "");

    let files = git
        .git_output(&["ls-tree", "-r", "-z", "--name-only", &wip_sha])
        .await
        .unwrap();
    let files: Vec<&str> = files.split('\0').filter(|f| !f.is_empty()).collect();
    assert!(files.contains(&"notes.txt"));
    for secret in ["clé.pem", "prod[1].key", "prod1.key"] {
        assert!(
            !files.contains(&secret),
            "{secret} must not be in the WIP snapshot: {files:?}"
        );
    }
}

#[test]
fn test_runtime_writes_do_not_follow_planted_symlinks() {
    use crate::fs_util::{create_real_dir, write_fresh};
    use crate::git::{GitConfigParams, rewrite_git_config};

    let temp = tempfile::tempdir().unwrap();
    let victim = temp.path().join("run.json");
    std::fs::write(&victim, "original").unwrap();

    // write_fresh replaces the link instead of writing through it
    let link = temp.path().join("settings.json");
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    write_fresh(&link, "new").unwrap();
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "original");
    assert_eq!(std::fs::read_to_string(&link).unwrap(), "new");

    // a symlinked directory is replaced by a real one
    let dir_target = temp.path().join("elsewhere");
    std::fs::create_dir_all(&dir_target).unwrap();
    let dir_link = temp.path().join(".claude");
    std::os::unix::fs::symlink(&dir_target, &dir_link).unwrap();
    create_real_dir(&dir_link).unwrap();
    assert!(
        !std::fs::symlink_metadata(&dir_link)
            .unwrap()
            .file_type()
            .is_symlink()
    );

    // .git/config planted as a symlink
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::os::unix::fs::symlink(&victim, repo.join(".git").join("config")).unwrap();
    let params = GitConfigParams {
        hooks_path: std::path::Path::new("/opt/factory/hooks-empty"),
        extra_config: &[],
        repo_url: "https://github.com/org/repo.git",
        ssl_ca_info: None,
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };
    rewrite_git_config(&repo, &params).unwrap();
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "original");
    assert!(
        std::fs::read_to_string(repo.join(".git/config"))
            .unwrap()
            .contains("[remote \"origin\"]")
    );
}
