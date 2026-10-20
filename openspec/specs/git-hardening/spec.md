# git-hardening Specification

## Purpose

Treats everything on the task volume as untrusted: the runtime regenerates the Git configuration it relies on and runs its own Git commands so that nothing planted in the repository can execute code or redirect traffic (`design/agent-run.md` §7).

## Requirements

### Requirement: Runtime-owned .git/config
Before using the repository and again at finalize, the runtime SHALL replace `.git/config` with a configuration generated from an allowlisted fresh-clone template plus its own keys (remote URL from `Assign`, partial-clone keys, credential helper = the runtime, commit identity from `Assign`, hooks path = the read-only empty directory), and SHALL delete `.git/config.worktree`.

#### Scenario: Planted configuration
- **WHEN** the agent added `core.fsmonitor`, `url.*.insteadOf`, an `include.path` and a second credential helper to `.git/config`
- **THEN** after the next rewrite none of them exist and the remote URL equals `Assign.workspace.repo_url`

### Requirement: Hardened Git invocations
Every Git command the runtime runs SHALL use the empty hooks directory, disable fsmonitor and submodule recursion, allow only the HTTPS protocol, reset credential helpers to the runtime only, ignore system and user Git config in favour of the read-only `/opt/factory/gitconfig`, and disable terminal prompts.

#### Scenario: Malicious hook
- **WHEN** the repository contains an executable `.git/hooks/pre-push`
- **THEN** the runtime's push doesn't execute it

#### Scenario: Non-HTTPS remote
- **WHEN** a Git operation of the runtime would use `ssh://` or `file://`
- **THEN** Git refuses the protocol

### Requirement: Credential helper
`factory-agent git-credential get` SHALL obtain credentials from the parent runtime via a Unix socket on tmpfs, which requests them from the control plane; credentials SHALL be kept in memory only until expiry and never written to the volume. `store` and `erase` SHALL be no-ops.

#### Scenario: Clone needs credentials
- **WHEN** Git asks the helper for `https://git.example.org/peter/dark-factory.git`
- **THEN** the helper prints the granted username and password, and no file under `/data` contains the password

#### Scenario: Denied request
- **WHEN** the control plane denies the request
- **THEN** the helper returns no credentials and Git fails with an authentication error
