# git-hardening Specification

## Purpose

Treats everything on the task volume as untrusted: the runtime regenerates the Git configuration it relies on and runs its own Git commands so that nothing planted in the repository can execute code or redirect traffic (`design/agent-run.md` §7).

## Requirements

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
