## MODIFIED Requirements

### Requirement: Artifact files read by the runtime
For `write_artifact` with `path`, the runtime SHALL read the file itself and send its bytes as the request's attachment, without `path`. The path SHALL be absolute, SHALL resolve beneath the workspace, `/tmp` or `/srv` without passing through any symlink, and SHALL name a regular file of at most 8 MiB, checked before reading. Otherwise the call SHALL fail locally with an error result and nothing SHALL be sent.

#### Scenario: Screenshot in /tmp
- **WHEN** the harness calls `write_artifact` with `path: "/tmp/login.png"`, a regular file
- **THEN** the control plane receives the request with the file's bytes as attachment and no `path`

#### Scenario: Symlinked directory
- **WHEN** `path` is `/data/repo/certs/client.key` and `/data/repo/certs` is a symlink to the runtime's certificate directory
- **THEN** the call fails with an error that the path passes through a symlink, and nothing is read

#### Scenario: Outside the allowed roots
- **WHEN** `path` is `/etc/passwd`
- **THEN** the call fails with an error naming the allowed roots

#### Scenario: Directory
- **WHEN** `path` names a directory in the workspace
- **THEN** the call fails with an error that the path is not a regular file

#### Scenario: Screenshot in the shared directory
- **WHEN** the harness calls `write_artifact` with `path: "/srv/login.png"`, a regular file written by a sidecar
- **THEN** the control plane receives the request with the file's bytes as attachment and no `path`
