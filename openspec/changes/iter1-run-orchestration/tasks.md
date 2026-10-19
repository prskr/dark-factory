# Tasks

## 1. Agent port and identity

- [x] 1.1 Create `df-agent-channel` with a test helper that issues a CA, server cert and per-pod client certs (URI SAN per `design/agent-run.md` §4.1) via `rcgen`, and starts the tonic server on an ephemeral port; verify a test client completes a TLS handshake
- [x] 1.2 Implement the rustls server config (ring, client cert required against the CA bundle file, 16 MiB message limit, HTTP/2 keepalive) test-first; verify the "no client certificate", "other CA" and oversized-message scenarios
- [x] 1.3 Implement SAN/peer-IP extraction and binding via `SandboxProvider::resolve_identity` (fake); verify the "stolen key" and "finished run" scenarios return `UNAUTHENTICATED`
- [x] 1.4 Mount the agent port in `dark-factory serve` (port 9443, cert paths from config); verify `serve` starts it alongside the UI with a config fixture

## 2. Protocol session

- [x] 2.1 Implement `Hello`/`HelloAck` (capabilities, last persisted seq) and the "message before Hello" rejection test-first; verify with the scripted client
- [x] 2.2 Implement `Assign` building from DB rows and instance config (design D8), embedding `contract/mcp/factory-tools.v1.json` as `ToolDescriptor`s; verify the "Assign for DF-1" scenario
- [x] 2.3 Implement batched persistence with cumulative acks, dedup and gap detection (design D3); verify the replay and gap scenarios
- [x] 2.4 Implement prompt (re-)sending on connect and on `task_message_queued`; verify "reconnect to another replica" with two server instances sharing one database

## 3. Transcript and phases

- [x] 3.1 Implement transcript persistence (canonical JSON, blob extraction, global id, `transcript_event` notification) test-first; verify the scenarios in `specs/transcript-storage/spec.md`
- [x] 3.2 Implement prompt delivery confirmation on `UserMessage`; verify "prompt delivered" and that a delivered prompt is not re-sent after reconnect
- [x] 3.3 Implement `PhaseChanged` → run state and `RunReport` handling (outcome → run state per design D4, task status mapping, session id, workspace state, teardown); verify every row of the mapping and the "message arrived during finalize" scenario

## 4. Scheduler

- [x] 4.1 Implement the scheduling loop with `SKIP LOCKED` (design D2) test-first; verify "new task" and "two replicas" (two schedulers, one database, exactly one run)
- [x] 4.2 Implement provisioning calls and provider-event handling (`Started` → `connecting` + pod IP, `Gone` → `sandbox_lost`), `provisioning_failed`; verify with scripted fake-provider events
- [x] 4.3 Implement connect timeout, wall-clock budget `Cancel` and grace-period teardown; verify with paused tokio time
- [x] 4.4 Implement continuation (previous session id, summary fallback per design D5); verify the "follow-up run" scenario

## 5. Tools, PR lifecycle, credentials

- [x] 5.1 Implement the tool registry with schema validation, scoping and exactly-once execution with audit (design D6); verify "unknown tool", "schema violation" and "retry after reconnect"
- [x] 5.2 Implement `update_pull_request`, `comment_on_issue`, `report_progress`, `complete_task` against `FakeForgeClient`; verify each scenario in `specs/factory-tools/spec.md`, incl. forge errors as tool errors
- [x] 5.3 Implement the PR lifecycle (draft on first push ahead, defaults, find-open idempotency, retry, ready + summary comment on `completed`, divergence flag); verify every scenario in `specs/pull-request-lifecycle/spec.md`
- [x] 5.4 Implement `HarnessCredentials` via `SecretStore` and Git `CredentialRequest` matching against the project repo URL; verify the scenarios in `specs/run-credentials/spec.md` and that no secret appears in logs (tracing capture test)

## 6. Verification

- [x] 6.1 Write one in-process end-to-end test: create task → scheduler → fake provider `Started` → scripted runtime client runs Hello, transcript, tool call, push report, `RunReport(end_turn)` → queue follow-up → run 2 with continuation; verify PR created once, statuses `waiting` then `running`
- [x] 6.2 Verify `cargo llvm-cov -p df-agent-channel` reports ≥ 70 % line coverage
