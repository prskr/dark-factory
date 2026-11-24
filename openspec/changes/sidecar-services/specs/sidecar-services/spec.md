## ADDED Requirements

### Requirement: Sidecar definition
Board members SHALL create, edit, reorder and delete up to 5 sidecars per project, each with name, image, command and args, ports, env, resources, start probe (`tcp`, `http` or `none`), start timeout, data path, purpose (at most 500 characters), MCP endpoint, egress mode (`none` or `proxy`) and shared directory (default off). Every change SHALL be audited. Agents SHALL have no way to create or change sidecars.

#### Scenario: Playwright sidecar
- **WHEN** a board member adds sidecar `playwright` to project `DF` with image `mcr.microsoft.com/playwright/mcp@sha256:…`, port 8931, MCP endpoint on 8931 with path `/mcp`, requests `500m`/`1Gi`, limits `2`/`2Gi`, egress `proxy` and shared directory on
- **THEN** the sidecar is stored, listed on the Sidecars tab of `DF`, and an audit entry names the change

#### Scenario: Image without digest
- **WHEN** a board member enters image `postgres:17` without a digest
- **THEN** the form shows a hint to pin a digest and saving succeeds

#### Scenario: Sixth sidecar
- **WHEN** project `DF` has 5 sidecars and a board member adds another
- **THEN** the form shows a validation error and nothing is stored

### Requirement: Sidecar names, ports and MCP endpoint
A sidecar name SHALL be a DNS label of 1–40 characters, unique in the project, and not `agent` or `factory`. Ports SHALL be TCP ports 1–65535, at most 8 per sidecar, unique across the project's sidecars. An MCP endpoint SHALL name one of the sidecar's ports and a path starting with `/` (default `/mcp`); its transport is Streamable HTTP.

#### Scenario: Reserved name
- **WHEN** a board member saves a sidecar named `agent`
- **THEN** the form shows a validation error at the name and nothing is stored

#### Scenario: Port used by another sidecar
- **WHEN** project `DF` has sidecar `postgres` with port 5432 and a board member saves sidecar `pg2` with port 5432
- **THEN** the form shows a validation error naming `postgres` and nothing is stored

#### Scenario: MCP port not among the ports
- **WHEN** a board member sets the MCP endpoint port to 9000 on a sidecar whose ports are 8931
- **THEN** the form shows a validation error at the MCP port

### Requirement: Sidecar resources and env
A sidecar SHALL have a CPU request, a CPU limit, a memory request and a memory limit; an ephemeral storage request and limit are optional; every request SHALL be at most its limit. Env names SHALL be C identifiers; `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`, `http_proxy`, `https_proxy` and `no_proxy` SHALL be rejected, because the egress mode sets them. The form SHALL say that env values are stored as plain text and are not for secrets.

#### Scenario: Missing memory limit
- **WHEN** a board member saves a sidecar without a memory limit
- **THEN** the form shows a validation error at the memory limit and nothing is stored

#### Scenario: Request above limit
- **WHEN** a board member saves a sidecar with memory request `4Gi` and limit `2Gi`
- **THEN** the form shows a validation error at the memory request

#### Scenario: Proxy variable in env
- **WHEN** a board member adds env `HTTPS_PROXY` to a sidecar
- **THEN** the form shows a validation error that the egress mode sets the proxy

### Requirement: Sidecar start timeouts
A start timeout SHALL be 10–300 s (default 120 s). The sum of the start timeouts of a project's sidecars SHALL be at most half of `runs.connectTimeoutSeconds`, so that a failing start ends the run before the connect timeout.

#### Scenario: Timeouts above the budget
- **WHEN** `runs.connectTimeoutSeconds` is 600 and project `DF` has sidecars with start timeouts 200 s and 60 s
- **THEN** saving a change that sets the second one to 120 s fails with a validation error naming the budget of 300 s

### Requirement: Sidecars in task runs
Every task run SHALL start with the sidecars its project had when the run was created. The run SHALL keep that set: the pod, the system prompt and `Assign` SHALL use it, and later changes to the project's sidecars SHALL apply only to later runs. Chat runs SHALL start without sidecars.

#### Scenario: Change during a run
- **WHEN** run 2 of `DF-7` was created with sidecar `postgres` and a board member deletes `postgres` while the run is active
- **THEN** the pod keeps `postgres`, the run's system prompt still names it, and run 3 starts without it

#### Scenario: Chat run
- **WHEN** a chat run of project `DF`, which has sidecar `playwright`, is provisioned
- **THEN** its pod has no sidecar and its system prompt names none

### Requirement: Start before the agent
Each sidecar SHALL start before the agent container, in the project's order, and the agent container SHALL start only after every sidecar passed its start probe (with probe `none`: after it started). Without an explicit probe, the probe SHALL be a TCP check on the MCP port, else on the first port, else none. After the agent container started, a sidecar SHALL be restarted when it exits, and the sidecars SHALL stop when the agent container ends.

#### Scenario: Database ready before the runtime
- **WHEN** run 1 of `DF-7` starts with sidecar `postgres` on port 5432 and default probe
- **THEN** the agent container starts only after port 5432 of the pod accepts connections

#### Scenario: Sidecar crashes during the run
- **WHEN** sidecar `playwright` exits with code 1 while the harness is in a turn
- **THEN** the sidecar is restarted, the run continues, and the restart count on the task page increases

#### Scenario: Run ends
- **WHEN** the agent container of the run ends
- **THEN** the pod's sidecars stop and the pod reaches a final phase

### Requirement: Sidecar start failures
A sidecar start failure reported by the provider (`sandbox-provisioning`, "Sidecar events") SHALL end the run `sandbox_lost` with the provider's reason, without waiting for the connect timeout. Automatic resume SHALL apply as for any lost sandbox.

#### Scenario: Wrong image
- **WHEN** the image of sidecar `postgres` can't be pulled
- **THEN** the run ends `sandbox_lost` with reason `sidecar postgres: image pull failed: <image>` within 30 seconds of the kubelet reporting the failure

#### Scenario: Probe never passes
- **WHEN** sidecar `playwright` with start timeout 60 s listens on 8930 but its probe checks 8931
- **THEN** about 60 s after the sidecar started, the kubelet kills it, and the run ends `sandbox_lost` with a reason naming `playwright` and the failed start probe, before the connect timeout

#### Scenario: Entrypoint needs root
- **WHEN** the entrypoint of sidecar `legacy` exits with code 1 because it doesn't run as root
- **THEN** the run ends `sandbox_lost` with reason `sidecar legacy failed to start: Error (exit code 1)`

### Requirement: Sidecars at localhost
Sidecar ports SHALL be reachable from the agent container at `localhost:<port>` without an allowlist entry and without passing the egress proxy, also for clients that honour the proxy variables, and SHALL NOT be reachable from outside the pod.

#### Scenario: Agent connects to the database
- **WHEN** the agent runs `psql postgres://dev:dev@localhost:5432/app` in a run with sidecar `postgres` on port 5432
- **THEN** the connection succeeds and no egress log entry is written

#### Scenario: HTTP client with proxy variables
- **WHEN** the agent runs `curl http://localhost:8931/mcp` with the container's proxy variables
- **THEN** the request reaches sidecar `playwright` directly and no egress log entry is written

#### Scenario: Other pod
- **WHEN** a pod of another run connects to the pod IP of `DF-7`'s run on port 5432
- **THEN** it doesn't connect, because the network policy drops the traffic

### Requirement: Sidecar data directory
A sidecar with a data path SHALL get an empty directory there at every run start, writable by user 1000.

#### Scenario: Fresh database each run
- **WHEN** run 2 of `DF-7` starts with sidecar `postgres`, data path `/var/lib/postgresql/data` and env `PGDATA=/var/lib/postgresql/data/pgdata`
- **THEN** the directory is empty, whatever run 1 wrote, and PostgreSQL initializes its cluster in `pgdata`

### Requirement: Sidecar egress
A sidecar with egress `proxy` SHALL reach external hosts through the egress proxy with the task's allowlist; its connections SHALL appear in the task's egress log and denied hosts SHALL raise `network-access` requests, as for the agent. A sidecar with egress `none` gets no proxy variables; its direct connections are limited by the run's network policy like the agent's (control plane, gateway, the project's cluster targets).

#### Scenario: Browser opens an allowed site
- **WHEN** sidecar `playwright` with egress `proxy` opens `https://docs.rs` and `docs.rs` is allowed for the task
- **THEN** the page loads and the egress log of the task shows the connection

#### Scenario: Denied site
- **WHEN** sidecar `playwright` with egress `proxy` opens `https://example.com`, which is not allowed
- **THEN** the connection is denied and a `network-access` request for `example.com` is raised for the task

#### Scenario: No egress
- **WHEN** sidecar `postgres` with egress `none` connects directly to `1.1.1.1:443`
- **THEN** the connection fails and the container has no proxy variables

### Requirement: Shared directory
When a run has a sidecar with the shared directory on, the agent container and every such sidecar SHALL see the same empty directory at `/tmp/shared`, writable by both. Files in it SHALL be usable with `write_artifact` within its size limit (8 MiB per file).

#### Scenario: Screenshot as artifact
- **WHEN** sidecar `playwright` saves `/tmp/shared/login.png` and the harness calls `write_artifact` with `path: "/tmp/shared/login.png"`
- **THEN** the artifact is stored with the file's bytes

#### Scenario: File for an upload
- **WHEN** the agent writes `/tmp/shared/fixture.csv`
- **THEN** sidecar `playwright` can read `/tmp/shared/fixture.csv`

### Requirement: Sidecar isolation
A sidecar SHALL have no access to the runtime's files, the certificate or the task volume, and SHALL NOT see the agent container's processes, nor the agent the sidecar's. The pod shape that ensures it is in `sandbox-provisioning`, "Restricted sidecar containers".

#### Scenario: No certificate in the sidecar
- **WHEN** a process in sidecar `playwright` lists `/run/factory`
- **THEN** the directory doesn't exist

#### Scenario: Agent can't see sidecar processes
- **WHEN** the agent lists processes in its container
- **THEN** no process of a sidecar is shown

### Requirement: Sidecars in the run instructions
The system prompt of a run with sidecars SHALL list each sidecar with name, ports, purpose, MCP server name (if it has an MCP endpoint) and `/tmp/shared` (if it mounts the shared directory), and SHALL say that they are reachable at `localhost` and that their ports are taken. A run without sidecars SHALL have no such part.

#### Scenario: Prompt with two sidecars
- **WHEN** run 1 of `DF-7` starts with sidecars `playwright` (port 8931, MCP, shared directory) and `postgres` (port 5432, purpose `postgres://dev:dev@localhost:5432/app`)
- **THEN** the system prompt names both with their ports, `playwright` as MCP server `playwright` with `/tmp/shared`, and `postgres` with its purpose

### Requirement: Sidecar logs for humans
While a task's current run has a pod with sidecars, the task page SHALL show each sidecar with its state and restart count, and on request the last 500 lines (at most 256 KiB) of its log as plain text, desktop only. After the pod is gone, it SHALL say that the logs are gone with the sandbox.

#### Scenario: Read a sidecar log
- **WHEN** a board member opens the log of sidecar `postgres` on the page of `DF-7` while its run is active
- **THEN** the last lines of the `postgres` container log are shown as text, with state `running` and the restart count

#### Scenario: Pod gone
- **WHEN** the run of `DF-7` has ended and its pod is deleted
- **THEN** the task page shows no log action and says that the logs are gone with the sandbox

#### Scenario: Markup in the log
- **WHEN** a sidecar log line contains `<script>alert(1)</script>`
- **THEN** the line is shown as text and no script runs
