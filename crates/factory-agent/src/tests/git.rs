//! Git configuration denylist, planted configuration, hooks, remotes, socket mode and the credential helper.

use df_proto::agent::v1::{AgentMessage, agent_message};

#[test]
fn test_git_config_denylist() {
    use crate::git::is_config_entry_denied;

    // Denied execution / programs
    assert!(is_config_entry_denied("core.hooksPath", "/tmp/hooks"));
    assert!(is_config_entry_denied("core.fsmonitor", "true"));
    assert!(is_config_entry_denied("core.sshCommand", "ssh -i id_rsa"));
    assert!(is_config_entry_denied("core.gitProxy", "proxy"));
    assert!(is_config_entry_denied("core.askPass", "pass"));
    assert!(is_config_entry_denied("core.editor", "vim"));
    assert!(is_config_entry_denied("core.pager", "less"));
    assert!(is_config_entry_denied("sequence.editor", "code"));
    assert!(is_config_entry_denied("diff.external", "ext-diff"));
    assert!(is_config_entry_denied("diff.rust.textconv", "cat"));
    assert!(is_config_entry_denied("diff.custom.command", "custom-diff"));
    assert!(is_config_entry_denied(
        "merge.custom.driver",
        "custom-merge"
    ));
    assert!(is_config_entry_denied("pager.log", "cat"));
    assert!(is_config_entry_denied("filter.clean.clean", "clean-cmd"));
    assert!(is_config_entry_denied("gpg.program", "gpg"));
    assert!(is_config_entry_denied("alias.evil", "!rm -rf /"));
    assert!(!is_config_entry_denied("alias.st", "status"));

    // Denied traffic / credentials
    assert!(is_config_entry_denied(
        "remote.origin.url",
        "https://evil.org"
    ));
    assert!(is_config_entry_denied(
        "url.https://evil.insteadOf",
        "https://good"
    ));
    assert!(is_config_entry_denied("branch.main.remote", "origin"));
    assert!(is_config_entry_denied("branch.main.pushRemote", "origin"));
    assert!(is_config_entry_denied("credential.helper", "store"));
    assert!(is_config_entry_denied("http.proxy", "http://evil"));
    assert!(is_config_entry_denied("protocol.allow", "always"));

    // Denied loading / runtime owned
    assert!(is_config_entry_denied("include.path", "/etc/passwd"));
    assert!(is_config_entry_denied("includeIf.gitdir:foo.path", "/bar"));
    assert!(is_config_entry_denied("user.name", "Attacker"));
    assert!(is_config_entry_denied("user.email", "attacker@evil.org"));
    assert!(is_config_entry_denied("submodule.recurse", "true"));
    assert!(is_config_entry_denied("lfs.url", "https://evil"));
    assert!(is_config_entry_denied("core.worktree", "/root"));
    assert!(is_config_entry_denied("core.bare", "false"));
    assert!(is_config_entry_denied("extensions.preciousObjects", "true"));
    assert!(is_config_entry_denied("commit.gpgSign", "true"));
    assert!(is_config_entry_denied("tag.gpgSign", "true"));

    // Allowed entries
    assert!(!is_config_entry_denied("rebase.autoStash", "true"));
    assert!(!is_config_entry_denied("diff.algorithm", "histogram"));
    assert!(!is_config_entry_denied("pull.rebase", "merges"));
    assert!(!is_config_entry_denied("merge.conflictStyle", "zdiff3"));
    assert!(!is_config_entry_denied("rerere.enabled", "true"));
}

#[tokio::test]
async fn test_planted_configuration_scenario() {
    use crate::git::{GitConfigParams, rewrite_git_config};
    use df_proto::agent::v1::GitConfigEntry;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_dir = temp_dir.path().join("repo");
    let git_dir = repo_dir.join(".git");
    std::fs::create_dir_all(&git_dir).expect("create .git");

    // Plant malicious configuration into .git/config
    let planted_config = r#"
[core]
	repositoryformatversion = 0
	filemode = true
	bare = false
	fsmonitor = true
[remote "origin"]
	url = https://malicious.example.org/planted.git
	fetch = +refs/heads/*:refs/remotes/origin/*
[url "https://malicious.org"]
	insteadOf = https://example.com
[include]
	path = /tmp/evil.inc
[credential]
	helper = store
"#;
    std::fs::write(git_dir.join("config"), planted_config).expect("write planted config");

    // Plant .git/config.worktree
    std::fs::write(git_dir.join("config.worktree"), "[core]\nworktree = /tmp\n")
        .expect("write worktree config");
    assert!(git_dir.join("config.worktree").exists());

    let hooks_path = std::path::Path::new("/opt/factory/hooks-empty");
    let params = GitConfigParams {
        repo_url: "https://git.example.org/peter/dark-factory.git",
        commit_author_name: "Agent Bot",
        commit_author_email: "bot@darkfactory.local",
        hooks_path,
        credential_helper: "/opt/factory/factory-agent git-credential",
        extra_config: &[
            GitConfigEntry {
                key: "rebase.autoStash".to_string(),
                value: "true".to_string(),
            },
            // Planted entry in extra_config should also be stripped by denylist
            GitConfigEntry {
                key: "core.sshCommand".to_string(),
                value: "malicious-ssh".to_string(),
            },
        ],
    };

    rewrite_git_config(&repo_dir, &params).expect("rewrite config");

    // 1. .git/config.worktree MUST be deleted
    assert!(!git_dir.join("config.worktree").exists());

    // 2. Read new .git/config
    let new_content = std::fs::read_to_string(git_dir.join("config")).expect("read config");

    // Must contain the expected repo_url from Assign
    assert!(new_content.contains("url = \"https://git.example.org/peter/dark-factory.git\""));
    // Must contain the expected author identity
    assert!(new_content.contains("name = \"Agent Bot\""));
    assert!(new_content.contains("email = \"bot@darkfactory.local\""));
    // Must contain runtime credential helper
    assert!(new_content.contains("helper = \"/opt/factory/factory-agent git-credential\""));
    // Must contain hooksPath
    assert!(new_content.contains("hooksPath = /opt/factory/hooks-empty"));
    // Must contain allowed extra config
    assert!(new_content.contains("autoStash = \"true\""));

    // MUST NOT contain any planted or denylisted entries
    assert!(!new_content.contains("fsmonitor"));
    assert!(!new_content.contains("insteadOf"));
    assert!(!new_content.contains("include"));
    assert!(!new_content.contains("store"));
    assert!(!new_content.contains("malicious"));
    assert!(!new_content.contains("sshCommand"));
}

#[tokio::test]
async fn test_malicious_hook_scenario() {
    use crate::git::Git;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let remote_dir = temp_dir.path().join("remote.git");
    let local_dir = temp_dir.path().join("local");
    let empty_hooks_dir = temp_dir.path().join("hooks-empty");
    std::fs::create_dir_all(&empty_hooks_dir).expect("create empty hooks dir");

    // 1. Initialize bare remote
    let out = tokio::process::Command::new("git")
        .args(["init", "--bare", remote_dir.to_str().unwrap()])
        .output()
        .await
        .expect("git init bare");
    assert!(out.status.success());

    // 2. Clone locally using standard git
    let out = tokio::process::Command::new("git")
        .args([
            "clone",
            remote_dir.to_str().unwrap(),
            local_dir.to_str().unwrap(),
        ])
        .output()
        .await
        .expect("git clone");
    assert!(out.status.success());

    // 3. Make an initial commit
    let file_path = local_dir.join("README.md");
    std::fs::write(&file_path, "test").expect("write file");
    tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["config", "commit.gpgsign", "false"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["config", "user.name", "Test User"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["config", "user.email", "test@example.com"])
        .output()
        .await
        .unwrap();
    tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["add", "README.md"])
        .output()
        .await
        .unwrap();
    let out = tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["commit", "-m", "init"])
        .output()
        .await
        .expect("git commit");
    assert!(out.status.success());

    // 4. Plant an executable malicious hook at .git/hooks/pre-push that fails with code 99
    let hooks_dir = local_dir.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).expect("create hooks dir");
    let pre_push_path = hooks_dir.join("pre-push");
    let hook_content = "#!/bin/sh\nexit 99\n";
    std::fs::write(&pre_push_path, hook_content).expect("write hook");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&pre_push_path)
            .expect("meta")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&pre_push_path, perms).expect("set permissions");
    }

    // 5. Normal git push would fail with status 99 because of the pre-push hook!
    let normal_push = tokio::process::Command::new("git")
        .current_dir(&local_dir)
        .args(["push", "origin", "HEAD:main"])
        .output()
        .await
        .expect("normal push");
    assert!(
        !normal_push.status.success(),
        "normal push must fail because hook exits 99"
    );

    // 6. Hardened Git with empty hooks directory MUST ignore the malicious hook and succeed!
    let hardened_git = Git::new(&local_dir)
        .allow_protocols(["https", "file"])
        .with_hooks_empty_dir(&empty_hooks_dir);

    let hardened_push = hardened_git
        .push("origin", "HEAD:main", false)
        .await
        .expect("hardened push");
    assert!(
        hardened_push.status.success(),
        "hardened push must succeed, ignoring malicious hook. Stderr: {}",
        String::from_utf8_lossy(&hardened_push.stderr)
    );
}

#[tokio::test]
async fn test_non_https_remote_scenario() {
    use crate::git::Git;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo_dir = temp_dir.path().join("repo");
    std::fs::create_dir_all(&repo_dir).expect("create repo dir");

    // Construct Git with production defaults (only https allowed)
    let git = Git::new(&repo_dir);

    // 1. Attempt file:// protocol
    let file_result = git.git_output(&["ls-remote", "file:///dev/null"]).await;
    assert!(file_result.is_err(), "file:// protocol should be refused");
    let err_str = file_result.unwrap_err().to_string();
    assert!(
        err_str.contains("transport 'file' not allowed") || err_str.contains("not allowed"),
        "expected transport 'file' not allowed error, got: {err_str}"
    );

    // 2. Attempt ssh:// protocol
    let ssh_result = git
        .git_output(&["ls-remote", "ssh://git@example.com/repo.git"])
        .await;
    assert!(ssh_result.is_err(), "ssh:// protocol should be refused");
    let err_str = ssh_result.unwrap_err().to_string();
    assert!(
        err_str.contains("transport 'ssh' not allowed") || err_str.contains("not allowed"),
        "expected transport 'ssh' not allowed error, got: {err_str}"
    );
}

#[tokio::test]
async fn test_runtime_socket_permissions_mode_0600() {
    use crate::runtime_socket::{RuntimeHandler, RuntimeSocketServer};

    struct DummyHandler;
    #[tonic::async_trait]
    impl RuntimeHandler for DummyHandler {
        async fn handle_git_credential(
            &self,
            _p: &str,
            _h: &str,
            _path: &str,
        ) -> (Option<String>, Option<String>, Option<String>) {
            (None, None, None)
        }
    }

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let socket_path = temp_dir.path().join("runtime.sock");

    let server = RuntimeSocketServer::bind(&socket_path, std::sync::Arc::new(DummyHandler))
        .expect("bind server");
    let _handle = server.spawn();

    assert!(socket_path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&socket_path)
            .expect("metadata")
            .permissions();
        assert_eq!(
            perms.mode() & 0o777,
            0o600,
            "runtime socket must have mode 0600"
        );
    }
}

#[tokio::test]
async fn test_git_credential_client_get_store_erase() {
    use crate::runtime_socket::{RuntimeHandler, RuntimeSocketServer, run_git_credential_stream};

    struct TestHandler {
        deny: bool,
    }

    #[tonic::async_trait]
    impl RuntimeHandler for TestHandler {
        async fn handle_git_credential(
            &self,
            protocol: &str,
            host: &str,
            path: &str,
        ) -> (Option<String>, Option<String>, Option<String>) {
            assert_eq!(protocol, "https");
            assert_eq!(host, "git.example.org");
            assert_eq!(path, "peter/dark-factory.git");
            if self.deny {
                (None, None, Some("access denied".to_string()))
            } else {
                (
                    Some("bot".to_string()),
                    Some("secret-token-xyz".to_string()),
                    None,
                )
            }
        }
    }

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let socket_path = temp_dir.path().join("runtime.sock");

    let server = RuntimeSocketServer::bind(
        &socket_path,
        std::sync::Arc::new(TestHandler { deny: false }),
    )
    .expect("bind server");
    let _handle = server.spawn();

    // 1. Test "get" with valid credentials
    let input = b"protocol=https\nhost=git.example.org\npath=peter/dark-factory.git\n\n";
    let mut output = Vec::new();
    run_git_credential_stream(&input[..], &mut output, &socket_path, Some("get"))
        .await
        .expect("run get");

    let out_str = String::from_utf8(output).expect("utf8 output");
    assert!(out_str.contains("username=bot\n"));
    assert!(out_str.contains("password=secret-token-xyz\n"));

    // 2. Test "store" is a no-op
    let store_input =
        b"protocol=https\nhost=git.example.org\nusername=bot\npassword=secret-token-xyz\n\n";
    let mut store_output = Vec::new();
    run_git_credential_stream(
        &store_input[..],
        &mut store_output,
        &socket_path,
        Some("store"),
    )
    .await
    .expect("run store");
    assert!(store_output.is_empty(), "store must be a no-op");

    // 3. Test "erase" is a no-op
    let erase_input = b"protocol=https\nhost=git.example.org\n\n";
    let mut erase_output = Vec::new();
    run_git_credential_stream(
        &erase_input[..],
        &mut erase_output,
        &socket_path,
        Some("erase"),
    )
    .await
    .expect("run erase");
    assert!(erase_output.is_empty(), "erase must be a no-op");

    // 4. Test "get" when request is denied
    let denied_socket = temp_dir.path().join("denied.sock");
    let denied_server = RuntimeSocketServer::bind(
        &denied_socket,
        std::sync::Arc::new(TestHandler { deny: true }),
    )
    .expect("bind denied server");
    let _denied_handle = denied_server.spawn();

    let mut denied_output = Vec::new();
    run_git_credential_stream(&input[..], &mut denied_output, &denied_socket, Some("get"))
        .await
        .expect("run denied get");
    assert!(
        denied_output.is_empty(),
        "denied credentials must output nothing"
    );
}

#[test]
fn test_credentials_are_scoped_to_the_task_repository() {
    use crate::runtime_handler::matches_repo_url;

    let repo = "https://github.com/prskr/dark-factory.git";
    assert!(matches_repo_url(
        repo,
        "https",
        "github.com",
        "prskr/dark-factory.git"
    ));
    assert!(matches_repo_url(
        repo,
        "https",
        "github.com",
        "prskr/dark-factory"
    ));
    assert!(
        !matches_repo_url(repo, "https", "github.com", ""),
        "path is required"
    );

    // Attacker host must be denied
    assert!(!matches_repo_url(
        repo,
        "https",
        "attacker.example",
        "prskr/dark-factory.git"
    ));
    assert!(!matches_repo_url(
        repo,
        "http",
        "github.com",
        "prskr/dark-factory.git"
    ));
    assert!(!matches_repo_url(
        repo,
        "https",
        "github.com",
        "other-org/other-repo"
    ));

    // Local file URL
    let file_repo = "file:///data/local-repo.git";
    assert!(matches_repo_url(
        file_repo,
        "file",
        "",
        "/data/local-repo.git"
    ));
    assert!(matches_repo_url(file_repo, "file", "", "data/local-repo"));
    assert!(!matches_repo_url(
        file_repo,
        "file",
        "",
        "data/different-repo"
    ));
}

#[test]
fn test_render_git_config_rejects_injection() {
    use crate::git::{GitConfigParams, render_git_config};
    use df_proto::agent::v1::GitConfigEntry;
    use std::path::Path;

    // Newlines in value must be skipped/filtered
    let entries = vec![GitConfigEntry {
        key: "user.name".to_string(),
        value: "evil\n[url \"https://evil/\"]\ninsteadOf = https://github.com/".to_string(),
    }];
    let params = GitConfigParams {
        hooks_path: Path::new("/etc/factory/hooks"),
        extra_config: &entries,
        repo_url: "https://github.com/prskr/dark-factory.git",
        credential_helper: "factory-agent git-credential",
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };
    let rendered = render_git_config(&params);
    assert!(!rendered.contains("https://evil/"));

    // Newlines in key must be skipped/filtered
    let entries2 = vec![GitConfigEntry {
        key: "user.name\nfake".to_string(),
        value: "test".to_string(),
    }];
    let params2 = GitConfigParams {
        hooks_path: Path::new("/etc/factory/hooks"),
        extra_config: &entries2,
        repo_url: "https://github.com/prskr/dark-factory.git",
        credential_helper: "factory-agent git-credential",
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };
    let rendered2 = render_git_config(&params2);
    assert!(!rendered2.contains("fake"));

    // Brackets in section name must be skipped/filtered
    let entries3 = vec![GitConfigEntry {
        key: "bad]section.key".to_string(),
        value: "test".to_string(),
    }];
    let params3 = GitConfigParams {
        hooks_path: Path::new("/etc/factory/hooks"),
        extra_config: &entries3,
        repo_url: "https://github.com/prskr/dark-factory.git",
        credential_helper: "factory-agent git-credential",
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };
    let rendered3 = render_git_config(&params3);
    assert!(!rendered3.contains("bad]section"));
}

#[test]
fn test_config_denylist_sees_through_whitespace_and_blocks_command_keys() {
    use crate::git::is_config_entry_denied;

    // Whitespace or `=` in a key must not hide a denied key
    assert!(is_config_entry_denied("core .hooksPath", "/x"));
    assert!(is_config_entry_denied("core.hooksPath=/x", ""));
    assert!(is_config_entry_denied("remote .origin.url", "https://evil"));
    assert!(is_config_entry_denied(" core.hooksPath ", "/x"));
    assert!(is_config_entry_denied("core.hooks Path", "/x"));

    // Keys that run commands
    assert!(is_config_entry_denied(
        "interactive.diffFilter",
        "sh -c evil"
    ));
    assert!(is_config_entry_denied("mergetool.x.cmd", "evil"));
    assert!(is_config_entry_denied("difftool.x.cmd", "evil"));
    assert!(is_config_entry_denied("core.alternateRefsCommand", "evil"));
    assert!(is_config_entry_denied("browser.x.cmd", "evil"));

    assert!(!is_config_entry_denied("rebase.autoStash", "true"));
}

#[test]
fn test_render_skips_unsafe_keys_and_quotes_values() {
    use crate::git::{GitConfigParams, render_git_config};
    use df_proto::agent::v1::GitConfigEntry;

    let entries = vec![
        GitConfigEntry {
            key: "core .hooksPath".to_string(),
            value: "/data/repo/.evil".to_string(),
        },
        GitConfigEntry {
            key: "pull.rebase".to_string(),
            value: "a#b\"c\\d".to_string(),
        },
    ];
    let params = GitConfigParams {
        hooks_path: std::path::Path::new("/etc/factory/hooks"),
        extra_config: &entries,
        repo_url: "https://github.com/prskr/dark-factory.git",
        credential_helper: "factory-agent git-credential",
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };
    let rendered = render_git_config(&params);
    assert!(!rendered.contains(".evil"));
    assert!(rendered.contains(r#"rebase = "a#b\"c\\d""#));
    assert!(rendered.contains("useHttpPath = true"));
}

#[test]
fn test_rewrite_git_config_requires_real_git_dir() {
    use crate::git::{GitConfigParams, rewrite_git_config};

    let temp = tempfile::tempdir().unwrap();
    let params = GitConfigParams {
        hooks_path: std::path::Path::new("/etc/factory/hooks"),
        extra_config: &[],
        repo_url: "https://github.com/prskr/dark-factory.git",
        credential_helper: "",
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };

    // Missing .git
    let missing = temp.path().join("missing");
    std::fs::create_dir_all(&missing).unwrap();
    assert!(rewrite_git_config(&missing, &params).is_err());
    assert!(!missing.join("config").exists());

    // .git is a `gitdir:` file pointing somewhere else
    let redirected = temp.path().join("redirected");
    let elsewhere = temp.path().join("elsewhere");
    std::fs::create_dir_all(&redirected).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(
        redirected.join(".git"),
        format!("gitdir: {}", elsewhere.display()),
    )
    .unwrap();
    assert!(rewrite_git_config(&redirected, &params).is_err());
    assert!(!elsewhere.join("config").exists());

    // .git is a symlink
    let linked = temp.path().join("linked");
    std::fs::create_dir_all(&linked).unwrap();
    std::os::unix::fs::symlink(&elsewhere, linked.join(".git")).unwrap();
    assert!(rewrite_git_config(&linked, &params).is_err());
    assert!(!elsewhere.join("config").exists());
}

#[test]
fn test_credential_scope_requires_matching_path() {
    use crate::runtime_handler::matches_repo_url;

    let repo = "https://github.com/org/a.git";
    assert!(matches_repo_url(repo, "https", "github.com", "org/a"));
    assert!(!matches_repo_url(repo, "https", "github.com", "attacker/b"));
    assert!(!matches_repo_url(repo, "https", "github.com", ""));

    // A local-path repo only matches the file protocol
    assert!(!matches_repo_url(
        "/srv/repo.git",
        "https",
        "evil.example",
        "srv/repo"
    ));
    assert!(matches_repo_url(
        "/srv/repo.git",
        "file",
        "",
        "/srv/repo.git"
    ));
}

type PendingCreds = std::sync::Arc<
    tokio::sync::Mutex<
        std::collections::HashMap<
            String,
            tokio::sync::oneshot::Sender<df_proto::agent::v1::CredentialGrant>,
        >,
    >,
>;

fn cred_handler_for_test(
    repo_url: &str,
    cached: Option<(String, String)>,
) -> (
    crate::runtime_handler::DriverRuntimeHandler,
    tokio::sync::mpsc::Receiver<AgentMessage>,
    PendingCreds,
) {
    let (outbound_tx, outbound_rx) = tokio::sync::mpsc::channel(10);
    let pending_creds =
        std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let handler = crate::runtime_handler::DriverRuntimeHandler::new(
        repo_url.to_string(),
        Vec::new(),
        outbound_tx,
        std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        std::sync::Arc::clone(&pending_creds),
        std::sync::Arc::new(tokio::sync::Mutex::new(cached)),
    );
    (handler, outbound_rx, pending_creds)
}

/// Answers the next `CredentialRequest` on `outbound_rx` with `grant`.
async fn answer_credential_request(
    outbound_rx: &mut tokio::sync::mpsc::Receiver<AgentMessage>,
    pending: &PendingCreds,
    username: &str,
    expires_at: Option<i64>,
) {
    use df_proto::agent::v1::{CredentialGrant, GitCredential, credential_grant};
    let msg = tokio::time::timeout(std::time::Duration::from_secs(2), outbound_rx.recv())
        .await
        .expect("a CredentialRequest must be sent")
        .expect("channel open");
    let Some(agent_message::Body::CredentialRequest(req)) = msg.body else {
        panic!("expected CredentialRequest, got {msg:?}");
    };
    let tx = pending
        .lock()
        .await
        .remove(&req.request_id)
        .expect("pending request");
    tx.send(CredentialGrant {
        request_id: req.request_id,
        result: Some(credential_grant::Result::Git(GitCredential {
            username: username.to_string(),
            password: "pw".to_string(),
            expires_at: expires_at
                .map(|seconds| df_proto::pbjson_types::Timestamp { seconds, nanos: 0 }),
        })),
    })
    .ok();
}

#[tokio::test]
async fn test_erase_and_expiry_drop_cached_credentials() {
    use crate::runtime_socket::RuntimeHandler;

    let repo = "https://git.example.org/org/repo.git";
    let (handler, mut outbound_rx, pending) =
        cred_handler_for_test(repo, Some(("old".to_string(), "pw".to_string())));
    let handler = std::sync::Arc::new(handler);

    // Cached credentials are served without asking the control plane
    let (user, _, _) = handler
        .handle_git_credential("https", "git.example.org", "org/repo.git")
        .await;
    assert_eq!(user.as_deref(), Some("old"));
    assert!(outbound_rx.try_recv().is_err());

    // `erase` drops them, so the next `get` asks again
    handler
        .handle_git_credential_erase("https", "git.example.org", "org/repo.git")
        .await;
    let h = std::sync::Arc::clone(&handler);
    let get = tokio::spawn(async move {
        h.handle_git_credential("https", "git.example.org", "org/repo.git")
            .await
    });
    answer_credential_request(&mut outbound_rx, &pending, "fresh", None).await;
    assert_eq!(get.await.unwrap().0.as_deref(), Some("fresh"));

    // A grant that is about to expire is not served from the cache
    handler
        .handle_git_credential_erase("https", "git.example.org", "org/repo.git")
        .await;
    let h = std::sync::Arc::clone(&handler);
    let get = tokio::spawn(async move {
        h.handle_git_credential("https", "git.example.org", "org/repo.git")
            .await
    });
    answer_credential_request(
        &mut outbound_rx,
        &pending,
        "short-lived",
        Some(chrono::Utc::now().timestamp() + 5),
    )
    .await;
    assert_eq!(get.await.unwrap().0.as_deref(), Some("short-lived"));
    let h = std::sync::Arc::clone(&handler);
    let get = tokio::spawn(async move {
        h.handle_git_credential("https", "git.example.org", "org/repo.git")
            .await
    });
    answer_credential_request(&mut outbound_rx, &pending, "renewed", None).await;
    assert_eq!(get.await.unwrap().0.as_deref(), Some("renewed"));

    // Erase for another repository leaves the cache alone
    handler
        .handle_git_credential_erase("https", "git.example.org", "other/repo")
        .await;
    let (user, _, _) = handler
        .handle_git_credential("https", "git.example.org", "org/repo.git")
        .await;
    assert_eq!(user.as_deref(), Some("renewed"));
}

#[tokio::test]
async fn test_git_credential_erase_reaches_handler_over_socket() {
    use crate::runtime_socket::{RuntimeHandler, RuntimeSocketServer, run_git_credential_stream};

    struct Recorder(std::sync::Arc<std::sync::atomic::AtomicBool>);
    #[tonic::async_trait]
    impl RuntimeHandler for Recorder {
        async fn handle_git_credential(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> (Option<String>, Option<String>, Option<String>) {
            (None, None, None)
        }
        async fn handle_git_credential_erase(&self, _: &str, host: &str, _: &str) {
            assert_eq!(host, "git.example.org");
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let sock = temp.path().join("runtime.sock");
    let erased = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server = RuntimeSocketServer::bind(
        &sock,
        std::sync::Arc::new(Recorder(std::sync::Arc::clone(&erased))),
    )
    .unwrap();
    let _handle = server.spawn();

    let input = b"protocol=https\nhost=git.example.org\npath=org/repo.git\n\n";
    let mut out = Vec::new();
    run_git_credential_stream(&input[..], &mut out, &sock, Some("erase"))
        .await
        .unwrap();
    assert!(erased.load(std::sync::atomic::Ordering::SeqCst));
    assert!(out.is_empty());
}

#[test]
fn test_repo_url_with_userinfo_matches() {
    use crate::runtime_handler::matches_repo_url;

    let repo = "https://x-access-token:abc@git.example.com/org/repo.git";
    assert!(matches_repo_url(
        repo,
        "https",
        "git.example.com",
        "org/repo"
    ));
    assert!(!matches_repo_url(
        repo,
        "https",
        "evil.example.com",
        "org/repo"
    ));
}

#[test]
fn test_runtime_owned_values_are_quoted_and_validated() {
    use crate::git::{GitConfigParams, render_git_config, rewrite_git_config};

    let params = GitConfigParams {
        hooks_path: std::path::Path::new("/opt/factory/hooks-empty"),
        extra_config: &[],
        repo_url: "https://git.example.org/a.git",
        credential_helper: "helper",
        commit_author_name: "Team #1 Bot",
        commit_author_email: "bot@example.com",
    };
    assert!(render_git_config(&params).contains("name = \"Team #1 Bot\""));

    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".git")).unwrap();
    let evil = GitConfigParams {
        commit_author_name: "Bot\n[core]\nhooksPath = /x",
        ..params
    };
    assert!(rewrite_git_config(temp.path(), &evil).is_err());
}
