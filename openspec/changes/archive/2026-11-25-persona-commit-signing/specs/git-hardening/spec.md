# Spec Delta

## MODIFIED Requirements

### Requirement: Runtime-owned .git/config
Before using the repository and again at finalize, the runtime SHALL replace `.git/config` with a configuration generated from an allowlisted fresh-clone template plus its own keys (remote URL from `Assign`, partial-clone keys, an empty credential helper list, the instance CA bundle as `http.<proxy URL>.sslCAInfo` in an `[http "<scheme://host:port/>"]` section, commit identity from `Assign`, the signing keys `gpg.format=ssh`, `gpg.ssh.program` = the read-only signing shim, `user.signingKey` = the persona's public key from `Assign` and `commit.gpgSign=true`, hooks path = the read-only empty directory), and SHALL delete `.git/config.worktree`.

#### Scenario: Planted configuration
- **WHEN** the agent added `core.fsmonitor`, `url.*.insteadOf`, an `include.path` and a credential helper to `.git/config`
- **THEN** after the next rewrite none of them exist and the remote URL equals `Assign.workspace.repo_url`

#### Scenario: Signing turned off by the agent
- **WHEN** the agent set `commit.gpgSign=false` and `gpg.ssh.program=/data/home/fake-sign` in `.git/config`
- **THEN** after the next rewrite `commit.gpgSign` is `true` and `gpg.ssh.program` is the shim under `/opt/factory`

#### Scenario: Proxy certificate trusted
- **WHEN** the runtime fetches from the Git proxy URL
- **THEN** TLS verification succeeds with the instance CA from the pod's certificate volume and fails for any other issuer
- **AND** HTTPS URLs on any other host keep the system trust store
