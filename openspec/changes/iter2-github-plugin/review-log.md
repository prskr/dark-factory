# Review log: iter2-github-plugin

- BASE: 8456277390bc7df407c7f18b7f6273a648a3bb8c (after the PR #42 review the branch was rebased onto 4cbae25, which adds task 2.6 and optional task 8.1)
- Store: none
- Status: in progress (sections 1–5, 6.1, 6.2, 6.4, 6.5 done; 6.3 and 7.1 code and live parts done, dev-cluster checks open; optional section 8 in progress)
- Implementer agent: fresh agent per run (stateless subagents)
- Scope: sections 1–5 (tasks 1.1–5.1); sections 6–8 on `iter2/github-plugin-routes` (section "Sections 6–7")
- DOCKER_HOST: unix:///var/run/docker.sock

## Implementation (2026-10-30)

One implementer subagent per chunk (the environment ends subagents after about 10 minutes without a report; two full-scope runs failed, the second left partial work that chunk 1a reused).

| Chunk | Tasks | Commit | Checks |
|---|---|---|---|
| 1a crate, settings, transport, `get` | 1.1, 2.1 | e96d3f4 | cargo fmt/clippy (host, wasm)/test, check-crates; Bazel build/lint/fmt/test of `//plugins/github/...` run by the orchestrator after the Xcode license was accepted |
| 1b error and rate-limit mapping | 2.2 | ecb857b | cargo fmt/clippy/test |
| 2a `create`, `find-open` | 2.3 (part) | fc96f42 | cargo, MODULE.bazel.lock regenerated |
| 2b issues | 2.3 | 0e1e71b | cargo |
| 3 `update`, native drafts, GraphQL errors | 2.4 | (squashed) | cargo |
| 4 README | 2.5 | 69bed09 | settings and error mapping checked against the code |
| 5 no early retries in the host | 3.1 | 83be636 | cargo, `--test forgejo` |
| 6 GitHub component in host tests | 3.2 | 7aa1b84 | cargo, Bazel `df_plugins_load_test` |
| 7 host tests against a mock GitHub | 3.3 | 3be801f | cargo, Bazel `df_plugins_github_test` |
| 8 plugin image | 4.1 | 457c3dc | Bazel image targets, `df_plugins_image_component_test`; fmt, clippy, machete, buildifier, shellcheck and `--nobuild //deploy:push_images` run by the orchestrator; `images_structure_test` is Linux-only (CI) |

Environment notes: lefthook hooks are not installed in this clone, so commits ran without hooks; the checks above were run explicitly. No `BUILDBUDDY_API_KEY`: Bazel runs without the remote cache.

## Round 1 (2026-10-30)

- Implementer: 457c3dc, tasks 11/17 (sections 1–4 done; 5.1 open; 6–7 wait for `iter2-multi-forge`)
- Reviewed range: 8456277...457c3dc, split into four reviewers (Standards: plugin code / tests, host, deploy; Spec: `github-integration` / host, build, image)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | P1 | `plugins/github/src/api.rs:174` | private `draft_change`/`graphql` between public methods | fix-now | fixed (187ce74) |
| S2 | Standards | P1 | `plugins/github/src/error.rs:121` | `impl Body` not next to `struct Body` | fix-now | fixed (187ce74) |
| S5 | Standards | P1 | `plugins/github/src/api.rs:326` | owner/name go into the path unencoded | fix-now | fixed (0b0c08b) |
| S7 | Standards | P1 | `plugins/github/src/api.rs:163` | guard-only `match` in `update` hard to follow | fix-now | fixed (187ce74) |
| S9, P8 | Both | P1 | `plugins/github/src/api.rs:345` | missing `node_id` sends the mutation with `""` | fix-now | fixed (0b0c08b) |
| S101, S102, P104 | Both | P1 | `docs/development.md:68`, `crates/df-plugins/README.md:81` | `GITHUB_PLUGIN_WASM` not documented | fix-now | fixed (7be3833) |
| S109 | Standards | P1 | `plugins/github/src/api/tests.rs:581` | constants mid-file, URL literal repeated | fix-now | fixed (187ce74) |
| S112 | Standards | P1 | `plugins/github/src/api/tests.rs:568` | test name claims more than it asserts | fix-now | fixed (187ce74) |
| P1 | Spec | P1 | `plugins/github/src/error.rs:78` | any 422 "already exists" becomes `conflict` | fix-now | fixed (0b0c08b) |
| P2 | Spec | P1 | `plugins/github/src/settings.rs:45` | empty `wip-prefixes` creates a ready PR in `title-prefix` mode | fix-now | fixed (0b0c08b) |
| P7 | Spec | P1 | `plugins/github/src/api.rs:182` | no test for title + mutation, or `draft: true` on a draft | fix-now | fixed (187ce74) |
| P101 | Spec | P1 | `crates/df-plugins/src/retry.rs` | no test for the 1 s wait without a hint | fix-now | fixed (7be3833) |
| P102 | Spec | P1 | `crates/df-plugins/src/retry.rs` | no test for 3 retries of `rate-limited` | fix-now | fixed (7be3833) |
| S115, S116 | Standards | P2 | `deploy/BUILD.bazel:118`, `deploy/test_images.sh:97` | plugin image targets and checks copied per plugin; matters when a third plugin image is added | tech-debt | open |
| S3, S6, S8, S10, S11, S12, S105, S106, S107, S108, S110, S111, S113, S114, S117, P3, P4, P5, P6, P103 | Both | P3 | various | naming, small duplications, edge cases GitHub does not produce (HTTP-date `retry-after`, caller title already prefixed) | cosmetic | cosmetic |
| S4 | Standards | Rejected | `plugins/github/src/settings.rs:57` | use `url` | rejected (base-url comes from the host; `url` adds IDNA tables to a component capped at 1 MiB) | rejected |
| S103 | Standards | Rejected | `crates/df-plugins/tests/load.rs:65` | capability test in `load.rs` | rejected (task 3.2 asks for it there) | rejected |
| S104 | Standards | Rejected | `load.rs`, `image_component.rs` | duplicated capability test | rejected (tasks 3.2 and 4.1 each require one; they load the dev and the image build) | rejected |

User decision: Fix blocking + P1 (no blocking findings; all P1 fix-now, P2 to tech debt at the end)

## Round 2 (2026-10-30)

- Implementer: fix round 1 in three parts, 0b0c08b, 187ce74, 7be3833; cargo fmt, clippy (host, wasm), tests green; no container-backed tests in scope
- Reviewed range: 457c3dc...7be3833, plus the ledger check: all 13 fix-now rows fixed

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S201 | Standards | P3 | `plugins/github/src/api.rs:19` | `QUERY_VALUE` now also encodes path segments | cosmetic | cosmetic |
| S202 | Standards | P3 | `plugins/github/src/api.rs:173` | `current.filter(\|_\| !mutated)` | cosmetic | cosmetic |
| S203 | Standards | P3 | `crates/df-plugins/src/retry.rs:79` | two wait tests could be one table | cosmetic | cosmetic |
| S204 | Standards | P3 | `crates/df-plugins/src/retry.rs:98` | older `gives_up_after_three_retries` now ambiguous | cosmetic | cosmetic |
| P201 | Spec | P1 | `design.md` D5 | design still said any "already exists" in a 422 means `conflict` | fixed by the orchestrator: D5 now names "A pull request already exists", like the spec | fixed |

No blocking finding open after round 2.

## Finish (2026-10-30)

- Container-backed tests: none in scope (`crates/df-plugins`, `plugins/github` use none).
- Bazel: `--lockfile_mode=error --nobuild //crates/... //plugins/...` ok; `bazel test //plugins/github/... //crates/df-plugins/... //deploy/...`: 9 passed, `images_structure_test` skipped (Linux only, CI), `df_plugins_github_test` failed:

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| B1 (S107) | Bazel run | Blocking | `crates/df-plugins/tests/github.rs:296` | rate-limit host test asserted 590–600 s; a 27 s first-call compile under load gave 573 s | fix-now | fixed (57b1415), Bazel target passes |

- Coverage (task 5.1): `github-plugin` 76.6 % lines (`lib.rs` and `wasi_transport.rs` run only as Wasm, covered by the host tests), `df-plugins` 93.7 %.
- Tech debt: TD-77 (S115, S116). No new roadmap rows: sections 6–7 stay in this change.
- Fix rounds: 2 (round 1 in three parts, round 2 for B1).

## Round 3: PR #42 review follow-up (2026-10-30)

Peter's threads on PR #42: `wiremock` wording (the in-process Rust crate) and OpenAPI validation as optional task 8.1; draft-mode `auto` added as a third option (the user kept `title-prefix` as the default). Artifacts updated in dd552be and 4cbae25 on `iter2/multi-forge`.

- Implementer: task 2.6 (`auto` draft mode), committed by the orchestrator with the lefthook pre-commit hooks (installed now)
- Reviewed range: 24d167c...c5be754 (before the rebase)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S301, S303 | Standards | P1 | `plugins/github/src/api.rs:102`, `settings.rs:51` | `DraftMode` checked with `!=` in four places; match arm with two outcomes | fix-now | fixed (fix round 3) |
| S304, P301 | Both | P1 | `plugins/github/README.md` | the host forwards plugin stderr at `info`, not `warn` | fix-now | fixed (fix round 3, README note) |
| S306 | Standards | P1 | `plugins/github/src/api/tests.rs:185` | test name no longer covers the parsing assertions | fix-now | fixed (fix round 3) |
| P302 | Spec | P1 | `product/iterations/iteration-2.md:54` | overview lists only `title-prefix` and `native` | fix-now | fixed (4cbae25, also `roadmap.md`) |
| S302, S305, P303 | Both | P3 | `api.rs:235`, `wasi_transport.rs` | bool parameter, module name, `StderrLog` untested | cosmetic | cosmetic |

User decision: Fix blocking + P1. No blocking finding open.

## PR #44 review by Peter (2026-10-30)

| ID | Class | Where | Finding | Decision | Status |
|----|-------|-------|---------|----------|--------|
| S1 | P1 | `crates/df-plugins/README.md:63` | "Retries" still says `retry-after` is capped at 30 s; above 30 s it is returned without retrying | fix-now | fixed (pending commit) |
| S2, S3 | P2 | `plugins/github/src/draft.rs`, `wasi_transport.rs` | draft and transport helpers copied from `plugins/forgejo` | tech-debt | tech-debt (TD-80) |
| S7 | P1 | `crates/df-plugins/tests/github.rs:292` | `let … else { panic!() }` guard; `unwrap_or_default()` turns a missing `retry-after` into 0 | fix-now | fixed (pending commit) |

Task 6.1 done in the same round: `tests/github.rs` routes `github.example` to the mock through `apiUrl` with `authScheme: bearer` and asserts `Authorization: Bearer`.

## Sections 6–7 (2026-10-30)

Branch `iter2/github-plugin-routes` from `fd5cf70` (main after PR #44).

| Task | Commit | Checks |
|---|---|---|
| 6.2 live test | 3ed5e0b | compiles, skipped by default; Bazel `df_plugins_github_live_test` builds |
| 6.4 chart default values | 2f1b6ea | helm-unittest, README values check |
| 6.3 dev deploy (code) | 4bdafbf | shellcheck; dev-cluster check open (no cluster access), `bazel build --nobuild //deploy:dev` fails on macOS on main too (`musl_runtime_linux_arm64` tarball with a case-only hardlink) |
| 6.5 dev config, configuration guide | 5365656 | serve-local steps run by hand (`just` not installed): PostgreSQL from `compose.yml` on host port 55432 because another project's container holds 5432; log `forge route host="github.com" plugin=github available=true`, `default forge route plugin=forgejo`, web UI answers 303. A LibreSSL 3.3 key in `target/local-dev` failed to parse (`agent.key`), and its v1 certificate was rejected; regenerated with OpenSSL 3 |
| 7.1 live part | 249ee64 | `tests/github_live.rs` on github.com: private repository of a GitHub Free account with `title-prefix` (`WIP:` renames) and `auto` (native draft, `ready_for_review`), public repository with `native`; all pass. Dev-cluster run open |

Review over `fd5cf70...249ee64`, one Standards and one Spec reviewer:

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | P1 | `design.md` D2, `proposal.md`, READMEs | "private repositories on GitHub Free and Pro have no draft pull requests" is contradicted by the live run (GitHub Free account, private repository, native draft); GitHub's plan docs no longer list drafts as a paid feature | fix-now: user decision, default `draft-mode` becomes `native`; `title-prefix` and `auto` stay options | fixed (843aa51 code, 7ddac93 docs, spec, design) |
| S1 | Standards | P1 | `crates/df-plugins/tests/github_live.rs:42` | client setup copied from `tests/github.rs`, `var` from `homelab.rs` | fix-now | fixed (45b24a5) |
| S2 | Standards | P1 | `crates/df-plugins/tests/common/mod.rs:199` | `named_https_router` repeats `named_router` | fix-now | fixed (45b24a5) |
| S3, P4 | Both | P1 | `crates/df-plugins/tests/github_live.rs:11` | `DF_TEST_GITHUB_API_URL` with the route host fixed to `github.com` | fix-now: remove the variable | fixed (45b24a5) |
| S4 | Standards | P1 | `crates/df-plugins/tests/github_live.rs:41` | test named after the topic | fix-now | fixed (45b24a5) |
| S5 | Standards | P1 | `deploy/dev.sh:14` | `PG_*` reads as PostgreSQL | fix-now | fixed (84d405a) |
| S7, P7 | Both | P1 | chart README "GitHub", `configuration.md` | draft-mode and GHES explanations in four places, chart copy drifted; separate GHES entry shows the generic icon and hint | fix-now | fixed (84d405a) |
| S8 | Standards | P1 | `deploy/helm/dark-factory/values.yaml:167` | github entry without `callTimeoutSeconds`, `memoryMib` and README rows | fix-now | fixed (84d405a) |
| S9 | Standards | P1 | `deploy/helm/dark-factory/tests/chart_test.yaml:1132` | github assertion duplicated in two tests | fix-now | fixed (84d405a) |
| P2 | Spec | P1 | `crates/df-plugins/tests/github_live.rs:124` | no issue comment, unlike `homelab.rs` | fix-now | fixed (45b24a5, live re-run) |
| P3 | Spec | P1 | `crates/df-plugins/tests/github_live.rs:88` | draft form not distinguishable by the test | fix-now: module doc names the manual timeline check | fixed (45b24a5) |
| P5 | Spec | P1 | `deploy/helm/dark-factory/tests/values/plugin-routing.yaml:7` | "Routing values" scenario with an `apiUrl` map has no test | fix-now | fixed (84d405a) |
| P6 | Spec | P1 | `dev/config.yaml` | serve-local run not recorded | fix-now | fixed (this section) |
| P8 | Spec | P1 | `plugins/github/README.md:79` | "mock tests cover the fallback": only the unit tests do | fix-now | fixed (7ddac93) |
| P9 | Spec | P1 | `crates/df-plugins/README.md:74` | no command for `github_live` | fix-now | fixed (45b24a5) |
| P10 | Spec | P1 | `crates/df-plugins/docs/configuration.md:207` | hint text differs from the UI ("Served by the GitHub integration") | fix-now | fixed (84d405a) |
| S6 | Standards | P2 | `deploy/dev.sh:9` | positional argument per plugin image shifts all later indexes | tech-debt (TD-77) | tech-debt (TD-77 extended, 84d405a) |

User decision: fix all as proposed; GitHub account plan is Free; default `draft-mode` becomes `native`.

Fix round (one implementer per chunk): 843aa51 (default in the plugin, tests), 7ddac93 (spec, design, proposal, product, READMEs, guide), 45b24a5 (test helpers, live test), 84d405a (chart, dev deploy, guides, TD-77). Checks: cargo fmt, clippy (host, wasm), `github-plugin` 68 unit tests, `df-plugins --test github` 4, all test binaries compile, helm-unittest 70/70, `//deploy:helm_chart_test`, shellcheck, typos, `openspec validate --strict`.

Live re-run with `DF_TEST_GITHUB_ISSUE` (2026-10-30): private repository with `native` (default; `ready_for_review`), `title-prefix` (`WIP:` renames) and `auto` (`ready_for_review`), public repository with `native`; all pass, issue comments present. Pull requests closed afterwards.
