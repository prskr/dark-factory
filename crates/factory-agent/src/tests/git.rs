//! Git configuration denylist, planted configuration, hooks, remotes and the runtime socket mode.

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
	helper = !evil
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
        ssl_ca_info: Some(std::path::Path::new("/run/factory/tls/ca.crt")),
        extra_config: &[
            GitConfigEntry {
                key: "rebase.autoStash".to_string(),
                value: "true".to_string(),
            },
            // Planted entries in extra_config should also be stripped by denylist
            GitConfigEntry {
                key: "core.sshCommand".to_string(),
                value: "malicious-ssh".to_string(),
            },
            GitConfigEntry {
                key: "credential.helper".to_string(),
                value: "!evil".to_string(),
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
    // Only the empty helper, which resets helpers of other config scopes
    let helpers = std::process::Command::new("git")
        .args(["config", "--file"])
        .arg(git_dir.join("config"))
        .args(["--get-all", "credential.helper"])
        .output()
        .expect("git config");
    assert_eq!(String::from_utf8_lossy(&helpers.stdout), "\n");
    let ca_info_for = |url: &str| {
        let out = std::process::Command::new("git")
            .args(["config", "--file"])
            .arg(git_dir.join("config"))
            .args(["--get-urlmatch", "http.sslCAInfo", url])
            .output()
            .expect("git config");
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    assert_eq!(
        ca_info_for("https://git.example.org/peter/dark-factory.git"),
        "/run/factory/tls/ca.crt\n"
    );
    assert_eq!(ca_info_for("https://github.com/a/b.git"), "");
    assert!(!new_content.contains("[http]"));
    // Must contain hooksPath
    assert!(new_content.contains("hooksPath = /opt/factory/hooks-empty"));
    // Must contain allowed extra config
    assert!(new_content.contains("autoStash = \"true\""));

    // MUST NOT contain any planted or denylisted entries
    assert!(!new_content.contains("fsmonitor"));
    assert!(!new_content.contains("insteadOf"));
    assert!(!new_content.contains("include"));
    assert!(!new_content.contains("evil"));
    assert!(!new_content.contains("malicious"));
    assert!(!new_content.contains("sshCommand"));
}

#[test]
fn test_repo_url_cannot_break_out_of_the_config_subsection() {
    use crate::git::{GitConfigParams, render_git_config};

    let urls = [
        "https://h\"]\n[core]\n\thooksPath=/tmp/x\n[x \"/",
        "https://h\\\"]\n[core]\n\thooksPath=/tmp/x\n[x \"/",
        "https://h\"] [core] hooksPath=/tmp/x [x \"/",
        "https://h\r[core]\r\thooksPath=/tmp/x/",
        "https://h\u{0}\"]\u{b}[core]\u{c}hooksPath=/tmp/x/",
        "https://h\\x/",
        "https://h x/",
    ];
    for url in urls {
        let params = GitConfigParams {
            repo_url: url,
            commit_author_name: "Agent Bot",
            commit_author_email: "bot@darkfactory.local",
            hooks_path: std::path::Path::new("/opt/factory/hooks-empty"),
            ssl_ca_info: Some(std::path::Path::new("/run/factory/tls/ca.crt")),
            extra_config: &[],
        };
        let rendered = render_git_config(&params);

        assert!(!rendered.contains("[http"), "{url:?}: {rendered}");
        let keys: Vec<&str> = rendered
            .lines()
            .filter(|l| !l.starts_with('['))
            .filter_map(|l| l.split_once(" =").map(|(k, _)| k.trim()))
            .collect();
        assert_eq!(
            keys,
            [
                "repositoryformatversion",
                "filemode",
                "bare",
                "logallrefupdates",
                "hooksPath",
                "url",
                "fetch",
                "promisor",
                "partialclonefilter",
                "helper",
                "name",
                "email"
            ],
            "{url:?}: {rendered}"
        );
        assert!(rendered.contains("hooksPath = /opt/factory/hooks-empty\n"));
        let sections: Vec<&str> = rendered.lines().filter(|l| l.starts_with('[')).collect();
        assert_eq!(
            sections,
            ["[core]", "[remote \"origin\"]", "[credential]", "[user]"],
            "{url:?}"
        );
    }
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
    impl RuntimeHandler for DummyHandler {}

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
        ssl_ca_info: None,
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
        ssl_ca_info: None,
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
        ssl_ca_info: None,
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
        ssl_ca_info: None,
        commit_author_name: "test",
        commit_author_email: "test@example.com",
    };
    let rendered = render_git_config(&params);
    assert!(!rendered.contains(".evil"));
    assert!(rendered.contains(r#"rebase = "a#b\"c\\d""#));
    // Without a helper there is nothing to scope per path
    assert!(!rendered.contains("useHttpPath"));
    assert!(!rendered.contains("[http]"));
}

#[test]
fn test_git_commands_never_run_a_prompt_program() {
    let cmd = crate::git::Git::new("/tmp/repo").build_command();
    let env: std::collections::HashMap<_, _> = cmd.as_std().get_envs().collect();
    assert_eq!(
        env.get(std::ffi::OsStr::new("GIT_ASKPASS")),
        Some(&Some(std::ffi::OsStr::new("")))
    );
    assert_eq!(
        env.get(std::ffi::OsStr::new("GIT_TERMINAL_PROMPT")),
        Some(&Some(std::ffi::OsStr::new("0")))
    );
}

#[test]
fn test_git_commands_leave_the_ca_to_the_scoped_config() {
    // GIT_SSL_CAINFO overrides `http.<url>.sslCAInfo`, so every host would trust only the CA.
    let cmd = crate::git::Git::new("/tmp/repo")
        .with_ssl_ca_info("/run/factory/tls/ca.crt")
        .build_command();
    let env: std::collections::HashMap<_, _> = cmd.as_std().get_envs().collect();
    assert_eq!(env.get(std::ffi::OsStr::new("GIT_SSL_CAINFO")), Some(&None));
}

#[test]
fn test_clone_trusts_the_ca_only_for_the_proxy_url() {
    assert_eq!(
        crate::git::proxy_ca_override(
            "https://git.df.svc:8443/git/DF-1.git",
            Some(std::path::Path::new("/run/factory/tls/ca.crt"))
        )
        .as_deref(),
        Some("http.https://git.df.svc:8443/.sslCAInfo=/run/factory/tls/ca.crt")
    );
    assert_eq!(
        crate::git::proxy_ca_override("https://git.df.svc:8443/git/DF-1.git", None),
        None
    );
}

#[test]
fn test_rewrite_git_config_requires_real_git_dir() {
    use crate::git::{GitConfigParams, rewrite_git_config};

    let temp = tempfile::tempdir().unwrap();
    let params = GitConfigParams {
        hooks_path: std::path::Path::new("/etc/factory/hooks"),
        extra_config: &[],
        repo_url: "https://github.com/prskr/dark-factory.git",
        ssl_ca_info: None,
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
fn test_runtime_owned_values_are_quoted_and_validated() {
    use crate::git::{GitConfigParams, render_git_config, rewrite_git_config};

    let params = GitConfigParams {
        hooks_path: std::path::Path::new("/opt/factory/hooks-empty"),
        extra_config: &[],
        repo_url: "https://git.example.org/a.git",
        ssl_ca_info: None,
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

#[tokio::test]
async fn http_remotes_are_refused() {
    use crate::git::Git;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let refused = Git::new(temp_dir.path())
        .git_output(&["ls-remote", "http://127.0.0.1:1/o/r.git"])
        .await
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("transport 'http' not allowed"),
        "{refused}"
    );
}
