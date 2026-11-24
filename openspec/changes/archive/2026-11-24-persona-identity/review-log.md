# Review log: persona-identity

- BASE: bbc946511b58f52f2d26f2530d7a8c72787a54ad
- Store: none
- Status: done
- Implementer agent: one implementer agent for all rounds (started 2026-11-24)
- DOCKER_HOST: unix:///var/run/docker.sock
- Step: 7 (finished)

## Design (2026-11-24)

- Tokens `color.avatar.{1..8}.{bg,fg}` added (sets light and dark).
- Config - Personas: `Settings · personas, identity`, `Personas · edit modal, identity`, `Personas · new persona, email taken`, `Phone · edit persona sheet, identity`, `Avatar · palette and sizes`.
- Tasks: `Desktop · persona identity`.
- User approved after two fixes (board overlaps, initials not centred). Labels written back into `specs/configuration-management` and `specs/task-ui`; mapping rows in `docs/design-workflow.md`.
- Design commit: c9936007.

## Round 1 (2026-11-24)

- Implementer: d43e982e, 9d32a8d2; tasks 19/19; checks: fmt, clippy, machete, check-file-length/test-names/crates, openspec validate, docs-build ok; cargo tests pass except df-web `installed_packages_have_the_versions_of_package_json` (local htmx 4.0.1 vs package.json 4.0.2, environment); bazel build ok, bazel test of 8 touched crates ok (not `//...`); e2e 46/46. Container tests ran (docker.sock), none skipped.
- Implementer deviations: run identity frozen by a BEFORE INSERT trigger on `runs` (not an explicit copy in `start_queued_runs`); `DbError::PersonaFieldTaken`; app.css cap 56000→58000; `.sqlx` regenerated against a hand-migrated DB; did not open PenPot.
- Reviewed range: BASE...9d32a8d2
- Security: 0 findings (no verify step needed).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P3 | Spec | Blocking | `crates/df-web/scripts/persona-avatar.ts` | Scenario "Avatar preview" has no behavioural test | fix-now | fixed (64ca8284) |
| P5 | Spec | Blocking | `docs/content/operations/upgrades.md:191` | Docs name a Helm value `agent.commitIdentity` that never existed | fix-now | fixed (64ca8284); proposal.md remainder → P8 |
| S1+P2 | Standards+Spec | P1 | `crates/df-core/src/persona.rs:28`, `crates/df-db/src/catalog.rs` save_persona | `PersonaIdentity` unused; `save_persona` does not validate (D4) | fix-now | fixed (64ca8284) |
| S2 | Standards | P1 | `crates/df-db/src/pool.rs:38`, `catalog.rs:638`, `df-web personas.rs:550` | `PersonaFieldTaken.field` is a stringly-typed field | fix-now | fixed (64ca8284) |
| S3+P1 | Standards+Spec | P1 | migration `20261124000001` :74-87, `runs.rs` start_queued_runs, `design.md` D1 | Identity freeze hidden in trigger; D1 text outdated | fix-now (keep trigger, rewrite D1, comment) | fixed (64ca8284) |
| S4 | Standards | P1 | `crates/df-db/src/model_providers.rs:125,285,308` | Touched queries still unchecked | fix-now | fixed (64ca8284) |
| P4 | Spec | P1 | `crates/factory-agent/src/runner.rs:688-703` | No runner-level test for "Assign without identity" | fix-now | fixed (64ca8284) |
| P6 | Spec | P1 | `crates/df-orchestrator/src/run_context.rs:51,131` | `PersonaInfo.handle` unused | fix-now | fixed (64ca8284) |
| P7 | Spec | P1 | migration :55-58 | Email CHECK not in D2 | fix-now (record in D2) | fixed (64ca8284) |
| S8 | Standards | P1 | `crates/df-web/src/assets.rs:~244` | Stale size comment | fix-now | fixed (64ca8284); wording → S10 |
| S5 | Standards | P2→fix | `RunRow`, `RunRecord`, `ActiveRun`, `GitRoute` | Data clump commit_name/commit_email | fix-now (user) | fixed (64ca8284; RunRecord/GitRoute stay flat as row/wire types) |
| S6 | Standards | P3→fix | task_views.rs, chat_views.rs, run_context.rs, persona_avatar.rs | `persona_name`/`name` hold the display name | fix-now (user) | fixed (64ca8284); remainder → S11 |
| S7 | Standards | P3 | `crates/df-db/src/tasks/tests.rs:60-74` | Shared fixture branches on schema | cosmetic | cosmetic |

User decision: fix blocking + P1, plus S5 and S6 (S7 stays cosmetic).

## Round 2 (2026-11-24)

- Implementer: 64ca8284 (fix round 1); reports all 12 items fixed (P3, P5, S1+P2, S2, S3+P1, S4, P4, P6, P7, S8, S5, S6); round-1 rows stay `open` until this round confirms them. Checks green except the htmx env test; e2e persona test run alone (full e2e and bazel test not rerun this round). Container tests ran.
- Reviewed range: 9d32a8d2...64ca8284 plus ledger check
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S11 | Standards | P1 | `crates/df-db/src/lifecycle_events.rs:169,356` | `persona_name` still holds the display name (S6 remainder) | fix-now | open |
| S10 | Standards | P1 | `crates/df-web/src/assets.rs:243` | Size comment contradicts itself, over 100 columns | fix-now | open |
| P8 | Spec | P1 | `openspec/changes/persona-identity/proposal.md:13,46` | Proposal still names a Helm value | fix-now | open |
| P9 | Spec | P1 | `openspec/changes/persona-identity/proposal.md:43` | "assign.rs fills commit_identity from the persona" (it reads the run) | fix-now | open |
| S9 | Standards | P1 | `crates/df-db/src/catalog.rs:753` | Inline test module ~587 lines | fix-now | open |
| S13 | Standards | P1 | `crates/df-db/src/catalog.rs:628` | Hard-to-read doc comment on validation | fix-now | open |
| S12 | Standards | P3 | `crates/df-web/src/config_pages/personas.rs:799-811` | Two matches map to the same field names | cosmetic | cosmetic |
| S14 | Standards | P3 | `crates/df-core/src/persona.rs:146`, `crates/df-agent-channel/src/assign.rs:192` | Domain and proto `CommitIdentity` share a name | cosmetic | cosmetic |
| P10 | Spec | — | renames, `PersonaField`, `CommitIdentity` | Flagged as scope creep | — | rejected (user-approved S2/S5/S6 fixes) |

User decision: fix P1 (S11, S10, P8, P9, S9, S13); S12, S14 cosmetic.

## Round 3 (2026-11-24)

- Implementer: 35559b41 (fix round 2: S11, S10, S9, S13; orchestrator edited proposal.md for P8/P9). Full verification: fmt, clippy, machete, xtask checks, openspec validate, docs-build ok; cargo test 2080 passed, 1 failed (htmx env test only); bazel build ok; bazel test //crates/... //plugins/... 51/51; full e2e 46/46. Container tests ran.
- Orchestrator check: catalog.rs test move is faithful (non-test code differs only in the S13 doc comment; moved tests differ only by rustfmt reflow).
- Reviewed range: 64ca8284...35559b41 plus ledger check
- Security: 0 findings. Spec: 0 findings (P8, P9 fixed; proposal consistent with D1/D6; lifecycle payload key `name` unchanged).
- Ledger: S11, S9, S13 fixed (35559b41); P8, P9 fixed (35559b41); S10 partial → S15.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S15 | Standards | P1 | `crates/df-web/src/assets.rs:243` | Comment 101 columns; "(the cap below)" reads as 57 KB cap | fix-now | fixed (1d936749) |

User decision: fix S15 (comment only, no further review round), then finish.

## Finish (2026-11-24)

- Fix round 3: 1d936749 (S15). fmt, clippy -p df-web ok; df-web assets tests pass except the htmx env test.
- Container tests: all ran against docker.sock in every round; none skipped.
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` and `test //crates/... //plugins/...` (51/51) green at 35559b41; 1d936749 changes only a comment.
- Docs: `just docs-build` green at 35559b41 (no docs changed since). Pages changed: `docs/content/using/setup.md`, `using/delegation.md`, `using/tasks.md`, `using/existing-branches.md`, `security/git-proxy.md`, `architecture/agent-runs.md`, `operations/upgrades.md`, `crates/df-agent-channel/docs/configuration.md`, `crates/df-gateway/README.md`.
- Known unrelated failure: df-web `installed_packages_have_the_versions_of_package_json` (local htmx 4.0.1 vs package.json 4.0.2).
- Tech debt: none (S5, the only P2, was fixed).
- Roadmap: backlog row "Persona avatars in the chat UI" added (iter3-chats archived without it); mention routing and signing rows added by the implementer.
- Cosmetic, not tracked: S7, S12, S14. Rejected: P10.
- Status: done. Fixed: P3, P5, S1+P2, S2, S3+P1, S4, P4, P6, P7, S8, S5, S6, S9, S10/S15, S11, S13, P8, P9.
