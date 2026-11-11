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
/// What the first turn of the `NA` script says before its fetch; the proxy then denies the
/// fetch, retried once a second for up to 240 s, until the request is decided.
pub const NETWORK_NEED: &str = "I need the package index from pypi.org.";

/// Project whose fake agent commits a test token (demo step 2 of iteration 2); tasks are `SC-<n>`.
pub const SECRET_PROJECT_KEY: &str = "SC";
pub const SECRET_PROJECT_NAME: &str = "Secret Scan";

/// Project whose fake agent edits the CI workflow (demo step 3 of iteration 2); tasks are `PP-<n>`.
pub const PROTECTED_PROJECT_KEY: &str = "PP";
pub const PROTECTED_PROJECT_NAME: &str = "Protected Path";

/// Project that hides `secret/**` and whose fake agent tries to delete `secret/value.txt` (demo
/// step 6 of iteration 3); tasks are `HP-<n>`.
pub const HIDDEN_PROJECT_KEY: &str = "HP";
pub const HIDDEN_PROJECT_NAME: &str = "Hidden Path";

/// The hidden glob of project `HP` and the file of the repository below it
/// (`deploy/e2e/e2e-local.sh` commits it to `main`).
pub const HIDDEN_GLOB: &str = "secret/**";
pub const HIDDEN_PATH: &str = "secret/value.txt";

/// Project whose fake agent completes the task in its first run and only reads feedback in the
/// next (forge webhooks); tasks are `WH-<n>`.
pub const WEBHOOK_PROJECT_KEY: &str = "WH";
pub const WEBHOOK_PROJECT_NAME: &str = "Webhooks";

/// The webhook secret of the `forgejo` plugin and of the repository webhook (`webhook_secret` of
/// `deploy/e2e/e2e-local.sh`).
pub const WEBHOOK_SECRET: &str = "e2e-webhook-secret";

/// Project whose fake agent delegates (`create_task`, `send_message`) without any webhook; tasks
/// are `DG-<n>`.
pub const DELEGATION_PROJECT_KEY: &str = "DG";
pub const DELEGATION_PROJECT_NAME: &str = "Delegation";

/// Project whose fake agents hand a result over as an artifact (demo step 5 of iteration 3);
/// tasks are `RS-<n>`.
pub const RESEARCH_PROJECT_KEY: &str = "RS";
pub const RESEARCH_PROJECT_NAME: &str = "Research";

/// Project whose fake agent keeps its first turn open until it is interrupted (`interrupt-turn`);
/// tasks are `IN-<n>`.
pub const INTERRUPT_PROJECT_KEY: &str = "IN";
pub const INTERRUPT_PROJECT_NAME: &str = "Interrupt";

/// What the first turn of the `IN` script says before it waits, and what the turn after the
/// interrupt says about its input.
pub const INTERRUPT_STARTED: &str = "Starting a long task.";
pub const INTERRUPT_NOTED: &str = "Noted the interruption.";
pub const INTERRUPT_SWITCHED: &str = "Switched to the other API.";

/// The artifact the researcher of project `RS` writes, and a line of it.
pub const RESEARCH_ARTIFACT: &str = "findings.md";
pub const RESEARCH_FINDING: &str = "The session cookie is dropped on the redirect.";

/// The file of `push-secret.yaml` with the token, and the line the token is on.
pub const SECRET_PATH: &str = "config.py";
pub const SECRET_LINE: u32 = 2;

/// The workflow `push-protected.yaml` adds.
pub const PROTECTED_PATH: &str = ".forgejo/workflows/ci.yml";

/// Persona that works on all projects.
pub const PERSONA_NAME: &str = "e2e";

/// The model of the fake provider that the persona selects.
pub const PERSONA_MODEL: &str = "claude-sonnet-5-5";

/// Forgejo owner and repository the projects are linked to.
pub const FORGE_OWNER: &str = "factory";
pub const FORGE_REPO: &str = "demo";

/// Second Forgejo user with write access to the repository (created by `just e2e-local`).
pub const FORGE_REVIEWER: &str = "reviewer";

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

/// The model provider of the persona: the fake Anthropic upstream in the `fake-upstream`
/// namespace (plain HTTP, the cluster allows it like for Forgejo), and the credential reference
/// the gateway sends to it (Secret `fake-llm-key`, key `key`).
pub const MODEL_PROVIDER_NAME: &str = "Fake Anthropic";
/// A second provider no persona uses, for the tests of the settings page: the provider form
/// requires HTTPS upstream URLs, so a save works only on a provider that has one. Never called.
pub const SETTINGS_PROVIDER_NAME: &str = "Settings Anthropic";
pub const SETTINGS_PROVIDER_URL: &str = "https://anthropic.example.org";
pub const MODEL_CREDENTIAL_NAME: &str = "fake-llm";
pub const MODEL_CREDENTIAL_SECRET: &str = "fake-llm-key";
pub const MODEL_CREDENTIAL_KEY: &str = "key";
pub const FAKE_UPSTREAM_NAMESPACE: &str = "fake-upstream";
pub const FAKE_UPSTREAM_SERVICE: &str = "fake-anthropic";
pub const FAKE_UPSTREAM_PORT: u16 = 8080;
/// The key the fake upstream expects, and the placeholder the sandbox must never get.
pub const MODEL_API_KEY: &str = "e2e-model-api-key";

/// Base URL of the fake upstream as the gateway addresses it.
pub fn fake_upstream_url() -> String {
    format!("http://{FAKE_UPSTREAM_SERVICE}.{FAKE_UPSTREAM_NAMESPACE}.svc:{FAKE_UPSTREAM_PORT}")
}

/// Test project image (`//deploy:test_project_image`, loaded into the kind node).
pub const PROJECT_IMAGE: &str = "ghcr.io/prskr/dark-factory/test-project:e2e";

/// Fake agent in the project image and the scripts it plays.
pub const FAKE_AGENT: &str = "/opt/harness/bin/fake-acp-agent";
pub const DEMO_SCRIPT: &str = "/opt/harness/scripts/demo.yaml";
pub const RATE_LIMIT_SCRIPT: &str = "/opt/harness/scripts/rate-limit.yaml";
pub const NETWORK_SCRIPT: &str = "/opt/harness/scripts/network-access.yaml";
pub const SECRET_SCRIPT: &str = "/opt/harness/scripts/push-secret.yaml";
pub const PROTECTED_SCRIPT: &str = "/opt/harness/scripts/push-protected.yaml";
pub const HIDDEN_SCRIPT: &str = "/opt/harness/scripts/push-hidden.yaml";
pub const WEBHOOK_SCRIPT: &str = "/opt/harness/scripts/webhook.yaml";
pub const DELEGATION_SCRIPT: &str = "/opt/harness/scripts/delegation.yaml";
pub const RESEARCH_SCRIPT: &str = "/opt/harness/scripts/research.yaml";
pub const INTERRUPT_SCRIPT: &str = "/opt/harness/scripts/interrupt.yaml";

/// Repository URL of the projects.
pub fn repo_url() -> String {
    format!("http://{FORGE_SERVICE}.{FORGE_NAMESPACE}.svc:{FORGE_PORT}/{FORGE_OWNER}/{FORGE_REPO}")
}

/// The GitHub token `push-secret.yaml` commits (rule `betterleaks.github-pat`). Assembled from
/// parts like the script does, so no source file holds a string that secret scanners match.
pub fn test_token() -> String {
    format!("{}_{}", "ghp", "E2eTestOnlyNotARealToken000000000000")
}

/// Command that starts the fake agent with `script`.
pub fn adapter_command(script: &str) -> Vec<String> {
    [FAKE_AGENT, "--script", script].map(str::to_owned).to_vec()
}
