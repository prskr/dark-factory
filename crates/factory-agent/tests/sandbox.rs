#![cfg(target_os = "linux")]

//! Kernel-backed checks of the `sandbox` subcommand: they run the real binary and need a kernel
//! with Landlock ABI 6 (Linux 6.12 or newer with `landlock` in `/sys/kernel/security/lsm`).
//! Without it they fail with a message naming that requirement instead of skipping.

#![expect(clippy::unwrap_used, reason = "helpers of an integration test")]

use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};

const KERNEL_REQUIREMENT: &str = "these tests need a kernel with Landlock ABI 6: Linux 6.12 or \
     newer with `landlock` in /sys/kernel/security/lsm";

/// Cargo sets `CARGO_BIN_EXE_*` at compile time; Bazel passes the path in `DF_TEST_BIN`.
fn binary() -> String {
    option_env!("CARGO_BIN_EXE_factory-agent")
        .map(str::to_string)
        .or_else(|| std::env::var("DF_TEST_BIN").ok())
        .expect("binary path (CARGO_BIN_EXE_factory-agent or DF_TEST_BIN)")
}

/// The first path component of `path`: the top-level directory that holds it.
fn top_level(path: &Path) -> Option<std::ffi::OsString> {
    path.components()
        .find_map(|c| matches!(c, Component::Normal(_)).then(|| c.as_os_str().to_owned()))
}

/// A writable directory for the layout whose top-level directory is not the one of the binary:
/// the layout's top-level directory has no rule (so `data/runtime` and `run/tls` are denied like
/// `/data` and `/run`), while the binary's must keep its read and execute rule.
fn layout_parent() -> PathBuf {
    let binary_top = top_level(Path::new(&binary()));
    let test_tmpdir = std::env::var_os("TEST_TMPDIR").map(PathBuf::from);
    ["/var/tmp", "/tmp"]
        .into_iter()
        .map(PathBuf::from)
        .chain(test_tmpdir)
        .find(|dir| top_level(dir) != binary_top && tempfile::tempdir_in(dir).is_ok())
        .expect("a writable directory outside the top-level directory of the binary")
}

/// A layout like the pod's.
struct Fixture {
    _temp: tempfile::TempDir,
    data: PathBuf,
    run: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir_in(layout_parent()).expect("temporary layout directory");
        let data = temp.path().join("data");
        let run = temp.path().join("run");
        for dir in [
            data.join("repo"),
            data.join("home"),
            data.join("runtime"),
            run.join("ca"),
            run.join("tls"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(run.join("tls/tls.key"), "secret\n").unwrap();
        std::fs::write(run.join("ca/ca.crt"), "first\n").unwrap();
        std::fs::write(data.join("runtime/outbox.dat"), "outbox\n").unwrap();
        Self {
            _temp: temp,
            data,
            run,
        }
    }

    /// A directory with full access for the harness: `/tmp`, or `/dev/shm` (below `/dev`, the other
    /// full-access directory) when the layout itself lives below `/tmp`, which then has no rule.
    fn scratch_dir(&self) -> &'static str {
        if top_level(&self.data).is_some_and(|top| top == "tmp") {
            "/dev/shm"
        } else {
            "/tmp"
        }
    }

    fn home(&self) -> PathBuf {
        self.data.join("home")
    }

    /// `factory-agent sandbox -- /bin/sh -c <script>` with the variables of a runtime.
    fn command(&self, root: &str, script: &str) -> Command {
        let mut command = Command::new(binary());
        command
            .args(["sandbox", "--root", root, "--", "/bin/sh", "-c", script])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home())
            .env("DF_DATA_DIR", &self.data)
            .env("DF_RUN_DIR", &self.run)
            .env("DF_TLS_DIR", self.run.join("tls"))
            .env("DF_CONTROL_PLANE_ADDR", "10.0.0.1:9443")
            .env("DF_CONTROL_PLANE_SERVER_NAME", "control-plane")
            .env("DF_LANDLOCK", "enforce")
            .env("DF_RUNTIME_SOCK", self.run.join("runtime.sock"))
            .env("DF_LOG_FORMAT", "json")
            .env("TLS_KEY", self.run.join("tls/tls.key"))
            .env("CA_FILE", self.run.join("ca/ca.crt"))
            .env("OUTBOX", self.data.join("runtime/outbox.dat"))
            .env("SCRATCH_DIR", self.scratch_dir())
            .env("FACTORY_BIN", binary())
            .env("RUN_DIR", &self.run)
            .env("DATA_DIR", &self.data);
        command
    }

    /// Runs `script` in the domain of the real root. Fails the test on a kernel without ABI 6.
    fn sandboxed(&self, script: &str) -> Output {
        require_landlock(self);
        self.command("/", script).output().unwrap()
    }
}

/// Whether the kernel offers Landlock ABI 6: a ruleset with its rights and scopes can be created
/// (never enforced, so the test process stays unrestricted).
fn kernel_has_landlock_abi_6() -> bool {
    use landlock::{ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, Scope};
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V6))
        .and_then(|ruleset| ruleset.scope(Scope::from_all(ABI::V6)))
        .and_then(Ruleset::create)
        .is_ok()
}

/// Fails with the kernel requirement when the kernel has no ABI 6, and with the wrapper's own
/// message when it has and the wrapper still fails to start a trivial command.
fn require_landlock(fixture: &Fixture) {
    assert!(kernel_has_landlock_abi_6(), "{KERNEL_REQUIREMENT}");
    let probe = fixture.command("/", "true").output().unwrap();
    assert!(
        probe.status.success(),
        "the sandbox wrapper failed on a kernel with Landlock ABI 6: {}",
        String::from_utf8_lossy(&probe.stderr)
    );
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn the_private_key_is_unreadable() {
    let out = Fixture::new().sandboxed(r#"cat "$TLS_KEY""#);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("Permission denied"),
        "{}",
        stderr(&out)
    );
    assert!(!stdout(&out).contains("secret"));
}

#[test]
fn the_ca_copy_stays_readable_after_it_was_replaced_by_rename() {
    let fixture = Fixture::new();
    require_landlock(&fixture);
    let mut child = fixture
        .command("/", r#"cat "$CA_FILE"; read _; cat "$CA_FILE""#)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    assert_eq!(line, "first\n");

    let ca = fixture.run.join("ca");
    std::fs::write(ca.join("ca.crt.new"), "second\n").unwrap();
    std::fs::rename(ca.join("ca.crt.new"), ca.join("ca.crt")).unwrap();
    child.stdin.take().unwrap().write_all(b"\n").unwrap();

    line.clear();
    out.read_line(&mut line).unwrap();
    assert_eq!(line, "second\n");
    assert!(child.wait().unwrap().success());
}

#[test]
fn a_symlinked_home_exits_125_without_running_the_command() {
    let fixture = Fixture::new();
    std::fs::remove_dir_all(fixture.home()).unwrap();
    std::os::unix::fs::symlink(fixture.data.join("runtime"), fixture.home()).unwrap();
    let out = fixture.command("/", "echo ran").output().unwrap();
    assert_eq!(out.status.code(), Some(125), "{}", stderr(&out));
    assert!(stderr(&out).contains("symlink"), "{}", stderr(&out));
    assert!(stdout(&out).is_empty());
}

#[test]
fn a_missing_ca_directory_exits_125_without_running_the_command() {
    let fixture = Fixture::new();
    // Without Landlock the wrapper would exit 125 for the kernel, not for the CA directory.
    require_landlock(&fixture);
    let ca_dir = fixture.run.join("ca");
    std::fs::remove_dir_all(&ca_dir).unwrap();
    let out = fixture.command("/", "echo ran").output().unwrap();
    assert_eq!(out.status.code(), Some(125), "{}", stderr(&out));
    assert!(
        stderr(&out).contains(&ca_dir.display().to_string()),
        "{}",
        stderr(&out)
    );
    assert!(stdout(&out).is_empty());
}

#[test]
fn the_runtime_directories_are_not_writable() {
    // A subshell, so the message of a failed redirection reaches stdout; the exit status of that
    // failure differs between shells (dash 2, bash 1), so only success or failure counts.
    let out = Fixture::new().sandboxed(
        r#"( echo x >> "$OUTBOX" ) 2>&1 && echo outbox=written || echo outbox=denied
        ( echo x > "$DATA_DIR/runtime/new" ) 2>&1 && echo new=written || echo new=denied
        ( echo x > "$RUN_DIR/runtime.sock" ) 2>&1 && echo run=written || echo run=denied"#,
    );
    let text = stdout(&out);
    for check in ["outbox=denied", "new=denied", "run=denied"] {
        assert!(text.contains(check), "{check} in {text}");
    }
    assert_eq!(text.matches("Permission denied").count(), 3, "{text}");
}

#[test]
fn workspace_home_and_tmp_are_writable_and_symlinks_inside_them_work() {
    let fixture = Fixture::new();
    let out = fixture.sandboxed(
        r#"set -e
        echo a > "$DATA_DIR/repo/file"
        echo b > "$HOME/.cache-file"
        scratch=$(mktemp "$SCRATCH_DIR/df-landlock-XXXXXX")
        echo c > "$scratch"
        rm "$scratch"
        ln -s "$HOME/.cache-file" "$DATA_DIR/repo/bazel-bin"
        cat "$DATA_DIR/repo/bazel-bin""#,
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "b\n");
}

#[test]
fn system_directories_are_read_only_but_binaries_run() {
    let out = Fixture::new().sandboxed(
        r"touch /usr/local/bin/x 2>&1 && echo usr=written || echo usr=denied
        touch /etc/x 2>&1 && echo etc=written || echo etc=denied
        /usr/bin/env true && echo env=ran || echo env=failed",
    );
    let text = stdout(&out);
    for check in ["usr=denied", "etc=denied", "env=ran"] {
        assert!(text.contains(check), "{check} in {text}");
    }
}

#[test]
fn an_alias_of_an_outbox_file_cannot_be_created_in_home() {
    let out = Fixture::new().sandboxed(r#"ln "$OUTBOX" "$HOME/alias" 2>&1; echo "ln=$?""#);
    assert!(!stdout(&out).contains("ln=0"), "{}", stdout(&out));
}

#[test]
fn signals_to_the_parent_are_refused() {
    // SIGCONT does nothing to a running process, so a failure of the scope is harmless.
    let out = Fixture::new().sandboxed(r#"kill -CONT "$PPID" 2>&1; echo "kill=$?""#);
    let text = stdout(&out);
    assert!(!text.contains("kill=0"), "{text}");
    assert!(text.contains("not permitted"), "{text}");
}

#[test]
fn the_command_does_not_see_the_runtime_variables_but_keeps_the_others() {
    let out = Fixture::new().sandboxed("env");
    let env = stdout(&out);
    for name in [
        "DF_CONTROL_PLANE_ADDR",
        "DF_CONTROL_PLANE_SERVER_NAME",
        "DF_TLS_DIR",
        "DF_LANDLOCK",
    ] {
        assert!(!env.contains(name), "{name} in {env}");
    }
    for name in ["DF_RUNTIME_SOCK=", "DF_RUN_DIR=", "DF_LOG_FORMAT=", "PATH="] {
        assert!(env.contains(name), "{name} in {env}");
    }
}

#[test]
fn the_command_inherits_only_the_standard_descriptors() {
    let fixture = Fixture::new();
    require_landlock(&fixture);
    // The wrapper is started by a shell that holds descriptors 7 and 8 open without close-on-exec,
    // as a job server of a test runner would. `ls` lists the descriptors of the shell the wrapper
    // became, so the listing's own descriptor is not among them. The trailing `true` keeps `ls`
    // from being the last command: bash as `/bin/sh` would exec it, making `$$` the pid of `ls`.
    let inner = fixture.command("/", r"ls /proc/$$/fd; true");
    let mut outer = Command::new("/bin/sh");
    outer
        .args(["-c", r#"exec 7</dev/null 8</dev/null; exec "$@""#, "sh"])
        .arg(inner.get_program())
        .args(inner.get_args())
        .env_clear();
    for (name, value) in inner.get_envs() {
        if let Some(value) = value {
            outer.env(name, value);
        }
    }
    let out = outer.output().unwrap();

    let mut fds: Vec<String> = stdout(&out).lines().map(str::to_string).collect();
    fds.sort();
    assert_eq!(fds, ["0", "1", "2"], "{}", stderr(&out));
}

/// A top-level symlink to `/` in the root listing must not become a grant on `/`: without it,
/// nothing allows `/usr`, so executing `/usr/bin/env` is denied.
#[test]
fn a_top_level_symlink_to_the_root_grants_nothing() {
    let fixture = Fixture::new();
    require_landlock(&fixture);
    let fake_root = fixture.data.parent().unwrap();
    std::os::unix::fs::symlink("/", fake_root.join("everything")).unwrap();
    let out = Command::new(binary())
        .args(["sandbox", "--root"])
        .arg(fake_root)
        .args(["--", "/usr/bin/env", "true"])
        .env_clear()
        .env("DF_DATA_DIR", &fixture.data)
        .env("DF_RUN_DIR", &fixture.run)
        .env("DF_TLS_DIR", fixture.run.join("tls"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(125), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("Permission denied"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn a_binary_below_a_project_specific_top_level_directory_executes() {
    // The binary lives below the checkout or the Bazel cache, not below `/usr`: only the read and
    // execute rule of its own top-level directory lets it run.
    let out = Fixture::new().sandboxed(r#""$FACTORY_BIN" --version; echo "rc=$?""#);
    let text = stdout(&out);
    assert!(text.contains("factory-agent"), "{text} {}", stderr(&out));
    assert!(text.contains("rc=0"), "{text} {}", stderr(&out));
}

#[test]
fn the_memory_and_environment_of_the_parent_process_are_unreadable() {
    let out = Fixture::new().sandboxed(
        r#"cat "/proc/$PPID/environ" >/dev/null 2>&1; echo "environ=$?"
        head -c 1 "/proc/$PPID/mem" >/dev/null 2>"$HOME/mem-error"; echo "mem=$?"
        cat "$HOME/mem-error""#,
    );
    let text = stdout(&out);
    assert!(!text.contains("environ=0"), "{text}");
    assert!(!text.contains("mem=0"), "{text}");
    // Opening is denied; a failed read of an unmapped address would say `Input/output error`.
    assert!(!text.contains("Input/output error"), "{text}");
}

/// A top-level directory the user can't open (`/lost+found`, a `0700` `/root`) gets no rule instead
/// of aborting the domain. The fake root can be listed but not entered, so none of its entries can
/// be opened; the domain still applies and only the exec of the command fails (nothing allows
/// `/usr`), which is a different failure from "cannot apply".
#[test]
fn a_top_level_directory_that_cannot_be_opened_does_not_abort_the_domain() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    require_landlock(&fixture);
    let fake_root = tempfile::tempdir_in(layout_parent()).unwrap();
    std::fs::create_dir(fake_root.path().join("lost+found")).unwrap();
    std::fs::set_permissions(fake_root.path(), std::fs::Permissions::from_mode(0o444)).unwrap();

    let out = Command::new(binary())
        .args(["sandbox", "--root"])
        .arg(fake_root.path())
        .args(["--", "/usr/bin/env", "true"])
        .env_clear()
        .env("DF_DATA_DIR", &fixture.data)
        .env("DF_RUN_DIR", &fixture.run)
        .env("DF_TLS_DIR", fixture.run.join("tls"))
        .output()
        .unwrap();

    // Let the temporary directory be removed.
    std::fs::set_permissions(fake_root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = stderr(&out);
    assert_eq!(out.status.code(), Some(125), "{error}");
    assert!(error.contains("cannot start"), "{error}");
    assert!(!error.contains("cannot apply"), "{error}");
}
