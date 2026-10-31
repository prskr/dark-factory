//! Workspace clone and prepare.

#![expect(
    clippy::too_many_lines,
    reason = "scenario tests set up and check a whole run"
)]

use super::{clone_for_workspace_test, setup_test_remote_repo};
use crate::git::{Git, proxy_ca_override};

/// Bare repository `repo.git` under a fresh `git_root` for the local HTTPS server.
fn init_bare_repo(git_root: &std::path::Path) {
    std::fs::create_dir_all(git_root).expect("create git_root");
    let out = std::process::Command::new("git")
        .args(["init", "--bare", "-b", "main"])
        .arg(git_root.join("repo.git"))
        .output()
        .expect("git init bare");
    assert!(out.status.success());
}

#[tokio::test]
async fn test_git_trusts_the_proxy_certificate_and_never_prompts_for_credentials() {
    use crate::test_harness::{LocalHttpsGitServer, TestPki};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let pki = TestPki::generate("localhost").expect("pki");
    let ca_path = temp_dir.path().join("ca.crt");
    std::fs::write(&ca_path, &pki.ca_cert_pem).expect("write ca.crt");
    let git = Git::new(temp_dir.path())
        .with_hooks_empty_dir(temp_dir.path().join("hooks-empty"))
        .with_global_gitconfig(temp_dir.path().join("gitconfig"))
        .with_ssl_ca_info(&ca_path);

    // Proxy certificate trusted: the instance CA, scoped to the proxy URL, verifies the server
    let open_root = temp_dir.path().join("open");
    init_bare_repo(&open_root);
    let open_server = LocalHttpsGitServer::start(&pki, open_root, None).expect("start server");
    let url = format!("https://localhost:{}/repo.git", open_server.addr.port());
    let ca = proxy_ca_override(&url, Some(&ca_path)).expect("CA override");
    let out = git
        .run_cmd(&["-c", &ca, "ls-remote", &url])
        .await
        .expect("ls-remote");
    assert!(
        out.status.success(),
        "ls-remote must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    open_server.shutdown();

    // No credential prompt: a 401 fails at once instead of asking for a username
    let auth_root = temp_dir.path().join("auth");
    init_bare_repo(&auth_root);
    let auth_server = LocalHttpsGitServer::start(
        &pki,
        auth_root,
        Some(("bot".to_string(), "secret".to_string())),
    )
    .expect("start server");
    let url = format!("https://localhost:{}/repo.git", auth_server.addr.port());
    let ca = proxy_ca_override(&url, Some(&ca_path)).expect("CA override");
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        git.run_cmd(&["-c", &ca, "ls-remote", &url]),
    )
    .await
    .expect("git must not wait for a prompt")
    .expect("ls-remote");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("terminal prompts disabled"),
        "unexpected stderr: {stderr}"
    );
    auth_server.shutdown();
}

#[tokio::test]
async fn test_git_rejects_a_server_certificate_from_another_ca() {
    use crate::test_harness::{LocalHttpsGitServer, TestPki};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let instance_pki = TestPki::generate("localhost").expect("pki");
    let other_pki = TestPki::generate("localhost").expect("pki");
    let ca_path = temp_dir.path().join("ca.crt");
    std::fs::write(&ca_path, &instance_pki.ca_cert_pem).expect("write ca.crt");
    let git = Git::new(temp_dir.path())
        .with_hooks_empty_dir(temp_dir.path().join("hooks-empty"))
        .with_global_gitconfig(temp_dir.path().join("gitconfig"))
        .with_ssl_ca_info(&ca_path);
    let root = temp_dir.path().join("root");
    init_bare_repo(&root);
    let server = LocalHttpsGitServer::start(&other_pki, root, None).expect("start server");
    let url = format!("https://localhost:{}/repo.git", server.addr.port());

    let ca = proxy_ca_override(&url, Some(&ca_path)).expect("CA override");

    let out = git
        .run_cmd(&["-c", &ca, "ls-remote", &url])
        .await
        .expect("ls-remote");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(
        stderr.contains("certificate"),
        "unexpected stderr: {stderr}"
    );
    server.shutdown();
}

#[tokio::test]
async fn test_fetch_trusts_the_proxy_through_the_scoped_ca_in_git_config() {
    use crate::git::{GitConfigParams, rewrite_git_config};
    use crate::test_harness::{LocalHttpsGitServer, TestPki};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let pki = TestPki::generate("localhost").expect("pki");
    let ca_path = temp_dir.path().join("ca.crt");
    std::fs::write(&ca_path, &pki.ca_cert_pem).expect("write ca.crt");
    let upstream = setup_test_remote_repo(&temp_dir.path().join("seed"));
    let root = temp_dir.path().join("root");
    let out = std::process::Command::new("git")
        .args(["clone", "--bare"])
        .arg(&upstream)
        .arg(root.join("repo.git"))
        .output()
        .expect("git clone --bare");
    assert!(out.status.success());
    let server = LocalHttpsGitServer::start(&pki, root, None).expect("start server");
    let url = format!("https://localhost:{}/repo.git", server.addr.port());

    let local = temp_dir.path().join("local");
    let out = std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&local)
        .output()
        .expect("git init");
    assert!(out.status.success());
    let hooks = temp_dir.path().join("hooks-empty");
    rewrite_git_config(
        &local,
        &GitConfigParams {
            repo_url: &url,
            commit_author_name: "Bot",
            commit_author_email: "bot@example.com",
            hooks_path: &hooks,
            ssl_ca_info: Some(&ca_path),
            extra_config: &[],
        },
    )
    .expect("rewrite config");
    // No GIT_SSL_CAINFO: only the `[http "<proxy URL>"]` section trusts the instance CA
    let git = Git::new(&local)
        .with_hooks_empty_dir(&hooks)
        .with_global_gitconfig(temp_dir.path().join("gitconfig"));

    let out = git
        .run_cmd(&["fetch", "origin", "main"])
        .await
        .expect("fetch");

    assert!(
        out.status.success(),
        "fetch must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    server.shutdown();
}
#[tokio::test]
async fn test_scenario_prepare_first_run() {
    use crate::git::Git;
    use crate::workspace::prepare_workspace;
    use df_proto::agent::v1::Workspace;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("data").join("repo");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // 1. Initialize bare remote repository
    let out = tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await
        .expect("git init bare");
    assert!(out.status.success());

    // Make an initial commit on main
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
    std::fs::write(seed_dir.join("init.txt"), "Initial").expect("write file");
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "init.txt"])
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

    // 2. Prepare workspace on empty volume
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    let res = prepare_workspace(&git, &local_dir, &workspace, &empty_hooks, None)
        .await
        .expect("prepare workspace");

    assert!(!res.diverged);
    assert!(res.push_report.is_none());

    // Branch must be factory/task/DF-1
    let branch = git
        .git_output(&["branch", "--show-current"])
        .await
        .expect("current branch");
    assert_eq!(branch, "factory/task/DF-1");

    // Must be at origin/main
    let local_head = git.current_head().await.expect("head");
    let remote_main = git
        .git_output(&["rev-parse", "origin/main"])
        .await
        .expect("origin/main");
    assert_eq!(local_head, remote_main);

    // Verified blobless clone config
    let config =
        std::fs::read_to_string(local_dir.join(".git").join("config")).expect("read config");
    assert!(config.contains("partialclonefilter = blob:none"));
}

#[tokio::test]
async fn test_scenario_prepare_volume_lost_restore_wip() {
    use crate::git::Git;
    use crate::workspace::prepare_workspace;
    use df_proto::agent::v1::Workspace;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("data").join("repo");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // 1. Initialize bare remote repository
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

    // Create main commit
    std::fs::write(seed_dir.join("main.txt"), "Main content").expect("write file");
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "main.txt"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "main commit"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await
        .unwrap();

    // Create factory/task/DF-1 branch on remote
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/task/DF-1"])
        .output()
        .await
        .unwrap();
    std::fs::write(seed_dir.join("task.txt"), "Task work").expect("write file");
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "task.txt"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "task commit"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await
        .unwrap();

    // Create factory/wip/DF-1 branch on remote
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "-b", "factory/wip/DF-1"])
        .output()
        .await
        .unwrap();
    std::fs::write(seed_dir.join("wip_uncommitted.txt"), "WIP draft").expect("write file");
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "wip_uncommitted.txt"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "wip commit"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/wip/DF-1"])
        .output()
        .await
        .unwrap();

    // 2. Volume lost after earlier runs: prepare on empty local_dir
    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    let res = prepare_workspace(&git, &local_dir, &workspace, &empty_hooks, None)
        .await
        .expect("prepare workspace");

    assert!(!res.diverged);

    // Task branch checked out
    let branch = git
        .git_output(&["branch", "--show-current"])
        .await
        .expect("branch");
    assert_eq!(branch, "factory/task/DF-1");

    // Committed task file is present
    assert!(local_dir.join("task.txt").exists());

    // WIP files restored into the working tree!
    assert!(
        local_dir.join("wip_uncommitted.txt").exists(),
        "WIP uncommitted file should be restored"
    );
    let content = std::fs::read_to_string(local_dir.join("wip_uncommitted.txt")).unwrap();
    assert_eq!(content, "WIP draft");
}

#[tokio::test]
async fn test_scenario_prepare_unpushed_commits_and_fast_forward() {
    use crate::git::Git;
    use crate::workspace::prepare_workspace;
    use df_proto::agent::v1::{PushStatus, PushTarget, Workspace};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("data").join("repo");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // 1. Initialize bare remote repository with main
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

    std::fs::write(seed_dir.join("main.txt"), "Main").expect("write file");
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

    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    // First prepare (fresh clone)
    let _ = prepare_workspace(&git, &local_dir, &workspace, &empty_hooks, None)
        .await
        .expect("initial prepare");

    // 2. Previous run made a commit locally on the task branch but was killed before pushing!
    git.run_cmd(&["config", "commit.gpgsign", "false"])
        .await
        .unwrap();
    git.run_cmd(&["config", "user.name", "Agent Bot"])
        .await
        .unwrap();
    git.run_cmd(&["config", "user.email", "bot@example.com"])
        .await
        .unwrap();

    std::fs::write(local_dir.join("agent_work.txt"), "Done work").expect("write file");
    git.run_cmd(&["add", "agent_work.txt"]).await.unwrap();
    git.run_cmd(&["commit", "-m", "agent committed"])
        .await
        .unwrap();

    let unpushed_sha = git.current_head().await.expect("head");

    // 3. Second run's prepare on the existing workspace MUST push the unpushed commits!
    let res = prepare_workspace(&git, &local_dir, &workspace, &empty_hooks, None)
        .await
        .expect("second prepare");

    assert!(!res.diverged);
    assert!(
        res.push_report.is_some(),
        "must emit PushReport for unpushed commits"
    );
    let report = res.push_report.unwrap();
    assert_eq!(report.target, PushTarget::TaskBranch as i32);
    assert_eq!(report.status, PushStatus::Ok as i32);
    assert_eq!(report.sha, unpushed_sha);
    assert_eq!(report.commits_ahead_of_base, 1);

    // Verify remote task branch now matches local SHA
    let remote_sha = git
        .git_output(&["rev-parse", "origin/factory/task/DF-1"])
        .await
        .expect("remote sha");
    assert_eq!(remote_sha, unpushed_sha);

    // 4. Remote moved ahead: seed_dir pushes another commit to factory/task/DF-1
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["fetch", "origin"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["checkout", "factory/task/DF-1"])
        .output()
        .await
        .unwrap();
    std::fs::write(seed_dir.join("remote_ahead.txt"), "From remote").expect("write file");
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "remote_ahead.txt"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "remote ahead commit"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await
        .unwrap();

    // Third prepare: local is behind remote, must fast-forward merge cleanly
    let res = prepare_workspace(&git, &local_dir, &workspace, &empty_hooks, None)
        .await
        .expect("third prepare ff");

    assert!(!res.diverged);
    assert!(res.push_report.is_none());
    assert!(
        local_dir.join("remote_ahead.txt").exists(),
        "fast-forward merge must bring in remote file"
    );
}

#[tokio::test]
async fn test_scenario_prepare_diverged() {
    use crate::git::Git;
    use crate::workspace::prepare_workspace;
    use df_proto::agent::v1::Workspace;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("data").join("repo");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks");

    // 1. Initialize bare remote repository with main
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

    std::fs::write(seed_dir.join("main.txt"), "Main").expect("write file");
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

    // Push initial task branch
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

    let git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks);

    let workspace = Workspace {
        repo_url: format!("file://{}", remote_dir.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-1".to_string(),
        wip_branch: "factory/wip/DF-1".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    // Prepare fresh workspace
    let _ = prepare_workspace(&git, &local_dir, &workspace, &empty_hooks, None)
        .await
        .expect("prepare");

    // 2. Local creates a commit A
    git.run_cmd(&["config", "commit.gpgsign", "false"])
        .await
        .unwrap();
    git.run_cmd(&["config", "user.name", "Agent Bot"])
        .await
        .unwrap();
    git.run_cmd(&["config", "user.email", "bot@example.com"])
        .await
        .unwrap();
    std::fs::write(local_dir.join("local.txt"), "Local only").expect("write local");
    git.run_cmd(&["add", "local.txt"]).await.unwrap();
    git.run_cmd(&["commit", "-m", "local commit"])
        .await
        .unwrap();
    let local_sha = git.current_head().await.expect("local head");

    // 3. Remote creates a conflicting commit B
    std::fs::write(seed_dir.join("remote.txt"), "Remote only").expect("write remote");
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "remote.txt"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "remote commit"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:factory/task/DF-1"])
        .output()
        .await
        .unwrap();

    // 4. Prepare on existing workspace MUST detect divergence:
    // Both have commits the other lacks!
    let res = prepare_workspace(&git, &local_dir, &workspace, &empty_hooks, None)
        .await
        .expect("prepare diverged");

    assert!(res.diverged, "workspace must be marked diverged");
    assert!(
        res.push_report.is_none(),
        "no push should happen when diverged"
    );
    assert_eq!(res.notices.len(), 1);
    assert_eq!(res.notices[0].code, "workspace.diverged");

    // Neither branch is reset: local HEAD is still local_sha
    let head = git.current_head().await.expect("head");
    assert_eq!(
        head, local_sha,
        "local branch must not be reset on divergence"
    );
}

#[tokio::test]
async fn test_prepare_recreates_missing_task_branch() {
    use crate::git::Git;
    use crate::workspace::prepare_workspace;
    use df_proto::agent::v1::Workspace;

    let temp = tempfile::tempdir().unwrap();
    let upstream = setup_test_remote_repo(temp.path());
    let local = temp.path().join("local");
    let hooks = temp.path().join("hooks-empty");
    std::fs::create_dir_all(&hooks).unwrap();
    // Existing workspace on `main` without a local task branch
    clone_for_workspace_test(&upstream, &local);

    let git = Git::new(&local)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&hooks);
    let workspace = Workspace {
        repo_url: format!("file://{}", upstream.display()),
        base_branch: "main".to_string(),
        task_branch: "factory/task/DF-13".to_string(),
        wip_branch: "factory/wip/DF-13".to_string(),
        commit_identity: None,
        git_hooks: vec![],
        git_config: vec![],
    };

    let res = prepare_workspace(&git, &local, &workspace, &hooks, None)
        .await
        .expect("prepare");
    assert!(!res.push_failed);
    assert_eq!(
        git.git_output(&["rev-parse", "--abbrev-ref", "HEAD"])
            .await
            .unwrap(),
        "factory/task/DF-13"
    );
}
