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

/// Delays before the retries of a clone, fetch or push that failed transiently
/// (`specs/workspace-lifecycle/spec.md`, "Transient Git failures are retried").
pub const GIT_RETRY_DELAYS: [std::time::Duration; 3] = [
    std::time::Duration::from_secs(1),
    std::time::Duration::from_secs(5),
    std::time::Duration::from_secs(15),
];

/// Output markers of failures that are final even when a transient marker shows up as well:
/// proxy or forge rejections and 502 (the forge rejected the proxy's credential). A 403 (policy)
/// is final too, except for [`UNKNOWN_SOURCE_LINE`].
const PERMANENT_FAILURE_MARKERS: &[&str] =
    &["[remote rejected]", "[rejected]", "returned error: 502"];

const FORBIDDEN_MARKER: &str = "returned error: 403";

/// Output markers of a connection error or HTTP 503 (gateway pod without a snapshot, forge
/// unreachable). DNS failures are not listed: the proxy host is a `hostAliases` entry.
///
/// Connection setup: `Failed to connect to`, `Connection refused`, `Empty reply from server`.
/// Mid-transfer (curl errors 18, 28, 35/56, 92): `Connection reset by peer`,
/// `Operation timed out`, `transfer closed with outstanding read data remaining`,
/// `SSL_ERROR_SYSCALL`, `Recv failure`, `was not closed cleanly` (HTTP/2 stream reset).
const TRANSIENT_FAILURE_MARKERS: &[&str] = &[
    "returned error: 503",
    "Failed to connect to",
    "Connection refused",
    "Connection reset by peer",
    "Empty reply from server",
    "Operation timed out",
    "transfer closed with outstanding read data remaining",
    "SSL_ERROR_SYSCALL",
    "Recv failure",
    "was not closed cleanly",
];

/// The 403 body the gateway sends while it has no snapshot with the sandbox's pod IP yet
/// (`df-gateway` `git/server.rs`), as git prints it.
const UNKNOWN_SOURCE_LINE: &str = "remote: df: unknown source";

/// Returns true if the output (stdout and stderr) of a failed git network operation shows a
/// transient failure worth retrying.
///
/// Only git's own lines count: `remote:` lines carry text from the server (or the repository's
/// hooks) and must not trigger a retry. The one exception is the gateway's 403
/// [`UNKNOWN_SOURCE_LINE`], which is transient together with git's `returned error: 403`.
pub fn is_transient_git_failure(output: &str) -> bool {
    let own_lines: Vec<&str> = output
        .lines()
        .filter(|l| !l.trim_start().starts_with("remote:"))
        .collect();
    let has = |marker: &str| own_lines.iter().any(|l| l.contains(marker));
    if PERMANENT_FAILURE_MARKERS.iter().any(|m| has(m)) {
        return false;
    }
    if has(FORBIDDEN_MARKER) {
        return output.lines().any(|l| l.trim() == UNKNOWN_SOURCE_LINE);
    }
    TRANSIENT_FAILURE_MARKERS.iter().any(|m| has(m))
}

pub const DEFAULT_COMMIT_AUTHOR_NAME: &str = "Dark Factory Agent";
pub const DEFAULT_COMMIT_AUTHOR_EMAIL: &str = "agent@dark-factory.internal";

/// Parameters for rendering the runtime-owned `.git/config`.
#[derive(Debug, Clone)]
pub struct GitConfigParams<'a> {
    pub repo_url: &'a str,
    pub commit_author_name: &'a str,
    pub commit_author_email: &'a str,
    pub hooks_path: &'a Path,
    /// CA bundle that verifies the gateway's Git proxy certificate (`http.sslCAInfo`).
    pub ssl_ca_info: Option<&'a Path>,
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

/// Quotes a config value: wraps it in double quotes, escapes `\`, `"`, newline and tab, and drops
/// other control characters, so the value stays on its line.
fn quote_config_value(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
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
    out.extend([
        "\thooksPath = ",
        &params.hooks_path.display().to_string(),
        "\n",
    ]);

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
            out.extend([
                "[",
                &section,
                "]\n\t",
                &key_name,
                " = ",
                &quote_config_value(&entry.value),
                "\n",
            ]);
        }
    }

    // 3. Remote origin section
    out.push_str("[remote \"origin\"]\n");
    out.extend(["\turl = ", &quote_config_value(params.repo_url), "\n"]);
    out.push_str("\tfetch = +refs/heads/*:refs/remotes/origin/*\n");
    out.push_str("\tpromisor = true\n");
    out.push_str("\tpartialclonefilter = blob:none\n");

    // 4. Credentials: the gateway's Git proxy authenticates upstream, so no helper runs here.
    // The empty value resets helpers from any other config scope.
    out.push_str("[credential]\n");
    out.push_str("\thelper = \n");
    if let (Some(ca), Some(prefix)) = (params.ssl_ca_info, url_prefix(params.repo_url)) {
        // Scoped to the proxy, so other HTTPS remotes keep the system trust store.
        out.extend(["[http \"", &prefix, "\"]\n"]);
        out.extend([
            "\tsslCAInfo = ",
            &quote_config_value(&ca.display().to_string()),
            "\n",
        ]);
    }

    // 5. User / Commit identity section
    out.push_str("[user]\n");
    out.extend([
        "\tname = ",
        &quote_config_value(params.commit_author_name),
        "\n",
    ]);
    out.extend([
        "\temail = ",
        &quote_config_value(params.commit_author_email),
        "\n",
    ]);

    out
}

/// `http.<proxy URL prefix>.sslCAInfo=<ca>` for `git -c`, so a clone trusts `ca` for the proxy
/// only, before `.git/config` exists. `None` without a CA or a usable `repo_url`.
pub fn proxy_ca_override(repo_url: &str, ca: Option<&Path>) -> Option<String> {
    let prefix = url_prefix(repo_url)?;
    Some(format!("http.{prefix}.sslCAInfo={}", ca?.display()))
}

/// `scheme://host[:port]/` of `url` (user info dropped), or `None` if `url` has no authority or
/// contains characters that would need escaping in a config subsection.
fn url_prefix(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split('/').next()?;
    let host_port = authority.rsplit('@').next()?;
    let valid = |s: &str| {
        !s.is_empty()
            && !s
                .chars()
                .any(|c| c == '"' || c == '\\' || c.is_whitespace() || c.is_control())
    };
    (valid(scheme) && valid(host_port)).then(|| format!("{scheme}://{host_port}/"))
}

/// Replaces `.git/config` with runtime-owned configuration and deletes `.git/config.worktree`.
///
/// `.git` must be a real directory inside `repo_dir`. A `gitdir:` file, a symlink or a missing
/// `.git` is treated as tampering, because the agent could use it to redirect the write.
pub fn rewrite_git_config(repo_dir: &Path, params: &GitConfigParams<'_>) -> std::io::Result<()> {
    let ca = params
        .ssl_ca_info
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    for value in [
        params.repo_url,
        &ca,
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
    pub allowed_protocols: Vec<String>,
    pub ssl_ca_info: Option<PathBuf>,
    pub program: PathBuf,
    pub retry_delays: Vec<std::time::Duration>,
}

impl Git {
    /// Creates a new hardened Git wrapper with production defaults.
    pub fn new(repo_dir: impl Into<PathBuf>) -> Self {
        Self {
            repo_dir: repo_dir.into(),
            hooks_empty_dir: PathBuf::from("/opt/factory/hooks-empty"),
            global_gitconfig: PathBuf::from("/opt/factory/gitconfig"),
            allowed_protocols: vec!["https".to_string()],
            ssl_ca_info: None,
            program: PathBuf::from("git"),
            retry_delays: GIT_RETRY_DELAYS.to_vec(),
        }
    }

    /// Overrides the git executable (used for tests).
    #[must_use]
    pub fn with_program(mut self, program: impl Into<PathBuf>) -> Self {
        self.program = program.into();
        self
    }

    /// Overrides the delays between retries of transient failures (used for tests).
    #[must_use]
    pub fn with_retry_delays(mut self, delays: Vec<std::time::Duration>) -> Self {
        self.retry_delays = delays;
        self
    }

    /// Sets the allowed protocols (e.g. `["https", "file"]` for tests).
    #[must_use]
    pub fn allow_protocols<I, S>(mut self, protocols: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_protocols = protocols.into_iter().map(Into::into).collect();
        self
    }

    /// Overrides the empty hooks directory (used for tests).
    #[must_use]
    pub fn with_hooks_empty_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.hooks_empty_dir = path.into();
        self
    }

    /// Overrides the global gitconfig path (used for tests).
    #[must_use]
    pub fn with_global_gitconfig(mut self, path: impl Into<PathBuf>) -> Self {
        self.global_gitconfig = path.into();
        self
    }

    /// Sets the CA bundle that verifies the proxy (the instance CA in production); written to
    /// `http.<proxy URL>.sslCAInfo`, never to `GIT_SSL_CAINFO`.
    #[must_use]
    pub fn with_ssl_ca_info(mut self, path: impl Into<PathBuf>) -> Self {
        self.ssl_ca_info = Some(path.into());
        self
    }

    /// Builds a hardened `tokio::process::Command` targeting the given working directory.
    pub fn build_command_in(&self, cwd: &Path) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.current_dir(cwd);
        // A timed-out or cancelled git invocation must not keep running
        cmd.kill_on_drop(true);

        // Security / hardening flags per design/agent-run.md §7.2
        cmd.arg("-c")
            .arg(format!("core.hooksPath={}", self.hooks_empty_dir.display()));
        cmd.arg("-c").arg("core.fsmonitor=false");
        cmd.arg("-c").arg("credential.helper=");
        cmd.arg("-c").arg("protocol.allow=never");
        for proto in &self.allowed_protocols {
            cmd.arg("-c").arg(format!("protocol.{proto}.allow=always"));
        }
        cmd.arg("-c").arg("submodule.recurse=false");

        // Environment variables
        cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        cmd.env("GIT_CONFIG_GLOBAL", &self.global_gitconfig);
        cmd.env("GIT_TERMINAL_PROMPT", "0");
        // An empty GIT_ASKPASS also overrides core.askPass and SSH_ASKPASS: no prompt program runs.
        cmd.env("GIT_ASKPASS", "");

        // Forward proxy environment variables if present
        for var in PROXY_ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        // Git needs no runtime socket: the gateway authenticates upstream.
        cmd.env_remove("DF_RUNTIME_SOCK");

        // It would override the scoped `http.<proxy URL>.sslCAInfo`: other hosts keep the
        // system trust store.
        cmd.env_remove("GIT_SSL_CAINFO");

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
            .map_err(|_elapsed| {
                anyhow::anyhow!("git {} timed out after {GIT_TIMEOUT:?}", args.join(" "))
            })?
            .map_err(|e| anyhow::anyhow!("failed to execute git {}: {e}", args.join(" ")))?;
        Ok(output)
    }

    /// Runs a Git network operation (clone, fetch) in `cwd`, retrying transient failures.
    pub async fn run_network_cmd_in(&self, cwd: &Path, args: &[&str]) -> anyhow::Result<Output> {
        self.run_with_retry(cwd, args, None).await
    }

    /// Runs a Git network operation in `self.repo_dir`, retrying transient failures.
    pub async fn run_network_cmd(&self, args: &[&str]) -> anyhow::Result<Output> {
        self.run_with_retry(&self.repo_dir, args, None).await
    }

    /// Runs `args` and retries it after each delay of `retry_delays` while it fails transiently.
    ///
    /// With a push target (`remote`, `refspec`) the remote ref is re-read before each retry; if it
    /// already has the pushed commit, the previous attempt's response was lost after the ref
    /// update and the push counts as successful.
    async fn run_with_retry(
        &self,
        cwd: &Path,
        args: &[&str],
        push_target: Option<(&str, &str)>,
    ) -> anyhow::Result<Output> {
        let mut delays = self.retry_delays.iter();
        loop {
            let output = self.run_cmd_in(cwd, args).await?;
            if output.status.success() || !is_transient_git_failure(&combined_output(&output)) {
                return Ok(output);
            }
            let Some(delay) = delays.next() else {
                return Ok(output);
            };
            tracing::warn!(
                operation = args.first().copied().unwrap_or_default(),
                delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                "transient git failure, retrying"
            );
            tokio::time::sleep(*delay).await;
            if let Some((remote, refspec)) = push_target
                && self.remote_ref_has_pushed_commit(remote, refspec).await
            {
                return Ok(already_pushed_output(refspec));
            }
        }
    }

    /// Returns true if the destination of `refspec` on `remote` already equals its source commit
    /// (or is absent for a delete). Any failure to read either side counts as false.
    async fn remote_ref_has_pushed_commit(&self, remote: &str, refspec: &str) -> bool {
        let refspec = refspec.trim_start_matches('+');
        let (src, dst) = refspec.split_once(':').unwrap_or((refspec, refspec));
        let dst = if dst.starts_with("refs/") {
            dst.to_string()
        } else {
            format!("refs/heads/{dst}")
        };
        let Ok(listing) = self.git_output(&["ls-remote", remote, &dst]).await else {
            return false;
        };
        let remote_sha = listing
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .find(|(_, name)| *name == dst)
            .map(|(sha, _)| sha.to_string());
        if src.is_empty() {
            return remote_sha.is_none();
        }
        let Ok(local_sha) = self
            .git_output(&["rev-parse", "--verify", &format!("{src}^{{commit}}")])
            .await
        else {
            return false;
        };
        remote_sha.as_deref() == Some(local_sha.as_str())
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

    /// Pushes to a remote with an explicit refspec, retrying transient failures.
    pub async fn push(&self, remote: &str, refspec: &str, force: bool) -> anyhow::Result<Output> {
        let mut args = vec!["push", "--porcelain", remote, refspec];
        if force {
            args.push("--force");
        }
        self.run_with_retry(&self.repo_dir, &args, Some((remote, refspec)))
            .await
    }
}

/// Stdout and stderr of a git invocation (`push --porcelain` reports rejections on stdout).
fn combined_output(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A successful push output for a push whose ref update reached the remote before its response
/// was lost.
fn already_pushed_output(refspec: &str) -> Output {
    use std::os::unix::process::ExitStatusExt;
    Output {
        status: std::process::ExitStatus::from_raw(0),
        stdout: format!("=\t{refspec}\t[up to date]\nDone\n").into_bytes(),
        stderr: Vec::new(),
    }
}
