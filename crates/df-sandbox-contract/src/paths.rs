//! Paths inside the sandbox that the control plane and the sandbox runtime agree on.

/// The persistent volume of the sandbox, and its directories the harness may write: the workspace
/// checkout and `$HOME`.
pub const SANDBOX_DATA_DIR: &str = "/data";
pub const SANDBOX_REPO_DIR: &str = "/data/repo";
pub const SANDBOX_HOME_DIR: &str = "/data/home";

/// The ephemeral runtime directory of the sandbox (runtime socket, CA copy).
pub const SANDBOX_RUN_DIR: &str = "/run/factory";

/// The runtime socket the factory MCP server of the harness connects to.
pub const SANDBOX_RUNTIME_SOCK: &str = "/run/factory/runtime.sock";

/// Where the sandbox pod mounts its TLS secret.
pub const SANDBOX_TLS_DIR: &str = "/run/factory/tls";

/// Where the runtime keeps a copy of `ca.crt` for the harness, which may not read
/// [`SANDBOX_TLS_DIR`] (it holds the private key).
pub const SANDBOX_CA_DIR: &str = "/run/factory/ca";

/// The instance CA in the sandbox, in [`SANDBOX_CA_DIR`]. The gateway's certificate (Git and
/// inference endpoint) is signed by it.
pub const INSTANCE_CA_PATH: &str = "/run/factory/ca/ca.crt";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_instance_ca_lies_in_the_directory_of_the_runtime_maintained_copy() {
        assert_eq!(INSTANCE_CA_PATH, format!("{SANDBOX_CA_DIR}/ca.crt"));
        assert_ne!(SANDBOX_CA_DIR, SANDBOX_TLS_DIR);
    }

    #[test]
    fn the_runtime_socket_lies_in_the_run_directory() {
        assert_eq!(
            SANDBOX_RUNTIME_SOCK,
            format!("{SANDBOX_RUN_DIR}/runtime.sock")
        );
    }
}
