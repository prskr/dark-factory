# Tasks

## 1. Contract and decision record

- [x] 1.1 Add `FactoryActivity` (`oneof kind { ArtifactWritten artifact_written = 1; }`) and `ArtifactWritten` (`name`, `kind`, `mime_type`, `size`) to `contract/proto/darkfactory/transcript/v1/transcript.proto`, and `FactoryActivity factory_activity = 20` to `Event.kind`, with comments that say who writes it and that later control-plane events join the oneof (design D2). Verify `cargo build -p df-proto` and that `buf lint` / the proto pre-commit checks pass.
- [x] 1.2 Write ADR-0057 "Factory activity in the transcript" from `docs/adr/template.md`: the family next to forge activity, written by the control plane only, keyed per source, artifact writes as the first variant, the tool row stays, and the rejected alternatives of design D1 and D2. Link it from ADR-0046 and ADR-0044 ("Related") and verify the links resolve.

## 2. Storage (`df-db`)

- [x] 2.1 Write failing tests in `crates/df-db/src/transcript/tests.rs`: a factory activity event is stored with kind `factory_activity`, publishes `transcript_event`, and is stored once per `(run, source_key)`; an event that is neither forge nor factory activity is still refused. Then add `factory_event` and generalize `append_control_event` (design D3) until they pass.
- [x] 2.2 Write failing tests in `crates/df-db/src/artifacts/tests.rs`: a stored artifact appends one artifact-written event with name, type, MIME type and size to the calling run's transcript; the same call id twice stores one event; a write rejected by the count limit stores neither artifact nor event. Then change `write_artifact` to take the run id and call id, lock with `lock_run_and_task` and append the event with source key `artifact:<call_id>` in its transaction (design D4) until they pass. If a `sqlx::query!` macro changes, regenerate `.sqlx/` and commit it.

## 3. Agent channel and runtime

- [x] 3.1 Pass the tool call's `call_id` from the dispatcher (`crates/df-agent-channel/src/tools.rs`) to `tools/artifacts.rs::write_artifact` and on to `df_db::artifacts::write_artifact`. Verify with a test in `crates/df-agent-channel` that a `write_artifact` call through the dispatcher leaves one artifact-written event in the run's transcript, and that a repeated call id leaves one.
- [x] 3.2 Make `allowed_from_runtime` refuse `FactoryActivity` and add the kind to `kind_name` and `content_blocks_mut` (`crates/df-agent-channel/src/transcript.rs`). Verify with a session test (next to the forged forge-activity test) that a factory activity event sent by the runtime is not stored and the following events are.
- [x] 3.3 Ignore `FactoryActivity` in the runner of `crates/factory-agent/src/runner.rs` like forge activity; verify `cargo test -p factory-agent` passes and no prompt or context built for a run contains the new kind (extend the existing forge-activity check if there is one).

## 4. Web UI (`df-web`)

- [x] 4.1 Write failing tests in `crates/df-web/src/transcript/tests.rs`: an artifact-written event renders the artifact row (`data-artifact-written`, name, label from the MIME type, opens the artifact); a completed `write_artifact` tool call (Claude Code style title) and an OpenCode `execute` call both render as ordinary tool rows; a failed call renders only its tool row. Then add the item kind, map the event in `Folder::apply` and remove `written_artifact` and its case in `render_tool` (design D5) until they pass. Update the existing tests in `crates/df-web/src/artifacts/tests.rs` that built the row from a tool call.
- [x] 4.2 Verify live behavior: extend the SSE or artifacts test that covers `artifacts-changed` so that an artifact-written event arriving on the live stream produces a row with `data-artifact-written` (the trigger of the tab reload, the out-of-band tab bar and the split-view reload in `scripts/artifactSplit.ts`).
- [x] 4.3 Check `crates/df-e2e/tests/delegation.rs` (`a_researcher_hands_its_findings_to_the_coordinator_as_an_artifact`) for assertions on the artifact row and adapt them to the tool row plus artifact row; run it with `just e2e-local` where a container runtime is available, otherwise state in the report that it did not run.

## 5. Documentation

- [x] 5.1 Update `docs/content/using/artifacts.md`: the "Transcript" bullet (a row from the control plane after every stored artifact, under the agent's tool row; a failed write shows only the tool row; works the same for every harness) and the "Live" bullet (triggered by that row). Verify with `just docs-build`.

## 6. Integration checks

- [ ] 6.1 Run `npx --yes @fission-ai/openspec@1.16.0 validate --all --strict --no-interactive`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete` and `bazel --bazelrc=.bazelrc.agents test //crates/df-db/... //crates/df-agent-channel/... //crates/df-web/... //crates/factory-agent/...` (container-backed tests with the socket, see skill `rust`); all pass.
- [ ] 6.2 Manual check with OpenCode and Claude Code tasks: each write shows the tool row and the artifact row, the Artifacts tab appears with the first artifact and reloads live, and an open artifact reloads.

## Workflow follow-up

- Done during the review (2026-11-10): add a backlog row to `product/iterations/roadmap.md` for further factory activity (agent woken by another task, incoming webhook events) and for run-less transcript rows, referencing this change and ADR-0057.
