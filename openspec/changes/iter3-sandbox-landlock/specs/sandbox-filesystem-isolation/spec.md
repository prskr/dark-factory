## ADDED Requirements

### Requirement: Harness runs under Landlock
With Landlock enforced, the runtime SHALL start every harness command (the harness image's adapter, a project's adapter command, full-harness mode) under a Landlock domain that the harness and all its descendants inherit. The runtime process itself SHALL stay outside the domain.

#### Scenario: Runtime keeps its access
- **WHEN** the mTLS certificate is rotated during a run and the runtime reconnects
- **THEN** the runtime reads the new `tls.key` and the connection succeeds

#### Scenario: Project adapter restricted
- **WHEN** a project's own adapter command runs a shell tool
- **THEN** the shell tool is inside the same domain as the adapter

### Requirement: Harness filesystem access
The domain SHALL grant read and execute on every top-level directory that is not a symlink, except those containing the data directory and the run directory; full access on the workspace and home directories of the layout (default `/data/repo`, `/data/home`), `/tmp` and `/dev`; read on the runtime-maintained copy of the CA certificate; and no other filesystem access.

#### Scenario: Toolchain under a top-level directory
- **WHEN** the harness runs `/usr/bin/git --version`
- **THEN** the command succeeds

### Requirement: Aliases from unrestricted runs
The runtime SHALL refuse to start the harness if a writable root directory is itself a symlink or has a symlink among its path components; symlinks inside the writable directories SHALL NOT be checked.

#### Scenario: Symlinked home
- **WHEN** an earlier unrestricted run replaced `/data/home` with a symlink to `/data/runtime`
- **THEN** the harness doesn't start and the run ends `FAILED`

#### Scenario: Symlinks inside the workspace
- **WHEN** a Bazel build created `/data/repo/bazel-bin` pointing into `/data/home/.cache/bazel`
- **THEN** the harness starts and can read and write through `bazel-bin`

#### Scenario: Private key unreadable
- **WHEN** the harness runs `cat /run/factory/tls/tls.key`
- **THEN** the read fails with `Permission denied`

#### Scenario: CA certificate readable after renewal
- **WHEN** the sandbox certificate is renewed during a run and the harness then runs `git fetch` through the Git proxy
- **THEN** TLS verification with the CA from `NODE_EXTRA_CA_CERTS` and `sslCAInfo` succeeds

#### Scenario: Outbox protected
- **WHEN** the harness writes to a file under `/data/runtime`
- **THEN** the write fails with `Permission denied`

#### Scenario: System directories read-only
- **WHEN** the harness runs `touch /usr/local/bin/x` or `touch /etc/x`
- **THEN** both fail with `Permission denied`, and `/usr/bin/git --version` succeeds

#### Scenario: Toolchain in a project-specific directory
- **WHEN** the project image installs a toolchain under `/opt/go`
- **THEN** the harness can execute `/opt/go/bin/go` without configuration

#### Scenario: Workspace and home writable
- **WHEN** the harness builds and runs a binary in `/data/repo/target` and writes `/data/home/.cache/x`
- **THEN** both succeed

### Requirement: Runtime directory checked at startup
At startup, before it opens the outbox, the runtime SHALL exit without starting the harness if its runtime directory (default `/data/runtime`) is a symlink or has a symlink among its path components. It SHALL remove symlinks inside the runtime directory (the links, never their targets) and break hard links to files in it. One runtime process handles one run.

#### Scenario: Hard link from an unrestricted run
- **WHEN** `/data/home/x` is a hard link to an outbox file, created while Landlock was disabled
- **THEN** after the next run starts, writes to `/data/home/x` don't change the outbox

#### Scenario: Symlinked outbox directory
- **WHEN** an earlier unrestricted run replaced `/data/runtime` with a symlink to `/data/home/rt`
- **THEN** the sandbox runtime exits at startup without opening the outbox or starting the harness

#### Scenario: Symlink inside the outbox directory
- **WHEN** an earlier unrestricted run left `/data/runtime/outbox` as a symlink into `/data/home`
- **THEN** the runtime removes the link before it opens the outbox, and the link's target is unchanged

### Requirement: Harness can't reach the runtime process
With Landlock enforced, the harness SHALL NOT be able to send signals to, trace, or read the memory of processes outside its domain, and SHALL NOT connect to abstract Unix sockets created outside its domain.

#### Scenario: Signal to the runtime
- **WHEN** the harness runs `kill -TERM 1`
- **THEN** the call fails with `Operation not permitted` and the runtime keeps running

#### Scenario: Runtime memory
- **WHEN** the harness reads `/proc/1/environ` or `/proc/1/mem`
- **THEN** the read fails

#### Scenario: Factory tools still work
- **WHEN** the harness calls a factory tool through the MCP server it started
- **THEN** the call reaches the runtime over the runtime socket and succeeds

### Requirement: Harness environment and descriptors
The runtime SHALL start the harness without its own configuration variables: every `DF_*` variable except `DF_RUNTIME_SOCK`, `DF_RUN_DIR` and `DF_LOG_FORMAT` SHALL be removed. Variables of the project image and of `Assign` SHALL stay. The harness SHALL inherit no file descriptors other than stdin, stdout and stderr.

#### Scenario: Runtime variables removed
- **WHEN** the harness runs `env`
- **THEN** the output contains no `DF_CONTROL_PLANE_ADDR`, `DF_CONTROL_PLANE_SERVER_NAME`, `DF_TLS_DIR` or `DF_LANDLOCK`, and contains the image's `PATH`

#### Scenario: Only standard descriptors
- **WHEN** the harness lists `/proc/self/fd` right after start
- **THEN** it finds only 0, 1 and 2 (plus the descriptor of the listing itself)

### Requirement: Minimum Landlock ABI
With Landlock enforced, the runtime SHALL check before starting the harness that the kernel supports Landlock ABI 6 or higher. If it doesn't, the runtime SHALL NOT start the harness, SHALL emit a `sandbox.landlock_unavailable` notice naming the detected ABI (or that Landlock is unavailable), and SHALL finalize the run with outcome `FAILED`. The domain SHALL handle exactly the access rights and scopes of ABI 6, also on kernels with a newer ABI.

#### Scenario: Kernel without Landlock
- **WHEN** a run starts on a node whose kernel has no Landlock and `DF_LANDLOCK` is `enforce`
- **THEN** no harness process starts, a `sandbox.landlock_unavailable` notice says Landlock is unavailable, and the run ends `FAILED`

#### Scenario: Newer kernel
- **WHEN** the kernel supports ABI 7
- **THEN** the harness starts with the ABI 6 ruleset

### Requirement: Instance-wide opt-out
The runtime SHALL read `DF_LANDLOCK` (`enforce` or `disabled`, default `enforce`); any other value SHALL make the runtime exit at startup with an error naming the variable. With `disabled`, the runtime SHALL start the harness without a Landlock domain and log a warning at every run start. The control plane SHALL set `DF_LANDLOCK` on every sandbox pod from the instance config `sandbox.landlock`.

#### Scenario: Opt-out
- **WHEN** the instance config sets `sandbox.landlock: disabled`
- **THEN** sandbox pods get `DF_LANDLOCK=disabled`, the harness can read `tls.key`, and the runtime log has a warning for each run

#### Scenario: Invalid value
- **WHEN** the runtime starts with `DF_LANDLOCK=off`
- **THEN** it exits non-zero with an error naming `DF_LANDLOCK`

### Requirement: Agent is told about the sandbox
With `sandbox.landlock: enforce`, the system prompt of every run SHALL contain the fixed "Sandbox" section naming the writable directories (`/data/repo`, `/data/home`, `/tmp`) and stating that other paths are read-only or inaccessible on purpose. With `disabled`, this part SHALL be absent.

#### Scenario: Enforced
- **WHEN** a run starts on an instance with `sandbox.landlock: enforce`
- **THEN** the system prompt names `/data/repo`, `/data/home` and `/tmp` as writable
