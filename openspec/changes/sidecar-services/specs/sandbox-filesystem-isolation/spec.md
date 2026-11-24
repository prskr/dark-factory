## MODIFIED Requirements

### Requirement: Harness filesystem access
The domain SHALL grant read and execute on every top-level directory that is not a symlink and that the runtime user can open, except those containing the data directory and the run directory; full access on the workspace and home directories of the layout (default `/data/repo`, `/data/home`), `/tmp`, `/dev` and `/srv` (the shared directory of sidecars, when it is a top-level directory); read on the runtime-maintained copy of the CA certificate; and no other filesystem access.

#### Scenario: Toolchain under a top-level directory
- **WHEN** the harness runs `/usr/bin/git --version`
- **THEN** the command succeeds

#### Scenario: Shared directory writable
- **WHEN** the run's pod mounts the shared directory at `/srv` and the harness writes `/srv/fixture.csv`
- **THEN** the write succeeds

### Requirement: Agent is told about the sandbox
With `sandbox.landlock: enforce`, the system prompt of every run SHALL contain the fixed "Sandbox" section naming the writable directories (`/data/repo`, `/data/home`, `/tmp`, and `/srv` when the run has a sidecar with the shared directory on) and stating that other paths are read-only or inaccessible on purpose. With `disabled`, this part SHALL be absent.

#### Scenario: Enforced
- **WHEN** a run starts on an instance with `sandbox.landlock: enforce`
- **THEN** the system prompt names `/data/repo`, `/data/home` and `/tmp` as writable

#### Scenario: Enforced with a shared directory
- **WHEN** a run with sidecar `playwright` (shared directory on) starts on an instance with `sandbox.landlock: enforce`
- **THEN** the system prompt also names `/srv` as writable
