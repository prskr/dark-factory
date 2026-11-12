//! Landlock confinement of the harness: the `sandbox` subcommand applies the domain to itself and
//! `exec`s the harness command, so the runtime stays outside (design D1 to D3).
//!
//! Setup failures end the process with [`SETUP_FAILED`] before the command runs; the harness
//! never runs unrestricted by accident.

use std::ffi::{OsStr, OsString};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{ENV_LOG_FORMAT, ENV_RUN_DIR, ENV_RUNTIME_SOCK, Layout};

pub mod aliases;
#[cfg(target_os = "linux")]
mod linux;
pub mod probe;
pub mod ruleset;

/// Closes the inherited descriptors and applies the Landlock domain. Landlock exists on Linux
/// only; elsewhere the wrapper fails like on a kernel without it.
#[cfg(target_os = "linux")]
use linux::apply_domain;

#[cfg(not(target_os = "linux"))]
fn apply_domain(_root: &Path, _layout: &Layout) -> anyhow::Result<()> {
    anyhow::bail!("Landlock is only available on Linux")
}

/// Exit code of the `sandbox` subcommand when the domain could not be applied or the command not
/// started.
pub const SETUP_FAILED: u8 = 125;

/// `DF_*` variables the harness keeps: the MCP server it starts needs the socket and the run
/// directory, and its logs follow the runtime's format.
const KEPT_ENV: [&str; 3] = [ENV_RUNTIME_SOCK, ENV_RUN_DIR, ENV_LOG_FORMAT];

/// The command line that starts `command` through the `sandbox` subcommand of `runtime_exe`.
pub fn wrap_command(runtime_exe: &Path, command: &str, args: &[String]) -> (PathBuf, Vec<String>) {
    let mut wrapped = vec!["sandbox".to_string(), "--".to_string(), command.to_string()];
    wrapped.extend_from_slice(args);
    (runtime_exe.to_path_buf(), wrapped)
}

/// Whether `name` is one of the runtime's own `DF_*` variables, which the harness must not see.
pub fn is_hidden_runtime_var(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with("DF_") && !KEPT_ENV.contains(&name.as_ref())
}

/// The variables among `names` the harness must not see.
pub fn hidden_runtime_vars(names: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    names
        .into_iter()
        .filter(|name| is_hidden_runtime_var(name))
        .collect()
}

/// Applies the domain and replaces the process with `command`. Only returns on failure, with the
/// reason.
pub fn run(root: &Path, command: &[OsString]) -> anyhow::Error {
    let Some((program, args)) = command.split_first() else {
        return anyhow::anyhow!("no command given");
    };
    if let Err(error) = apply_domain(root, &Layout::from_env()) {
        return error;
    }
    let mut process = Command::new(program);
    process.args(args);
    for name in hidden_runtime_vars(std::env::vars_os().map(|(name, _)| name)) {
        process.env_remove(name);
    }
    let error = process.exec();
    anyhow::Error::new(error).context(format!("cannot start {}", program.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wrapper_runs_the_runtime_itself_with_the_command_after_the_separator() {
        let (program, args) = wrap_command(
            Path::new("/opt/factory/bin/factory-agent"),
            "/opt/harness/bin/node",
            &["index.js".to_string()],
        );
        assert_eq!(program, Path::new("/opt/factory/bin/factory-agent"));
        assert_eq!(args, ["sandbox", "--", "/opt/harness/bin/node", "index.js"]);
    }

    #[test]
    fn an_empty_command_is_a_setup_failure() {
        assert!(run(Path::new("/"), &[]).to_string().contains("no command"));
    }

    #[test]
    fn only_runtime_variables_outside_the_kept_list_are_hidden_from_the_harness() {
        let names = [
            "DF_CONTROL_PLANE_ADDR",
            "DF_CONTROL_PLANE_SERVER_NAME",
            "DF_TLS_DIR",
            "DF_LANDLOCK",
            "DF_DATA_DIR",
            "DF_RUNTIME_SOCK",
            "DF_RUN_DIR",
            "DF_LOG_FORMAT",
            "PATH",
            "JAVA_HOME",
        ]
        .map(OsString::from);

        let hidden = hidden_runtime_vars(names);

        assert_eq!(
            hidden,
            [
                "DF_CONTROL_PLANE_ADDR",
                "DF_CONTROL_PLANE_SERVER_NAME",
                "DF_TLS_DIR",
                "DF_LANDLOCK",
                "DF_DATA_DIR"
            ]
            .map(OsString::from)
        );
    }
}
