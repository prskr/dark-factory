## MODIFIED Requirements

### Requirement: Runtime-owned .git/config
Before using the repository and again at finalize, the runtime SHALL replace `.git/config` with a configuration generated from an allowlisted fresh-clone template plus its own keys (remote URL from `Assign`, partial-clone keys, an empty credential helper list, the instance CA bundle as `http.<proxy URL>.sslCAInfo` in an `[http "<scheme://host:port/>"]` section, commit identity from `Assign`, hooks path = the read-only empty directory, sparse checkout enabled in non-cone mode, also without hidden paths), and SHALL delete `.git/config.worktree`. It SHALL replace `.git/info/sparse-checkout` with patterns generated from the hidden paths of `Assign.workspace`; without hidden paths the patterns include every path.

#### Scenario: Planted configuration
- **WHEN** the agent added `core.fsmonitor`, `url.*.insteadOf`, an `include.path` and a credential helper to `.git/config`
- **THEN** after the next rewrite none of them exist and the remote URL equals `Assign.workspace.repo_url`

#### Scenario: Proxy certificate trusted
- **WHEN** the runtime fetches from the Git proxy URL
- **THEN** TLS verification succeeds with the instance CA from the pod's certificate volume and fails for any other issuer
- **AND** HTTPS URLs on any other host keep the system trust store

#### Scenario: Sparse checkout disabled by the agent
- **WHEN** the agent set `core.sparseCheckout=false` and emptied `.git/info/sparse-checkout`
- **THEN** after the next rewrite sparse checkout is enabled and the patterns match `Assign.workspace` again
