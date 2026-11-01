//! The well-known test data written by the seed program; tests refer to the same names.

/// Project of the demo flow; tasks are `DF-<n>`.
pub const PROJECT_KEY: &str = "DF";
pub const PROJECT_NAME: &str = "Demo Factory";

/// Project whose fake agent hits a rate limit; tasks are `RL-<n>`.
pub const RATE_LIMIT_PROJECT_KEY: &str = "RL";
pub const RATE_LIMIT_PROJECT_NAME: &str = "Rate Limit";

/// Project whose fake agent needs `pypi.org` (demo step 4 of iteration 2); tasks are `NA-<n>`.
pub const NETWORK_PROJECT_KEY: &str = "NA";
pub const NETWORK_PROJECT_NAME: &str = "Network Access";

/// The host the fake agent of project `NA` fetches from, and the port of its `CONNECT`.
pub const NETWORK_HOST: &str = "pypi.org";
pub const NETWORK_PORT: u16 = 443;

/// Persona that works on all projects.
pub const PERSONA_NAME: &str = "e2e";

/// Forgejo owner and repository the projects are linked to.
pub const FORGE_OWNER: &str = "factory";
pub const FORGE_REPO: &str = "demo";

/// Issue of the repository that tasks link to (created by `just e2e-local`).
pub const FORGE_ISSUE_NUMBER: u64 = 1;

/// Forgejo as sandboxes and the control plane address it: the Service in the `forgejo` namespace.
pub const FORGE_NAMESPACE: &str = "forgejo";
pub const FORGE_SERVICE: &str = "forgejo-http";
pub const FORGE_PORT: u16 = 3000;

pub const BASE_BRANCH: &str = "main";

/// Credential reference for the Forgejo bot token (Secret `forgejo-token`, key `token`).
pub const CREDENTIAL_NAME: &str = "forgejo-bot";
pub const CREDENTIAL_SECRET: &str = "forgejo-token";
pub const CREDENTIAL_KEY: &str = "token";

/// Test project image (`//deploy:test_project_image`, loaded into the kind node).
pub const PROJECT_IMAGE: &str = "ghcr.io/prskr/dark-factory/test-project:e2e";

/// Fake agent in the project image and the scripts it plays.
pub const FAKE_AGENT: &str = "/opt/harness/bin/fake-acp-agent";
pub const DEMO_SCRIPT: &str = "/opt/harness/scripts/demo.yaml";
pub const RATE_LIMIT_SCRIPT: &str = "/opt/harness/scripts/rate-limit.yaml";
pub const NETWORK_SCRIPT: &str = "/opt/harness/scripts/network-access.yaml";

/// Repository URL of the projects.
pub fn repo_url() -> String {
    format!("http://{FORGE_SERVICE}.{FORGE_NAMESPACE}.svc:{FORGE_PORT}/{FORGE_OWNER}/{FORGE_REPO}")
}

/// Command that starts the fake agent with `script`.
pub fn adapter_command(script: &str) -> Vec<String> {
    [FAKE_AGENT, "--script", script].map(str::to_owned).to_vec()
}
