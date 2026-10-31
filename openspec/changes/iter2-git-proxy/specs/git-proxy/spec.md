# Spec Delta

## Purpose

The gateway's Git smart-HTTP proxy is the only way a sandbox reaches its project repository: it holds the forge credential, lets a run touch only its own task refs and records every push (F1, `design/agent-run.md` §11).

## ADDED Requirements

### Requirement: Task repository endpoint
The gateway SHALL serve Git smart HTTP at `https://<gateway host>:<git port>/git/<TASK-KEY>.git` with a certificate from the instance CA, supporting `info/refs`, `git-upload-pack` and `git-receive-pack`, including protocol v2 and partial-clone filters for fetches.

#### Scenario: Blobless clone
- **WHEN** the runtime of run R for `DF-7` runs `git clone --filter=blob:none https://<gateway>/git/DF-7.git`
- **THEN** the clone succeeds with the content of the project's repository

### Requirement: Caller identification
The proxy SHALL identify the run by source pod IP from the latest policy snapshot. It SHALL answer 403 for unknown sources and when the task key in the path differs from the run's task, and 503 until the first snapshot arrived.

#### Scenario: Other task's repository path
- **WHEN** the sandbox of `DF-7` requests `/git/DF-8.git/info/refs?service=git-upload-pack`
- **THEN** the proxy answers 403 and contacts no upstream

#### Scenario: Unknown pod
- **WHEN** a pod that isn't an active sandbox requests `/git/DF-7.git/info/refs`
- **THEN** the proxy answers 403

### Requirement: Credential injection
The proxy SHALL forward requests to the project's repository URL with the project's forge credential, SHALL remove any `Authorization` header sent by the client, and SHALL never return the credential to the client or write it to logs.

#### Scenario: Client sends its own credentials
- **WHEN** a client sends `Authorization: Basic …` with other credentials
- **THEN** the upstream request carries only the project's forge credential

### Requirement: Allowed ref updates
A push SHALL be forwarded only if every ref update targets `refs/heads/factory/task/<KEY>` as a create or fast-forward, or `refs/heads/factory/wip/<KEY>` as any create, update or delete. Otherwise the whole push SHALL be rejected without contacting upstream.

#### Scenario: Push to main
- **WHEN** the sandbox of `DF-7` pushes `refs/heads/main`
- **THEN** git reports `remote rejected` for `main` with reason `df:ref-not-allowed` and the upstream `main` is unchanged

#### Scenario: Force push of the task branch
- **WHEN** a push updates `factory/task/DF-7` to a commit that doesn't contain the current remote commit
- **THEN** the push is rejected with reason `df:non-fast-forward`

#### Scenario: Task branch deletion
- **WHEN** a push deletes `factory/task/DF-7`
- **THEN** the push is rejected with reason `df:ref-not-allowed`

#### Scenario: WIP snapshot
- **WHEN** the runtime force-pushes `factory/wip/DF-7`
- **THEN** the push is forwarded and succeeds

#### Scenario: Mixed push
- **WHEN** one push updates `factory/task/DF-7` and `refs/tags/v1`
- **THEN** both ref updates are rejected and nothing reaches upstream

### Requirement: Rejections reach the git client
Rejected pushes SHALL be answered with a valid `report-status` response (side-band when the client requested it), naming each rejected ref and a reason prefixed with `df:`, so that `git push` exits non-zero and prints the reason.

#### Scenario: Reason visible to the agent
- **WHEN** a push to `main` is rejected
- **THEN** `git push` prints `! [remote rejected] main (df:ref-not-allowed …)` and exits with a non-zero status

### Requirement: Push size limit
The proxy SHALL reject a push whose request body exceeds the configured maximum push size (default 256 MiB): it SHALL answer with HTTP 413 and record the push as `rejected` with reason `df:push-too-large`, the ref commands read so far and the bytes read. Git shows only the HTTP status, not the reason.

#### Scenario: Oversized push
- **WHEN** a push body exceeds 256 MiB with the default configuration
- **THEN** the push is answered with HTTP 413, recorded as `rejected` with `df:push-too-large`, and nothing reaches upstream

### Requirement: Every push is recorded
For every push attempt, the proxy SHALL store a record (run, task, ref updates with old and new SHA, verdict, reasons, bytes, duration) with the control plane before answering the client. A push to forward SHALL be recorded as `forwarding` before anything reaches upstream, then updated to its final verdict. If the record cannot be stored within 5 seconds, the push SHALL be rejected with `df:audit-unavailable`.

#### Scenario: Accepted push recorded
- **WHEN** the task branch of `DF-7` is pushed successfully
- **THEN** a push record with verdict `accepted` and an audit entry `git.push` exist for run R

#### Scenario: Recorded before forwarding
- **WHEN** a push passes all rules and checks
- **THEN** its record exists with verdict `forwarding` before the proxy sends the first byte to upstream

#### Scenario: Retried record
- **WHEN** the gateway sends the same final verdict for a push twice
- **THEN** one record and one audit entry exist for that push

#### Scenario: Control plane unreachable
- **WHEN** no control plane replica answers the record within 5 seconds
- **THEN** the push is rejected with `df:audit-unavailable` and nothing reaches upstream

### Requirement: Upstream failures are reported, not hidden
Before checking a push the proxy SHALL fetch its mirror: if that fetch cannot reach the forge or times out it answers 503; if the forge rejects the credential (401, 403) or answers another error, 502 with a fixed message. A forwarded request gets the same 503, or the same 502 for 401/403; every other response is relayed with status and body (headers filtered). A push failing either way is recorded `upstream-failed`. 503 is only for conditions that clear by themselves.

#### Scenario: Forge down
- **WHEN** Forgejo is unreachable during a push
- **THEN** the proxy answers 503, `git push` fails and the push record has verdict `upstream-failed`

#### Scenario: Forge error during the pre-push fetch
- **WHEN** Forgejo answers 500 to the proxy's mirror fetch before a push is checked
- **THEN** the proxy answers 502 with a fixed message, forwards nothing and the push record has verdict `upstream-failed`

#### Scenario: Forge rejects the credential
- **WHEN** Forgejo answers 401 to the proxy's forwarded push
- **THEN** the proxy answers 502 without an authentication challenge and the push record has verdict `upstream-failed`

#### Scenario: Forge refuses a ref
- **WHEN** Forgejo answers a forwarded push with 200 and an `ng` line in its `report-status` (for example a pre-receive hook declined it)
- **THEN** the proxy relays the answer, `git push` fails and the push record has verdict `upstream-failed` with reason `df:forge-refused` on the refused ref

### Requirement: Plain-HTTP forges only when allowed
With the instance setting `forge.allowPlainHttp` on, the control plane SHALL build Git routes for `http://` repositories and the gateway SHALL connect to such upstreams without TLS. With the setting off, an `http://` repository SHALL get no Git route, and the gateway SHALL answer a route to an `http://` upstream with `502` without connecting. The sandbox SHALL use HTTPS to the gateway in both cases.

#### Scenario: Forge over plain HTTP in a throwaway cluster
- **WHEN** `forge.allowPlainHttp` is on and the runtime of `DF-7` pushes its task branch while the project repository is `http://forgejo-http.forgejo.svc:3000/factory/demo`
- **THEN** the push goes over HTTPS to the gateway, and the gateway forwards it to Forgejo over plain HTTP

#### Scenario: Plain HTTP not allowed
- **WHEN** `forge.allowPlainHttp` is off and a project's repository is `http://forgejo-http.forgejo.svc:3000/factory/demo`
- **THEN** the run gets no Git route and the gateway answers its clone with `403` (`df: no repository for this task`)
