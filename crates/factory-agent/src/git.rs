//! Hardened Git command builder and configuration rendering.
//!
//! Follows `design/agent-run.md` §7 and `specs/git-hardening/spec.md`.

use std::path::{Path, PathBuf};
use std::process::Output;

use df_proto::agent::v1::GitConfigEntry;

pub const PROXY_ENV_VARS: &[&str] = &[
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "https_proxy",
    "http_proxy",
    "no_proxy",
];

/// Upper bound for a single git invocation (clone, fetch and push talk to a remote peer).
pub const GIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

pub const DEFAULT_COMMIT_AUTHOR_NAME: &str = "Dark Factory Agent";
pub const DEFAULT_COMMIT_AUTHOR_EMAIL: &str = "agent@dark-factory.internal";

/// Parameters for rendering the runtime-owned `.git/config`.
#[derive(Debug, Clone)]
pub struct GitConfigParams<'a> {
    pub repo_url: &'a str,
    pub commit_author_name: &'a str,
    pub commit_author_email: &'a str,
    pub hooks_path: &'a Path,
    pub credential_helper: &'a str,
    pub extra_config: &'a [GitConfigEntry],
}

/// Returns true if a Git config key or value is denied.
///
/// Denylist per `design/agent-run.md` §7.1:
/// - Programs / execution: `core.hooksPath`, `core.fsmonitor`, `core.sshCommand`, `core.gitProxy`,
///   `core.askPass`, `core.editor`, `core.pager`, `pager.*`, `sequence.editor`, `filter.*`,
///   `diff.external`, `diff.*.textconv`, `diff.*.command`, `merge.*.driver`, `gpg.*`,
///   `alias.*` values starting with `!`.
/// - Traffic / credentials: `remote.*`, `url.*`, `branch.*.remote`, `branch.*.pushRemote`,
///   `credential.*`, `http.*`, `protocol.*`.
/// - Config loading and runtime-owned: `include.*`, `includeIf.*`, `user.*`, `submodule.*`,
///   `lfs.*`, `core.worktree`, `core.bare`, `extensions.*`, `commit.gpgSign`, `tag.gpgSign`.
pub fn is_config_entry_denied(key: &str, value: &str) -> bool {
    // Check the normalized `section.name` that `render_git_config` writes, not the raw key, so
    // whitespace or `=` cannot hide a denied key. Keys that don't normalize are never written.
    let Some((section, name)) = parse_config_key(key) else {
        return true;
    };
    let lower_key = format!("{section}.{name}").to_ascii_lowercase();
    let lower_val = value.trim();

    // 1. Programs / execution
    if matches!(
        lower_key.as_str(),
        "core.hookspath"
            | "core.fsmonitor"
            | "core.sshcommand"
            | "core.gitproxy"
            | "core.askpass"
            | "core.editor"
            | "core.pager"
            | "core.alternaterefscommand"
            | "sequence.editor"
            | "diff.external"
            | "interactive.difffilter"
            | "uploadpack.packobjectshook"
            | "web.browser"
    ) {
        return true;
    }
    if lower_key.starts_with("mergetool.")
        || lower_key.starts_with("difftool.")
        || lower_key.starts_with("browser.")
        || lower_key.starts_with("man.")
    {
        return true;
    }
    if lower_key == "pager"
        || lower_key.starts_with("pager.")
        || lower_key == "filter"
        || lower_key.starts_with("filter.")
        || lower_key == "gpg"
        || lower_key.starts_with("gpg.")
    {
        return true;
    }
    if lower_key.starts_with("diff.")
        && (lower_key.ends_with(".textconv") || lower_key.ends_with(".command"))
    {
        return true;
    }
    if lower_key.starts_with("merge.") && lower_key.ends_with(".driver") {
        return true;
    }
    if (lower_key == "alias" || lower_key.starts_with("alias.")) && lower_val.starts_with('!') {
        return true;
    }

    // 2. Traffic redirection / credentials
    if lower_key == "remote"
        || lower_key.starts_with("remote.")
        || lower_key == "url"
        || lower_key.starts_with("url.")
        || lower_key == "credential"
        || lower_key.starts_with("credential.")
        || lower_key == "http"
        || lower_key.starts_with("http.")
        || lower_key == "protocol"
        || lower_key.starts_with("protocol.")
    {
        return true;
    }
    if lower_key.starts_with("branch.")
        && (lower_key.ends_with(".remote") || lower_key.ends_with(".pushremote"))
    {
        return true;
    }

    // 3. Configuration loading and runtime-owned areas
    if lower_key == "include"
        || lower_key.starts_with("include.")
        || lower_key == "includeif"
        || lower_key.starts_with("includeif.")
        || lower_key == "user"
        || lower_key.starts_with("user.")
        || lower_key == "submodule"
        || lower_key.starts_with("submodule.")
        || lower_key == "lfs"
        || lower_key.starts_with("lfs.")
        || lower_key == "extensions"
        || lower_key.starts_with("extensions.")
    {
        return true;
    }
    if matches!(
        lower_key.as_str(),
        "core.worktree" | "core.bare" | "commit.gpgsign" | "tag.gpgsign"
    ) {
        return true;
    }

    false
}

/// Splits a Git config key like `rebase.autoStash` into section `rebase` and key name `autoStash`.
///
/// Returns `None` unless the section only has `[A-Za-z0-9.-]` and the name matches
/// `[A-Za-z][A-Za-z0-9-]*`, so the pair can be written back without altering the config structure.
fn parse_config_key(full_key: &str) -> Option<(String, String)> {
    let trimmed = full_key.trim();
    let idx = trimmed.rfind('.')?;
    let section = &trimmed[..idx];
    let key = &trimmed[idx + 1..];
    let section_ok = !section.is_empty()
        && section
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    let key_ok = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    (section_ok && key_ok).then(|| (section.to_string(), key.to_string()))
}

/// Quotes a config value: wraps it in double quotes and escapes `\` and `"`.
fn quote_config_value(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Renders the runtime-owned `.git/config` contents.
pub fn render_git_config(params: &GitConfigParams<'_>) -> String {
    let mut out = String::new();

    // 1. Core section
    out.push_str("[core]\n");
    out.push_str("\trepositoryformatversion = 0\n");
    out.push_str("\tfilemode = true\n");
    out.push_str("\tbare = false\n");
    out.push_str("\tlogallrefupdates = true\n");
    out.push_str(&format!("\thooksPath = {}\n", params.hooks_path.display()));

    // 2. Extra allowed config entries (filtered through denylist)
    for entry in params.extra_config {
        if is_config_entry_denied(&entry.key, &entry.value) {
            continue;
        }
        // Reject newlines or null bytes in keys or values to prevent config injection (Finding 18)
        if entry.key.contains('\n')
            || entry.key.contains('\r')
            || entry.key.contains('\0')
            || entry.value.contains('\n')
            || entry.value.contains('\r')
            || entry.value.contains('\0')
        {
            continue;
        }
        if let Some((section, key_name)) = parse_config_key(&entry.key) {
            out.push_str(&format!(
                "[{}]\n\t{} = {}\n",
                section,
                key_name,
                quote_config_value(&entry.value)
            ));
        }
    }

    // 3. Remote origin section
    out.push_str("[remote \"origin\"]\n");
    out.push_str(&format!(
        "\turl = {}\n",
        quote_config_value(params.repo_url)
    ));
    out.push_str("\tfetch = +refs/heads/*:refs/remotes/origin/*\n");
    out.push_str("\tpromisor = true\n");
    out.push_str("\tpartialclonefilter = blob:none\n");

    // 4. Credential section
    out.push_str("[credential]\n");
    // Git only passes the repository path to the helper with this set; the runtime scopes
    // credentials to the assigned repository by path.
    out.push_str("\tuseHttpPath = true\n");
    out.push_str("\thelper = \n");
    if !params.credential_helper.is_empty() {
        out.push_str(&format!(
            "\thelper = {}\n",
            quote_config_value(params.credential_helper)
        ));
    }

    // 5. User / Commit identity section
    out.push_str("[user]\n");
    out.push_str(&format!(
        "\tname = {}\n",
        quote_config_value(params.commit_author_name)
    ));
    out.push_str(&format!(
        "\temail = {}\n",
        quote_config_value(params.commit_author_email)
    ));

    out
}

/// Replaces `.git/config` with runtime-owned configuration and deletes `.git/config.worktree`.
///
/// `.git` must be a real directory inside `repo_dir`. A `gitdir:` file, a symlink or a missing
/// `.git` is treated as tampering, because the agent could use it to redirect the write.
pub fn rewrite_git_config(repo_dir: &Path, params: &GitConfigParams<'_>) -> std::io::Result<()> {
    for value in [
        params.repo_url,
        params.credential_helper,
        params.commit_author_name,
        params.commit_author_email,
    ] {
        if value.contains(['\n', '\r', '\0']) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "git config value contains a newline or NUL",
            ));
        }
    }

    let git_dir = repo_dir.join(".git");
    if !std::fs::symlink_metadata(&git_dir)?.file_type().is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} is not a regular directory", git_dir.display()),
        ));
    }

    crate::fs_util::write_fresh(&git_dir.join("config"), render_git_config(params))?;

    let worktree_path = git_dir.join("config.worktree");
    if worktree_path.exists() {
        std::fs::remove_file(worktree_path)?;
    }

    Ok(())
}

/// Hardened Git command builder and runner.
#[derive(Debug, Clone)]
pub struct Git {
    pub repo_dir: PathBuf,
    pub hooks_empty_dir: PathBuf,
    pub global_gitconfig: PathBuf,
    pub credential_helper: String,
    pub allowed_protocols: Vec<String>,
    pub runtime_sock: Option<PathBuf>,
    pub ssl_ca_info: Option<PathBuf>,
}

impl Git {
    /// Creates a new hardened Git wrapper with production defaults.
    pub fn new(repo_dir: impl Into<PathBuf>) -> Self {
        Self {
            repo_dir: repo_dir.into(),
            hooks_empty_dir: PathBuf::from("/opt/factory/hooks-empty"),
            global_gitconfig: PathBuf::from("/opt/factory/gitconfig"),
            credential_helper: "/opt/factory/factory-agent git-credential".to_string(),
            allowed_protocols: vec!["https".to_string()],
            runtime_sock: None,
            ssl_ca_info: None,
        }
    }

    /// Sets the allowed protocols (e.g. `["https", "file"]` for tests).
    pub fn allow_protocols<I, S>(mut self, protocols: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_protocols = protocols.into_iter().map(Into::into).collect();
        self
    }

    /// Overrides the empty hooks directory (used for tests).
    pub fn with_hooks_empty_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.hooks_empty_dir = path.into();
        self
    }

    /// Overrides the global gitconfig path (used for tests).
    pub fn with_global_gitconfig(mut self, path: impl Into<PathBuf>) -> Self {
        self.global_gitconfig = path.into();
        self
    }

    /// Overrides the credential helper command (used for tests).
    pub fn with_credential_helper(mut self, helper: impl Into<String>) -> Self {
        self.credential_helper = helper.into();
        self
    }

    /// Sets the runtime socket path passed via `DF_RUNTIME_SOCK`.
    pub fn with_runtime_sock(mut self, path: impl Into<PathBuf>) -> Self {
        self.runtime_sock = Some(path.into());
        self
    }

    /// Sets the custom CA info path passed via `GIT_SSL_CAINFO`.
    #[cfg(test)]
    pub fn with_ssl_ca_info(mut self, path: impl Into<PathBuf>) -> Self {
        self.ssl_ca_info = Some(path.into());
        self
    }

    /// Builds a hardened `tokio::process::Command` targeting the given working directory.
    pub fn build_command_in(&self, cwd: &Path) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new("git");
        cmd.current_dir(cwd);
        // A timed-out or cancelled git invocation must not keep running
        cmd.kill_on_drop(true);

        // Security / hardening flags per design/agent-run.md §7.2
        cmd.arg("-c")
            .arg(format!("core.hooksPath={}", self.hooks_empty_dir.display()));
        cmd.arg("-c").arg("core.fsmonitor=false");
        cmd.arg("-c").arg("credential.useHttpPath=true");
        cmd.arg("-c").arg("credential.helper=");
        if !self.credential_helper.is_empty() {
            cmd.arg("-c")
                .arg(format!("credential.helper={}", self.credential_helper));
        }
        cmd.arg("-c").arg("protocol.allow=never");
        for proto in &self.allowed_protocols {
            cmd.arg("-c").arg(format!("protocol.{proto}.allow=always"));
        }
        cmd.arg("-c").arg("submodule.recurse=false");

        // Environment variables
        cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        cmd.env("GIT_CONFIG_GLOBAL", &self.global_gitconfig);
        cmd.env("GIT_TERMINAL_PROMPT", "0");

        // Forward proxy environment variables if present
        for var in PROXY_ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        if let Some(sock) = &self.runtime_sock {
            cmd.env("DF_RUNTIME_SOCK", sock);
        } else if let Ok(val) = std::env::var("DF_RUNTIME_SOCK") {
            cmd.env("DF_RUNTIME_SOCK", val);
        }

        if let Some(ca) = &self.ssl_ca_info {
            cmd.env("GIT_SSL_CAINFO", ca);
        } else if let Ok(val) = std::env::var("GIT_SSL_CAINFO") {
            cmd.env("GIT_SSL_CAINFO", val);
        }

        cmd
    }

    /// Builds a hardened `tokio::process::Command` in `self.repo_dir`.
    pub fn build_command(&self) -> tokio::process::Command {
        self.build_command_in(&self.repo_dir)
    }

    /// Runs a Git command and returns the raw `Output`.
    pub async fn run_cmd(&self, args: &[&str]) -> anyhow::Result<Output> {
        self.run_cmd_in(&self.repo_dir, args).await
    }

    /// Runs a Git command in a specific directory and returns the raw `Output`.
    pub async fn run_cmd_in(&self, cwd: &Path, args: &[&str]) -> anyhow::Result<Output> {
        let mut cmd = self.build_command_in(cwd);
        cmd.args(args);
        let output = tokio::time::timeout(GIT_TIMEOUT, cmd.output())
            .await
            .map_err(|_| anyhow::anyhow!("git {} timed out after {GIT_TIMEOUT:?}", args.join(" ")))?
            .map_err(|e| anyhow::anyhow!("failed to execute git {}: {e}", args.join(" ")))?;
        Ok(output)
    }

    /// Runs a Git command, verifies success, and returns stdout trimmed as `String`.
    pub async fn git_output(&self, args: &[&str]) -> anyhow::Result<String> {
        let output = self.run_cmd(args).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            anyhow::bail!(
                "git {} failed (status {:?}): {}\n{}",
                args.join(" "),
                output.status.code(),
                stderr.trim(),
                stdout.trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Returns the porcelain status of the repository.
    pub async fn git_status(&self) -> anyhow::Result<String> {
        self.git_output(&["status", "--porcelain"]).await
    }

    /// Returns true if the working tree has no uncommitted changes or untracked files.
    pub async fn is_clean(&self) -> anyhow::Result<bool> {
        let status = self.git_status().await?;
        Ok(status.is_empty())
    }

    /// Returns the current commit SHA of HEAD.
    pub async fn current_head(&self) -> anyhow::Result<String> {
        self.git_output(&["rev-parse", "HEAD"]).await
    }

    /// Counts how many commits `target_ref` is ahead of `base_ref`.
    pub async fn commits_ahead(&self, base_ref: &str, target_ref: &str) -> anyhow::Result<u32> {
        let rev_spec = format!("{base_ref}..{target_ref}");
        let count_str = self.git_output(&["rev-list", "--count", &rev_spec]).await?;
        let count = count_str
            .parse::<u32>()
            .map_err(|e| anyhow::anyhow!("invalid commit count '{count_str}': {e}"))?;
        Ok(count)
    }

    /// Pushes to a remote with an explicit refspec.
    pub async fn push(&self, remote: &str, refspec: &str, force: bool) -> anyhow::Result<Output> {
        let mut args = vec!["push", "--porcelain", remote, refspec];
        if force {
            args.push("--force");
        }
        self.run_cmd(&args).await
    }
}
