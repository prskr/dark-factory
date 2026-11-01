# Spec Delta

## MODIFIED Requirements

### Requirement: Runtime-owned .git/config
Before using the repository and again at finalize, the runtime SHALL replace `.git/config` with a configuration generated from an allowlisted fresh-clone template plus its own keys (remote URL from `Assign`, partial-clone keys, an empty credential helper list, the instance CA bundle as `http.<proxy URL>.sslCAInfo` in an `[http "<scheme://host:port/>"]` section, commit identity from `Assign`, hooks path = the read-only empty directory), and SHALL delete `.git/config.worktree`.

#### Scenario: Planted configuration
- **WHEN** the agent added `core.fsmonitor`, `url.*.insteadOf`, an `include.path` and a credential helper to `.git/config`
- **THEN** after the next rewrite none of them exist and the remote URL equals `Assign.workspace.repo_url`

#### Scenario: Proxy certificate trusted
- **WHEN** the runtime fetches from the Git proxy URL
- **THEN** TLS verification succeeds with the instance CA from the pod's certificate volume and fails for any other issuer
- **AND** HTTPS URLs on any other host keep the system trust store

### Requirement: Hardened Git invocations
Every Git command the runtime runs SHALL use the empty hooks directory, disable fsmonitor and submodule recursion, allow only the HTTPS protocol, reset credential helpers to none, ignore system and user Git config in favour of the read-only `/opt/factory/gitconfig`, and disable terminal prompts. It SHALL NOT set `GIT_SSL_CAINFO` (it overrides the scoped `http.<url>.sslCAInfo`); the clone, which runs before `.git/config` exists, gets the instance CA as `-c http.<proxy URL>.sslCAInfo=<ca>`.

#### Scenario: Malicious hook
- **WHEN** the repository contains an executable `.git/hooks/pre-push`
- **THEN** the runtime's push doesn't execute it

#### Scenario: Non-HTTPS remote
- **WHEN** a Git operation of the runtime would use `ssh://` or `file://`
- **THEN** Git refuses the protocol

#### Scenario: No credential prompt
- **WHEN** the proxy answers 401 or 403
- **THEN** the Git command fails immediately without asking for credentials

## REMOVED Requirements

### Requirement: Credential helper
**Reason**: Sandboxes hold no forge credential any more; the Git proxy authenticates upstream (F1).
**Migration**: None for users. `factory-agent git-credential` is removed; Git configurations that still reference it fail with "command not found", which the `.git/config` rewrite prevents.
