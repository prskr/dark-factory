# Review log: iter3-opencode-harness

- BASE: b0422b46078b2e835aec73d20c4301ed6077bb42
- Store: none
- Status: done
- Implementer agent: aef1013a8ac85eb20
- DOCKER_HOST: unix:///run/user/501/podman/podman.sock

## Step: 1. Design (task 7.0)

- Design commit: d4db7380 (approved after two revisions: tabbed provider modal General | Models, scrollable model list, "+ Add model" top right)

## Step: 2. Implement (iteration 1)

- HEAD_1: 6b0bcd19 (commits d6276e45, 6b0bcd19), tasks 25/29 (open: 8.1, 9.2, 9.3)
- Checks: fmt, clippy -D warnings, machete, deny, xtask checks, lefthook pre-commit, cargo test --workspace, just bazel-test 48/48 (with DOCKER_HOST), just docs-build, just e2e-local (one flaky PR-timing failure on the first run, rerun passed)
- Container tests: ran (df-db, df-agent-channel, df-gateway, df-web, image test, e2e)
- Blocked:
  - 8.1: D6 launcher does not work for OpenCode (`opencode acp` re-execs process.execPath = musl loader). Implemented: release binary patched with patchelf (interpreter + RUNPATH), no launcher. Proposed artifact change: amend D6 and the build-pipeline spec.
  - 9.2: manual run not done (needs the opencode image in kind and an e2e flow).
  - 9.3: needs a real OpenCode Go key.
- User decision: 8.1 accept the patchelf approach, amend D6 and the build-pipeline spec via /opsx:update before archive; 9.2/9.3 run manually after the review, stay unticked until then.

## Round 1 (2026-11-05)

- Reviewed range: BASE...6b0bcd19
- Security: 0 findings (no verify needed)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | Blocking | `crates/df-web/src/config_pages/providers.rs:994`, `personas.rs:408` | Conflict kind told apart by message text; persona model conflict lands under field "name" | typed DbError variant, map to right field | fixed (9f3411da) |
| SP4 | Spec | Blocking | `crates/df-web/src/config_pages/providers/models.rs:162` | "Remove a used API" message does not name the model; no df-web test | name model ID, add df-web test | fixed (9f3411da) |
| SP3 | Spec | Blocking | `docs/content/operations/upgrades.md:103` | Says opencode tag default follows chart version (it is 2.2.5) | fix docs | fixed (9f3411da) |
| SP6 | Spec | Blocking | `docs/content/security/inference-endpoint.md:26` | OpenCode config document / placeholder key not described | add opencode sentence + link | fixed (9f3411da) |
| SP5 | Spec | Blocking | `tasks.md` 9.2 | Five opencode-harness scenarios have no test or manual check | add them to the 9.2 checklist | fixed (9f3411da) |
| SP2 | Spec | Blocking (artifact) | `helm-deployment/spec.md` | Spec says empty tag falls back to appVersion; code requires it (correct) | amend spec via /opsx:update with D6 | fixed (7f27f226, artifacts) |
| SP1 | Spec | P1 (artifact) | `tasks.md` 7.1 | Wire API select lists all APIs, server rejects | accept, reword 7.1 via /opsx:update | fixed (7f27f226, artifacts) |
| S9 | Standards | P1 | `crates/df-core/src/config.rs:542`, agent-channel configuration.md | "launcher" wording, image has none | reword | fixed (7e26d7c4, round 2) |
| SP7 | Spec | P1 | `contract/proto/darkfactory/agent/v1/agent.proto:215` | AcpHarness.model comment stale | reword | fixed (9f3411da) |
| S2 | Standards | P1 | `crates/df-db/src/model_providers.rs:199` | Unused pub delegate `personas_using_model` | delete | fixed (9f3411da) |
| S10 | Standards | P1 | `crates/df-core/src/harness/opencode.rs` test | Test named after the design doc | rename | fixed (9f3411da) |
| S4 | Standards | P2→fix-now | `crates/df-agent-channel/src/assign.rs:292`, `harness.rs check_scheme` | Claude-specific branches outside HarnessProfile | move to profile; matters when a 3rd harness is added | fixed (9f3411da) |
| S5 | Standards | P2→fix-now | `config.rs:476/485`, `personas.rs:33`, `PROFILES` | Harness list repeated in several matches | derive from PROFILES; matters when a 3rd harness is added | fixed (9f3411da) |
| S6 | Standards | P2→fix-now | `personas.rs:351`, `providers.rs:878` | Refresh handlers derive mode from echoed form_action string | pass Option<Uuid>; matters when more refresh forms appear | fixed (9f3411da) |
| S3 | Standards | P3 | `crates/df-core/src/harness.rs:58` | fn pointer instead of trait for render | none | cosmetic |
| S7 | Standards | P3 | `providers.rs:868` | op string "remove:<n>" | none | cosmetic |
| S8 | Standards | P3 | `providers/models.rs:18` | PresetModel duplicates ModelLimits shape | none | cosmetic |

User decision: Fix blocking + P1, plus S4, S5, S6 (moved from P2 to fix-now). SP2 and SP1 go to /opsx:update with D6.

## Round 2 (2026-11-05)

- Implementer: 9f3411da, tasks 25/29 (9.2 text extended), checks fmt/clippy/machete/tests/bazel-test 48/48/docs-build/lefthook ok, container tests ran (df-db, df-web, df-agent-channel, Bazel, e2e-local)
- Reviewed range: 6b0bcd19...9f3411da
- Security: 0 findings. Ledger checks: SP3, SP4, SP5, SP6, SP7, S1, S2, S4, S5, S6, S10 verified fixed; S9 not fully fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S9 | Standards | Blocking | `crates/df-k8s/README.md:51` | Still says "launcher" | reword | fixed (7e26d7c4) |
| S15/SP9 | Standards+Spec | Blocking | `crates/df-web/src/config_pages/personas.rs:403` | Persona ModelConflict → model field has no df-web test | test the mapping (unit test of the handler mapping if the form validation blocks every case) | fixed (7e26d7c4) |
| SP8 | Spec | Blocking | `docs/content/operations/upgrades.md:103` | New literal "2.2.5", not covered by check_versions.sh | drop the literal | fixed (7e26d7c4) |
| S11 | Standards | Blocking (regression) | `crates/df-web/src/config_pages/personas.rs:32` | Orphaned doc comment above routes() | delete | fixed (7e26d7c4) |
| S16 | Standards | Blocking | `crates/df-web/src/config_pages/providers/tests.rs:738` | Test name claims persona-use path, asserts core validation message | rename | fixed (7e26d7c4) |
| S12 | Standards | Blocking | `crates/df-core/src/config.rs:65` | Private HarnessEntry between pub type and its impl | move below impl | fixed (7e26d7c4) |
| S14 | Standards | P1 | `crates/df-web/src/config_pages/personas.rs:423` | Reimplements harness::profile() lookup | use harness::profile | fixed (7e26d7c4) |
| S13 | Standards | P3 | `harness.rs:159`, `config.rs entry()` | settings_path and settings_json must stay in sync | none (one harness has settings) | cosmetic |
| S17 | Standards | P3 | `crates/df-core/src/config.rs:566` | Doc line 117 columns | none | cosmetic |

User decision: Fix blocking + P1.

## Round 3 (2026-11-05)

- Implementer: 7e26d7c4, tasks 25/29, checks fmt/clippy/machete/df-web 621/df-core 156/Bazel df-web,df-core,df-k8s/docs-build/lefthook ok, container tests ran (df-web, Bazel); e2e not rerun (no UI behavior change)
- Reviewed range: 9f3411da...7e26d7c4
- Security: 0 findings. Spec: 0 new. Ledger checks: S9, S15/SP9, SP8, S11, S16, S12, S14 verified fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S18 | Standards | Blocking (convergence, nit) | `crates/df-web/src/config_pages/providers/tests.rs:1130` | Test name says "removed" but creates a provider | rename | fixed (afee3864) |
| S19 | Standards | Blocking (convergence, nit) | `crates/df-web/src/config_pages/tests/personas.rs:231` | `use` lines inside the test function | move to file top | fixed (afee3864) |

User decision: fix S18 and S19 (fix round 3), verify by orchestrator, then Finish.

## Round 3 fixes (2026-11-05)

- Implementer: afee3864 (S18, S19), df-web 621 pass, check-test-names/clippy/fmt ok, container tests ran (df-web). Verified by the orchestrator (diff read), no further review round (user decision).

## Finish (2026-11-05)

- Container tests: all ran (no skipped container-backed tests reported).
- Bazel lockfile build: skipped, no Cargo.toml/Cargo.lock change.
- `just docs-build`: ok.
- Tech debt: TD-105 (SP7 note: claude-code persona model not passed to Claude Code, pre-existing).
- Roadmap: backlog rows for the Pi harness and the `/v1/models` import.
- Open before archive: /opsx:update for D6/8.1 (patchelf instead of launcher), SP1 (task 7.1 wording), SP2 (helm spec: empty tag fails the render); manual tasks 9.2 and 9.3.

## Step: /opsx:update (2026-11-05)

- Artifacts commit: 7f27f226 (D6 patched binaries for both images, no launcher; helm tag required; task 7.1 wording; new tasks 8.1 (Claude image), 8.7, 8.8). SP1, SP2 resolved by the artifact change.

## Step: 2. Implement (iteration 2)

- HEAD_2: 823dffff, tasks 27/29 (open: 9.2, 9.3 manual), checks fmt/clippy/machete/deny/xtask/bazel-test 49/49/docs-build/lefthook ok, container tests ran (both harness :test, df-agent-channel); e2e-local not run (uses the fake ACP agent, no claude-harness image)
- Notes: Claude CLI is Bun single-file, runs after patchelf (`bin/claude --version` 2.1.309); unpatched CLI copy remains in node_modules (~200 MB per arch, unused); TDD slip on the CLAUDE_CODE_EXECUTABLE assertion (not run red first)

## Round 4 (2026-11-05, iteration 2)

- Reviewed range: 7f27f226...823dffff
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P10 | Spec | Blocking | `crates/df-core/src/harness.rs:114`, `crates/df-agent-channel/src/assign.rs:350` | CLAUDE_CODE_EXECUTABLE set unconditionally; a project with its own adapter (no harness mount) gets a path that does not exist (launcher only set it when started from the mount) | set only when the harness image is mounted; assign test; spec sentence | fixed (cd584630) |
| P11 | Spec | Blocking | `crates/factory-agent/docs/claude-acp.md:9-13,32` | Task 8.8 ticked but page lacks CLAUDE_CODE_EXECUTABLE and the default command | update page | fixed (cd584630) |
| S22/P12 | Standards+Spec | P1 | `deploy/images/claude-harness/BUILD.bazel:52` | Comment claims the unpatched CLI is not in the image | fix comment | fixed (cd584630) |
| D1 | Spec | P1 | `deploy/images/claude-harness` | Unpatched musl CLI ships unused in node_modules (~200 MB per arch) | exclude from the layer if feasible, else TD | fixed (cd584630) |
| S26 | Standards | P1 | `crates/df-agent-channel/src/assign.rs:726` | CLAUDE_CODE_EXECUTABLE assertion never run red | confirm red, then green | fixed (cd584630) |
| S20 | Standards | P1 | `deploy/images/{claude,opencode}-harness/BUILD.bazel` | Patch genrule, LOADER select and musl runtime files copied | shared Starlark macro | fixed (cd584630) |
| S21 | Standards | P1 | `deploy/images/{claude,opencode}-harness/test.sh` | CLI selection, extract, fail, readelf checks copied | shared shell lib | fixed (cd584630) |
| S23 | Standards | P1 | `deploy/images/claude-harness/BUILD.bazel:46` | Empty SRC gives an unrelated cp error | explicit error | fixed (cd584630) |
| S24 | Standards | P1 | `deploy/images/claude-harness/test.sh:116` | shellcheck disable without reason | add reason | fixed (cd584630) |
| S25 | Standards | P1 | `crates/df-agent-channel/src/assign.rs:511` | Fixture passes half the default adapter command | use CLAUDE_ADAPTER | fixed (cd584630) |
| P13 | Spec | P1 | `deploy/images/claude-harness/test.sh:433` | podman/DOCKER_HOST fallback undocumented in README | document | fixed (cd584630) |

User decision: Fix blocking + P1. Spec sentence for P10 committed by the orchestrator.

## Round 5 (2026-11-05)

- Implementer: cd584630, tasks 27/29, checks fmt/clippy/machete/deny/xtask/shellcheck/bazel-test 49/49/docs-build/lefthook ok, container tests ran (df-agent-channel, both harness :test); D1 done (without.bzl filters the unpatched CLI out of the layer)
- Note: new P10 test not seen red on its assertion (first red was a setup error)
- Reviewed range: 38b12771...cd584630
- Security: 0 findings. Ledger checks: P10, P11, P12, D1, P13, S20-S25 verified fixed; S26 rests on the implementer's statement.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S28 | Standards | Rejected | `crates/df-agent-channel/src/assign.rs:755` | New P10 test assertion never seen red | rejected (spec reviewer: before cd584630 the var was in fixed_vars unconditionally, so the absence assertion fails on the old code) | rejected |
| SP14 | Spec | Blocking (artifact) | `design.md:92`, `tasks.md:51` | D6 and 8.1 still describe CLAUDE_CODE_EXECUTABLE as a fixed var; D6 lacks the excluded unpatched CLI | orchestrator edits design/tasks | fixed (d22e2e6f) |
| S27 | Standards | Blocking (convergence) | `crates/df-agent-channel/src/assign.rs:334` | Doc line 149 columns, missing word | re-wrap, reword | fixed (3e6ee0ca) |
| S29 | Standards | Blocking (convergence, nit) | `crates/df-agent-channel/src/assign.rs:341` | bool flag parameter `harness_mounted` | extend env at the call site | fixed (3e6ee0ca) |
| S30 | Standards | Blocking (convergence, nit) | `deploy/images/claude-harness/without.bzl` | Filters by any path substring | match the package directory segment, clearer name | fixed (3e6ee0ca) |
| S31 | Standards | P3 | `deploy/images/lib.sh:513` | arch_vars sets globals | none (test helper) | cosmetic |

User decision: fix all (SP14 by the orchestrator, S27/S29/S30 by the implementer), verify the diff, then Finish.

## Round 5 fixes (2026-11-05)

- Implementer: 3e6ee0ca (S27, S29, S30), df-agent-channel tests, claude-harness structure_test and :test, bazel-test 49/49, lefthook ok, container tests ran. Verified by the orchestrator (diff read: inference_env has no flag, mounted_vars added at the call site under mounts_harness(); npm_files_without_packages matches whole package path segments). No further review round (user decision).

## Finish, iteration 2 (2026-11-05)

- Container tests: all ran.
- Bazel lockfile build: skipped, no Cargo.toml/Cargo.lock change.
- `just docs-build`: ok.
- Tech debt: none new (TD-105 from iteration 1 stands). Roadmap: unchanged.
- Open: manual tasks 9.2 and 9.3 (user).

## Step: manual checks and artifact update (2026-11-05)

- Manual results recorded in task 9.2 (838d43f8). A repository opencode.json provider is contacted at start; OpenCode does not start when that host is denied. User decision: keep repository config (no OPENCODE_DISABLE_PROJECT_CONFIG); spec scenario reworded, new scenario "Provider defined in the repository", D7 amended, new tasks 8.9 (debug config test) and 8.10 (guide note).
- Model dialog layout fix: e96f4520 (orchestrator; fields widths on wrappers, app.css regenerated).

## Step: 2. Implement (iteration 3)

- HEAD_3: a5ec9379, tasks 8.9 and 8.10 done, checks fmt/clippy/df-core 157/bazel-test 49/49/docs-build/shellcheck/lefthook ok, container tests ran (opencode-harness :test)
- User decisions: harness stderr in the run error skipped (cause is in the run's logs); wider provider dialog now (design first); other follow-ups to the roadmap backlog (Go preset check, own adapter run, Claude settings env check). Spec "no other host" requirement and design spike row corrected by the orchestrator; 9.3 marked postponed.

- Design: provider modal widened to 896 px on all six "Config - Model providers" boards (approved); task 7.6 ticked, task 7.7 open.

- HEAD: 24c0aaf7 (task 7.7, DialogStyle::Wider, screenshots checked by the orchestrator), 214ca946 (Wire API column w-48, orchestrator)

## Round 6 (2026-11-05, iterations 3 and dialog)

- Reviewed range: 82a9ee5f...214ca946 (code: e96f4520, a5ec9379, 24c0aaf7, 214ca946; artifacts 838d43f8, 16af049d, 0c9fd399)
- Security: 0 findings (D7 checked against the inference endpoint, egress and credential boundaries).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP17 | Spec | Blocking | `docs/content/using/opencode.md:35` | Guide still says no host besides the gateway; contradicts its new section | add the repository-provider exception | fixed (f32c2928) |
| SP18 | Spec | Blocking (artifact) | `proposal.md` | Proposal not updated to D7 (kept customization, provider exception, MCP wording) | orchestrator edits | fixed (bb39f248) |
| SP16 | Spec | P1 (artifact) | `specs/opencode-harness/spec.md:23` | "follow the repository's" is ambiguous | "come after the repository's and override them" | fixed (bb39f248) |
| SP19 | Spec | P1 (artifact) | `design.md` risks, open questions | Still says the Go check is in this change's manual run | point to the backlog row | fixed (bb39f248) |
| SP15 | Spec | P1 (artifact) | `tasks.md` 9.2 | "model requests only to the inference endpoint" with an allowed rogue host never observed | state it rests on the merge order, or a manual run | fixed (bb39f248) |
| SP20 | Spec | P1 (artifact) | `tasks.md` 9.2 | Plugin not loaded / registry not contacted not recorded | record as not checked, or a manual check | fixed (bb39f248) |
| SP22 | Spec | P3 | `product/iterations/roadmap.md:130` | Claude settings row cites D7 | cite 9.2 | fixed (bb39f248) |
| SP21 | Spec | P1 | `deploy/images/opencode-harness/test.sh:11` | Header says both modes run debug config; only --acp does; structure_test gets an unused arg | fix comment, drop arg | fixed (f32c2928) |
| S32 | Standards | P1 | `crates/df-core/src/harness/opencode.rs:72` | Expected document duplicated in a test and the fixture | compare against the fixture | fixed (f32c2928) |
| S34 | Standards | P1 | `crates/df-web/src/config_pages.rs:366` | tabbed_form doc names only DialogStyle::Wide | reword | fixed (f32c2928) |
| S33 | Standards | P3 | `crates/df-web/src/components/overlay.rs:45` | Wider copies the Wide class string | doc note to keep in step | fixed (f32c2928) |

User decision: fix all, then finish. Artifacts by the orchestrator; SP17, SP21, S32, S33, S34 by the implementer.

## Round 6 fixes and finish (2026-11-05)

- Implementer: f32c2928 (SP17, SP21, S32, S33, S34), df-core 156, df-web 622, opencode-harness structure_test and :test, docs-build, lefthook ok, container tests ran. Orchestrator: 0a9a6d10 (doc comment re-wrapped to 100 columns). Verified by the orchestrator (diff read); no further review round (user decision).
- Bazel lockfile build: skipped, no Cargo.toml/Cargo.lock change. `just docs-build`: ok.
- Tech debt: none new. Roadmap: Go preset check, own adapter run, Claude settings env check (16af049d).
- Open: task 9.3 (postponed, backlog). Task 9.2 results recorded with the checks not done listed.

## Step: TD-105 fix (2026-11-05)

- User decision: fix TD-105 in this change (ANTHROPIC_MODEL from the persona model). Spec "Harness start" and scenario "Claude Code uses the persona's model" added, task 8.11.
- Implementer: 8024d3ab (task 8.11, HarnessProfile.model_var, TD-105 deleted), df-core 156, df-agent-channel 248+27+1, bazel-test 49/49, docs-build, lefthook ok, container tests ran

## Round 7 (2026-11-05)

- Reviewed range: 61c639ad...8024d3ab
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P23 | Spec | Blocking | `crates/factory-agent/docs/configuration.md:62` | Harness env list lacks ANTHROPIC_MODEL | add | fixed (750a4a84) |
| P25 | Spec | P1 (artifact) | `design.md` D3, `proposal.md:12` | Profile fields lack the model variable | add | fixed (750a4a84) |
| S35 | Standards | P1 | `crates/df-agent-channel/src/assign.rs:336` | Doc line 108 columns | re-wrap | fixed (750a4a84) |
| S36 | Standards | P1 | `crates/df-agent-channel/src/assign.rs:711` | harness_env_of_run undocumented side effect | doc comment | fixed (750a4a84) |
| P24 | Spec | P1 | `docs/content/security/inference-endpoint.md:26` | Claude Code variable list lacks ANTHROPIC_MODEL | add | fixed (750a4a84) |
| P27 | Spec | P1 | `crates/df-agent-channel/src/assign.rs:770` | Test did not check each setup took effect | assert key var and CLAUDE_CODE_EXECUTABLE per setup | fixed (750a4a84) |
| P26 | Spec | Rejected | `docs/content/using/model-providers.md:73` | Extra doc corrections beyond 8.11 | rejected (they fix earlier drift, keep) | rejected |

- Fixes by the orchestrator (small, docs and one test assertion), tests/clippy/docs-build/validate ok; no further review round. TD-105 closed.

- Usage view after a claude-code run shows the persona model (user, 2026-11-05). Task 9.2 ticked; 9.3 postponed.
