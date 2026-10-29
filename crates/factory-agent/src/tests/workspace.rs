//! Workspace clone and prepare.

#![expect(
    clippy::too_many_lines,
    reason = "scenario tests set up and check a whole run"
)]

use super::{clone_for_workspace_test, setup_test_remote_repo};
use crate::runtime_socket::RuntimeHandler;

fn scan_directory_for_secret(dir: &std::path::Path, secret: &[u8]) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(scan_directory_for_secret(&path, secret));
            } else if path.is_file()
                && let Ok(content) = std::fs::read(&path)
                && content.windows(secret.len()).any(|w| w == secret)
            {
                found.push(path);
            }
        }
    }
    found
}

struct TestCredentialProvider {
    user: String,
    pass: String,
}
#[tonic::async_trait]
impl RuntimeHandler for TestCredentialProvider {
    async fn handle_git_credential(
        &self,
        _p: &str,
        _h: &str,
        _path: &str,
    ) -> (Option<String>, Option<String>, Option<String>) {
        (Some(self.user.clone()), Some(self.pass.clone()), None)
    }
}

struct DeniedProvider;
#[tonic::async_trait]
impl RuntimeHandler for DeniedProvider {
    async fn handle_git_credential(
        &self,
        _p: &str,
        _h: &str,
        _path: &str,
    ) -> (Option<String>, Option<String>, Option<String>) {
        (None, None, Some("access denied".to_string()))
    }
}

#[tokio::test]
async fn test_scenario_clone_from_local_https_test_server_using_credential_helper() {
    use crate::runtime_socket::RuntimeSocketServer;
    use crate::test_harness::{LocalHttpsGitServer, TestPki};

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let git_root = temp_dir.path().join("git_root");
    let repo_dir = git_root.join("repo.git");
    let data_dir = temp_dir.path().join("data");
    let runtime_dir = temp_dir.path().join("runtime");
    let empty_hooks = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&data_dir).expect("create data dir");
    std::fs::create_dir_all(&runtime_dir).expect("create runtime dir");
    std::fs::create_dir_all(&empty_hooks).expect("create hooks dir");

    // 1. Initialize bare git repository in git_root
    std::fs::create_dir_all(&git_root).expect("create git_root");
    let out = tokio::process::Command::new("git")
        .args(["init", "--bare", "-b", "main", repo_dir.to_str().unwrap()])
        .output()
        .await
        .expect("git init bare");
    assert!(out.status.success());

    // Enable receivepack / uploadpack
    tokio::process::Command::new("git")
        .current_dir(&repo_dir)
        .args(["config", "http.receivepack", "true"])
        .output()
        .await
        .unwrap();

    // Seed initial commit into bare repo
    let seed_dir = temp_dir.path().join("seed");
    let out = tokio::process::Command::new("git")
        .args([
            "clone",
            repo_dir.to_str().unwrap(),
            seed_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .expect("clone seed");
    assert!(out.status.success());

    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.name", "Seed User"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["config", "user.email", "seed@example.com"])
        .output()
        .await
        .unwrap();

    std::fs::write(seed_dir.join("hello.txt"), "Dark Factory Iteration 1").expect("write file");
    tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["add", "hello.txt"])
        .output()
        .await
        .unwrap();
    let out = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["commit", "-m", "initial commit"])
        .output()
        .await
        .expect("seed commit");
    assert!(out.status.success());
    let out = tokio::process::Command::new("git")
        .current_dir(&seed_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await
        .expect("seed push");
    assert!(out.status.success());

    // 2. Start LocalHttpsGitServer with required Basic auth
    let pki = TestPki::generate("localhost").expect("pki");
    let expected_user = "bot".to_string();
    let expected_pass = "super-secret-token-42".to_string();
    let git_server = LocalHttpsGitServer::start(
        &pki,
        git_root.clone(),
        Some((expected_user.clone(), expected_pass.clone())),
    )
    .expect("start https git server");

    // Write CA certificate to file for Git to trust
    let ca_path = temp_dir.path().join("ca.crt");
    std::fs::write(&ca_path, &pki.ca_cert_pem).expect("write ca.crt");

    // 3. Start RuntimeSocketServer on runtime_dir/runtime.sock
    let socket_path = runtime_dir.join("runtime.sock");

    let socket_server = RuntimeSocketServer::bind(
        &socket_path,
        std::sync::Arc::new(TestCredentialProvider {
            user: expected_user,
            pass: expected_pass.clone(),
        }),
    )
    .expect("bind socket");
    let _socket_handle = socket_server.spawn();

    // 4. Create python credential helper that communicates with runtime socket
    let helper_path = temp_dir.path().join("git-credential-helper.py");
    let python_helper_code = r#"#!/usr/bin/env python3
import sys, socket, json, struct

operation = sys.argv[-1] if len(sys.argv) > 1 else "get"
if operation not in ("get",):
    sys.exit(0)

sock_path = sys.argv[1] if len(sys.argv) > 2 else os.environ.get("DF_RUNTIME_SOCK", "")

fields = {}
for line in sys.stdin:
    line = line.strip()
    if not line:
        break
    if "=" in line:
        k, v = line.split("=", 1)
        fields[k.strip()] = v.strip()

s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sock_path)

req = {
    "type": "git_credential",
    "protocol": fields.get("protocol", ""),
    "host": fields.get("host", ""),
    "path": fields.get("path", "")
}
payload = json.dumps(req).encode("utf-8")
s.sendall(struct.pack(">I", len(payload)) + payload)

len_bytes = s.recv(4)
if len(len_bytes) == 4:
    resp_len = struct.unpack(">I", len_bytes)[0]
    data = b""
    while len(data) < resp_len:
        chunk = s.recv(resp_len - len(data))
        if not chunk:
            break
        data += chunk
    resp = json.loads(data.decode("utf-8"))
    username = resp.get("username")
    password = resp.get("password")
    if username and password:
        sys.stdout.write(f"username={username}\npassword={password}\n")
        sys.stdout.flush()
"#;
    std::fs::write(&helper_path, python_helper_code).expect("write helper script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&helper_path).expect("meta").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&helper_path, perms).expect("set permissions");
    }

    // 5. Clone repository using hardened Git with credential helper
    let target_clone_dir = data_dir.join("repo");
    let repo_url = format!("https://localhost:{}/repo.git", git_server.addr.port());
    let helper_cmd = format!("{} {}", helper_path.display(), socket_path.display());

    let clone_output = tokio::process::Command::new("git")
        .arg("-c")
        .arg(format!("core.hooksPath={}", empty_hooks.display()))
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("credential.helper=")
        .arg("-c")
        .arg(format!("credential.helper={helper_cmd}"))
        .arg("-c")
        .arg("protocol.allow=never")
        .arg("-c")
        .arg("protocol.https.allow=always")
        .arg("-c")
        .arg(format!("http.sslCAInfo={}", ca_path.display()))
        .arg("-c")
        .arg("submodule.recurse=false")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["clone", &repo_url, target_clone_dir.to_str().unwrap()])
        .output()
        .await
        .expect("git clone");

    assert!(
        clone_output.status.success(),
        "git clone with credential helper must succeed. Stderr: {}",
        String::from_utf8_lossy(&clone_output.stderr)
    );

    // Verify cloned content
    assert!(target_clone_dir.join("hello.txt").exists());
    let file_content = std::fs::read_to_string(target_clone_dir.join("hello.txt")).unwrap();
    assert_eq!(file_content, "Dark Factory Iteration 1");

    // 6. Verify: NO password lands under /data
    let leaked_files = scan_directory_for_secret(&data_dir, expected_pass.as_bytes());
    assert!(
        leaked_files.is_empty(),
        "password must never land under /data, but found in: {leaked_files:?}"
    );

    // 7. Verify scenario: Denied request fails git clone
    let denied_clone_dir = data_dir.join("denied_repo");
    let denied_socket_path = runtime_dir.join("denied_runtime.sock");
    let denied_socket_server =
        RuntimeSocketServer::bind(&denied_socket_path, std::sync::Arc::new(DeniedProvider))
            .expect("bind denied socket");
    let _denied_handle = denied_socket_server.spawn();

    let denied_helper_cmd = format!("{} {}", helper_path.display(), denied_socket_path.display());
    let denied_output = tokio::process::Command::new("git")
        .arg("-c")
        .arg(format!("core.hooksPath={}", empty_hooks.display()))
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("credential.helper=")
        .arg("-c")
        .arg(format!("credential.helper={denied_helper_cmd}"))
        .arg("-c")
        .arg("protocol.allow=never")
        .arg("-c")
        .arg("protocol.https.allow=always")
        .arg("-c")
        .arg(format!("http.sslCAInfo={}", ca_path.display()))
        .arg("-c")
        .arg("submodule.recurse=false")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["clone", &repo_url, denied_clone_dir.to_str().unwrap()])
        .output()
        .await
        .expect("git clone denied");

    assert!(
        !denied_output.status.success(),
        "git clone must fail when credentials are denied"
    );

    git_server.shutdown();
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

    let res = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
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

    let res = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
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
    let _ = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
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
    let res = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
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
    let res = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
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
    let _ = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
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
    let res = prepare_workspace(
        &git,
        &local_dir,
        &workspace,
        &empty_hooks,
        "/opt/factory/factory-agent git-credential",
    )
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

    let res = prepare_workspace(&git, &local, &workspace, &hooks, "")
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
