# Tasks

> **Design first:** update the PenPot design (group 1) and get the user's approval before starting the UI tasks of group 5. Groups 2–4 do not depend on it.

## 1. Design agreement (PenPot)

- [x] 1.1 Draw these in PenPot, in light, dark and at 390 px width: the persona form with the handle, display name and email fields and the live avatar preview; the persona card; the avatar at its sizes (transcript, task row, details); the transcript author line "Display name @handle"; the persona value in the task details. Verify: the user approves the boards, and every changed label is written back into `specs/configuration-management/spec.md` and `specs/task-ui/spec.md` before group 5 starts.

## 2. Identity type and storage

- [x] 2.1 Write failing unit tests first for `PersonaIdentity` in `df-core` (design D4): handle regex, display name length and forbidden characters (`<`, `>`, control characters, newline), email rules and lowercasing. Then implement. Verify: `cargo nextest run -p df-core persona`.
- [x] 2.2 Write failing migration tests first for the scenarios of "Identity migration" (spaces, colliding slugs in creation order, a suffixed handle colliding with an existing slug, a 39-character slug with suffix, name without usable characters → `persona`, a name with `<`, `>` or over 100 characters → valid display name). Then add the migration of design D2 plus `runs.commit_name`/`commit_email` with the backfill (design D1, migration plan step 2). Verify: `cargo nextest run -p df-db migration` (container-backed).
- [x] 2.3 Write failing tests first: `save_persona` stores and audits handle, display name and email; duplicate handle or email returns a conflict naming the other persona; `persona_id_by_handle` replaces `persona_id_by_name`; `list_personas` is ordered by handle. Add one test that feeds the invalid handles from 2.1 to the DB CHECK. Implement in `catalog.rs` and regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db catalog` (container-backed).
- [x] 2.4 Write a failing test first: starting a run (`runs::start_queued_runs`, which creates the run row) copies the persona's display name and email onto the run. Implement. Verify: `cargo nextest run -p df-db runs` (container-backed).

## 3. Identity through Assign, gateway and runtime

- [x] 3.1 Write failing tests first (`df-agent-channel` assign tests) for the run-orchestration scenario "Persona commit identity" and for "Identity edited during a run" (Assign uses the run's frozen identity). Then fill `commit_identity` from the run in `run_context.rs` / `assign.rs` and remove `agent.commitIdentity` from `df-core/src/config.rs`. Verify: `cargo nextest run -p df-agent-channel assign` and `-p df-core config`.
- [x] 3.2 Write a failing test first: the snapshot's `GitRoute.commit_name`/`commit_email` equal the run's identity, also after a persona edit. Implement in `df-gateway-control/src/snapshot.rs`. Verify: `cargo nextest run -p df-gateway-control snapshot` (container-backed).
- [x] 3.3 Add gateway tests for the push-checks scenarios "Commit as another persona" and "Commit as the run's persona", and for the workspace-lifecycle scenario "Rebase of another persona's commits". No gateway code change is expected. Verify: `cargo nextest run -p df-gateway policy_checks`.
- [x] 3.4 Write failing factory-agent tests first: an `Assign` without an identity, or with an empty name or email, fails the run before the workspace is prepared; WIP snapshot commits carry the `Assign` identity. Then remove `DEFAULT_COMMIT_AUTHOR_*` (design D6) and update the comment of the proto field. Verify: `cargo nextest run -p factory-agent`.
- [x] 3.5 Update the reference docs: `crates/df-agent-channel/docs/configuration.md` (remove `agent.commitIdentity`), `crates/df-gateway/README.md` (identity = the run's persona), and remove `agent.commitIdentity` from the Helm chart values and templates, if present, along with their Helm tests. Verify: `just docs-build` and the Helm tests in `lefthook.yml` pass.

## 4. Factory tools by handle

- [x] 4.1 Write failing tests first for the factory-tools scenarios "Review task for an implementation" (handle), "Unknown persona", "Display name instead of handle" and "Choosing a reviewer". Then switch `create_task` and `list_personas` in `delegation.rs` to handles, with the error text from design D3. Verify: `cargo nextest run -p df-agent-channel delegation`.
- [x] 4.2 Update the tool descriptions sent to the agent and `product/agent-playbook.md`: personas are addressed by handle, commits carry the persona's identity, and rewriting another persona's commits is rejected. Verify: the tool description test (if any) passes, and the diff of the playbook is reviewed.

## 5. Web UI

- [x] 5.1 Write failing `df-web` tests first for the avatar partial: initials from up to two words, a stable color class for a fixed persona id (FNV-1a), the `aria-label`, and no external URL in the markup. Implement (design D5). Verify: `cargo nextest run -p df-web avatar`.
- [x] 5.2 Write failing tests first for the persona form and cards: the new fields, field errors for invalid values and duplicates (naming the other persona), the hint below the email field that edits apply from the next run, cards ordered by handle with avatar, display name, `@handle` and email. Implement it, including the Alpine avatar preview. Verify: `cargo nextest run -p df-web config_pages`.
- [x] 5.3 Write failing tests first for "Persona as author" in the transcript and for "Persona on task rows and in the task details". Implement, resolving the author from the current persona (design D7). Verify: `cargo nextest run -p df-web`.
- [x] 5.4 Update the e2e fixtures and the demo seed (skill `e2e`): personas get handles and emails, and commit assertions expect the persona identity. Add an e2e test: create a persona and run a task with it, then check the commit author on the forge and the transcript author. Verify: `just e2e-local`.
- [x] 5.5 Document personas: the persona identity fields and their rules in `docs/content/using/setup.md`; a how-to "Give a persona its identity" (setting a real email, what changes when it is edited); `docs/content/using/delegation.md` (handles); `docs/content/security/git-proxy.md` and `docs/content/using/existing-branches.md` (`df:identity` against the run's persona); `docs/content/architecture/agent-runs.md`; `docs/content/operations/upgrades.md` (drain active runs before the upgrade, placeholder emails after it, `agent.commitIdentity` removed). Verify: `just docs-build`.

## 6. Design docs and roadmap

- [x] 6.1 Update `design/agent-run.md` §11.1 (identity check against the run's persona) and add backlog rows to `product/iterations/roadmap.md`: "Route `@handle` mentions to a persona" (idea) and a reference to `persona-commit-signing` in the signing row. Set ADR-0062 to Accepted. Verify: the diff is reviewed, and `openspec validate persona-identity --strict` passes.

## 7. Integration

- [x] 7.1 Run the full checks: `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length`, `bazel --bazelrc=.bazelrc.agents test //...`. Report which container-backed tests could not run. Verify: all are green, or the gaps are listed.

## Workflow follow-up

- Archive the change after review. The follow-up change `persona-commit-signing` builds on it.
