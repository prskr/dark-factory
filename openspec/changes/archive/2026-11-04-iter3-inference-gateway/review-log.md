# Review log: iter3-inference-gateway

- BASE: 5a03c6b6d3c219338be0fff0199609a94b5f089f
- Store: none
- Status: done
- Implementer agent: ad3a5d94b54a4d118
- DOCKER_HOST: unix:///run/user/501/podman/podman.sock
- Design tasks: none
- Tasks: 28/31 (open: 1.1, 6.3 need a real Claude Code and token; 2.6 guide not walked through `just serve-local`)
- Tech debt created: TD-102, TD-103. Roadmap backlog rows: provider adapters as WASI plugins (F5), allowed models per provider.
- Deferred spec-delta batch for `/opsx:update`: SPEC-4 (Accept-Encoding/Content-Length handling), SPEC-11 (403 without route, refused records counted as dropped), SPEC-17 (timeouts: 504 head, 408 upload, response idle), SPEC-21 (fake agent env dump and key line), git-proxy delta for the `forge.allowPlainHttp` → `network.allowPlainHttp` rename (living `openspec/specs/git-proxy/spec.md`).

## Round 1 (2026-11-04)

- Implementer: 7a31c189 (b6a3e9d2, 61ec81f6, 36527f4d, e8a63737, 7a31c189), tasks 28/31, checks fmt/clippy/machete/deny/xtask/bazel 19 targets/docs-build/sqlx check/e2e ok, cargo test 1982 passed, llvm-cov df-gateway 94.6 %; `cargo xtask outdated` fails on pre-existing kube/rpi-*; container tests ran
- Reviewed range: BASE...7a31c189
- Security (find): 0 findings

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| STD-1 | Standards | Blocking | `crates/df-gateway/src/inference/mod.rs:168`, forward.rs | No response-head timeout, no body idle timeout | fix-now | fixed (c9e5e9d6, 7f5a1864) |
| SPEC-1 | Spec | Blocking | `crates/df-core/src/model_provider.rs:238` | `header` scheme rejected `api-key`/`authorization` | fix-now | fixed (c9e5e9d6) |
| SPEC-6 | Spec | Blocking | `docs/content/operations/upgrades.md` | Upgrade note for the persona migration missing in published docs | fix-now | fixed (c9e5e9d6) |
| SPEC-2 | Spec | Blocking (user) | tasks 1.1, 6.3 vs group 7 | Runtime usage removed before manual Claude Code verification (D9) | user | fixed (manual dev-cluster run 2026-11-04: streaming works, usage recorded per request, sandbox holds only the placeholder) |
| STD-2 | Standards | P1 | `inference/mod.rs`, `git/upstream.rs` | connect/handshake/spawn duplicated | fix-now | fixed (c9e5e9d6) |
| STD-3 | Standards | P2 → fix-now | `git/upstream.rs` | forge-named HTTP client carries model traffic | fix-now | fixed (c9e5e9d6) |
| STD-4 | Standards | P1 | 4 sites | hand-written WireApi ↔ pb matches | fix-now | fixed (c9e5e9d6, 7f5a1864) |
| STD-5 | Standards | P1 | header_template.rs, headers.rs | header lists duplicated | fix-now | fixed (c9e5e9d6) |
| STD-6 | Standards | P1 | model_provider.rs, headers.rs | scheme → header name duplicated | fix-now | fixed (c9e5e9d6) |
| STD-7 | Standards | P1 | harness.rs, route.rs | `/inference` twice | fix-now | fixed (c9e5e9d6) |
| STD-8 | Standards | P1 | report/tests.rs | sleep-based test | fix-now | fixed (c9e5e9d6) |
| STD-9 | Standards | P1 | fake upstream, inference/tests.rs | polling sleeps, port rebind | fix-now | fixed (c9e5e9d6, 7f5a1864) |
| STD-11 | Standards | P1 | report.rs:233 | Notify enable order | fix-now | fixed (c9e5e9d6) |
| STD-12 | Standards | P1 | df-db model_providers.rs:47 | unused `upstream_url` | fix-now | fixed (c9e5e9d6) |
| STD-15 | Standards | P1 | forward.rs, providers.rs | pub after private | fix-now | fixed (c9e5e9d6, 7f5a1864) |
| SPEC-3 | Spec | P1 | providers.rs:99 | GitHub Models preset URL wrong | fix-now | fixed (c9e5e9d6; preset removed, proposal/spec edited) |
| SPEC-5 | Spec | P1 | docs | plain-http model upstreams undocumented | fix-now | fixed (c9e5e9d6) |
| SPEC-7 | Spec | P1 | df-e2e flow.rs | e2e didn't check sandbox env | fix-now | fixed (c9e5e9d6, 7f5a1864, 779bbbf9) |
| SPEC-9 | Spec | P1 | contract/README.md, transcript.proto | wrong wording on old fields | fix-now | fixed (c9e5e9d6) |
| SPEC-10 | Spec | P1 | control/tests.rs | "other entries unaffected" untested | fix-now | fixed (c9e5e9d6) |
| SPEC-12 | Spec | P1 | providers/tests.rs | no secret-absence assertion | fix-now | fixed (c9e5e9d6, 7f5a1864) |
| SPEC-13 | Spec | P2 → fix-now | harness.rs:19 | CA path not tied to mount | fix-now | fixed (c9e5e9d6) |
| SPEC-4 | Spec | P3 | inference-endpoint spec | header handling not in spec delta | deferred /opsx:update | cosmetic |
| SPEC-11 | Spec | P3 | inference-endpoint spec | 403/no route, refused records not in spec | deferred /opsx:update | cosmetic |
| SPEC-8 | Spec | P3 | fake agent model step | not streamed | | cosmetic |
| STD-10 | Standards | P3 | fake-acp-agent upstream.rs | unowned spawns, no read timeout (test tool) | | cosmetic |
| STD-13 | Standards | P3 | usage_records.rs:18 | counts as string tuples | | cosmetic |
| STD-14 | Standards | P3 | assign.rs:10 | shadowed alias | | cosmetic |
| STD-16 | Standards | P3 | fake-acp-agent upstream.rs:31 | `tracing_free_log` | | cosmetic |

User decision: fix Blocking + P1 + STD-3 + SPEC-13. SPEC-2 stays with the user; P3 ledger only.

- Fix round 1: c9e5e9d6

## Round 2 (2026-11-04)

- Reviewed range: 7a31c189...c9e5e9d6
- Security (find): 0 findings

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| STD-17 + SPEC-15 | Standards, Spec | Blocking | `inference/mod.rs:143,189` | stalled upload reported as provider unreachable, untested | fix-now | fixed (7f5a1864) |
| SPEC-14 | Spec | P1 | fake agent, flow.rs | e2e checked only one env var | fix-now | fixed (7f5a1864) |
| SPEC-16 + STD-22 | Spec, Standards | P1 | providers/tests.rs | vacuous assertion | fix-now | fixed (7f5a1864) |
| STD-18 | Standards | P1 | upstream.rs | "request body" messages on responses | fix-now | fixed (7f5a1864) |
| STD-19 + STD-20 | Standards | P1 | route.rs | conversions in routing module | fix-now | fixed (7f5a1864) |
| STD-21 | Standards | P3 → fix-now | headers.rs, forward.rs | pass-throughs | fix-now | fixed (7f5a1864) |
| SPEC-17 | Spec | P1 (docs) / P3 (spec) | inference-endpoint.md | head timeout wording; timeouts not in spec | fix-now (docs), deferred (spec) | fixed (7f5a1864) / cosmetic |
| SPEC-18 | Spec | P1 | design.md:102 | GitHub Models leftover | fix-now | fixed (7f5a1864) |
| STD-24 | Standards | P1 | upstream.rs, main.rs | forge-only wording | fix-now | fixed (7f5a1864) |
| STD-23 | Standards | P3 | report/tests.rs:519 | error variant in fake | | cosmetic |
| STD-25 | Standards | P3 | git/upstream/tests.rs:4 | import grouping | | cosmetic |

User decision: fix Blocking + P1 + STD-21.

- Fix round 2: 7f5a1864

## Round 3 (2026-11-04)

- Reviewed range: c9e5e9d6...7f5a1864
- Security (find): 0 findings (noted pre-existing `spawn_harness` env inheritance)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SPEC-19 | Spec | Blocking | `inference/tests.rs:423` | refused-connection scenario lost its test | fix-now | fixed (779bbbf9) |
| SPEC-20 | Spec | Blocking (docs) | `df-gateway/README.md:144` | plain-HTTP row omitted inference | fix-now | fixed (779bbbf9) |
| STD-28 | Standards | P1 (breaking) | main.rs, Helm | `forge.allowPlainHttp` → `network.allowPlainHttp` (user chose the name; setting also governs forge plugin API URLs) | fix-now | fixed (779bbbf9) |
| STD-26 | Standards | P1 | docs/demo.md | env dump undocumented | fix-now | fixed (779bbbf9) |
| STD-29 | Standards | P1 | df-gateway lib.rs | `pub mod convert` | fix-now | fixed (779bbbf9) |
| STD-30 | Standards | P1 | inference/testing.rs | dead arm waits forever | fix-now | fixed (779bbbf9) |
| SPEC-14b | Spec | P1 | flow.rs | forge token not checked | fix-now | fixed (779bbbf9) |
| TD-env | Security | P2 | factory-agent runner.rs | harness inherits full runtime env (pre-existing) | tech debt | tech-debt (TD-102) |
| SPEC-21 | Spec | P3 | fake-acp-agent spec | env dump not in spec | deferred /opsx:update | cosmetic |
| STD-31 | Standards | P3 | inference/mod.rs:448 | data clump | | cosmetic |
| STD-27 | Standards | — | fake agent agent.rs:430 | redact env dump values | | rejected (values are the check; test-only binary; security found no exposure) |

User decision: fix Blocking + P1 incl. STD-28 rename to `network.allowPlainHttp`.

- Fix round 3: 779bbbf9 (e2e first run flaked in `follow_up_resumes_the_session_and_completing_marks_the_pr_ready`, rerun 9/9)

## Round 4 (2026-11-04)

- Reviewed range: 7f5a1864...779bbbf9
- Security (find): 0 findings

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| STD-32 | Standards | P1 | `dark-factory/src/forge.rs:116` | stray doc comment | fix-now | fixed (74db8193) |
| SPEC-22 | Spec | P1 | upgrades.md:66 | hand-written config case missing | fix-now | fixed (74db8193) |
| TD-e2e | Spec | P2 | `df-e2e/tests/flow.rs:229` | follow-up test flake (pre-existing) | tech debt | tech-debt (TD-103) |
| STD-33 | Standards | P3 | values.yaml:211 | long comment line | | cosmetic |

User decision: fix P1 nits, then finish.

- Fix round 4: 74db8193 (docs-only; fmt, clippy -p dark-factory, docs-build ok)

## Manual test follow-up (2026-11-04)

- Dev-cluster run by the user: `ANTHROPIC_BASE_URL` and `df-placeholder` confirmed; the model's `env | sort` omitted `CLAUDE_CODE_OAUTH_TOKEN` (Claude Code likely drops its own credential vars for the Bash tool), `/proc/<pid>/environ` shows the placeholder.
- Fix round 5: 516e3793 (docs only): inference and Git traffic are not in the network graph or egress log (only the egress proxy writes flow events); hint for `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` via `ENV` in the sandbox image. TD-102 notes the dependency.
- User decision: set `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` by default in the claude-code harness constants, with an acp-harness spec delta. Fix round 6: da396a97 (spec delta acp-harness and run-credentials, `fixed_vars` in df-core harness entry, tests, docs). Diff read by the orchestrator, no reviewer round (small, test-first, no new logic).
- Task 1.1 ticked by user decision: interactive session on the dev cluster works through the gateway; the network graph shows no path information, so no per-path list was recorded. Task 6.3 ticked: the user confirmed streaming works and token usage was recorded for the run. SPEC-2 fixed.
- Design: the change had no PenPot task; the user asked for boards as part of this change (step 1 of the skill, done after implementation). Approved by the user: Rail item and variant "Model providers"; Model providers · table; new modal (Claude subscription preset, custom header) after one iteration (denser rows, scheme-driven disabled fields, split value with example token, extra headers disabled for claude-subscription); persona cards, edit modal (with hint) and phone sheet; run card with requests without usage. Boards stay on Config until the user splits the page. New task 2.4b aligns df-web with the boards.

## Finish (2026-11-04)

- Container tests: ran in every round (cargo, Bazel, e2e) on the Podman socket; none skipped
- `bazel --bazelrc=.bazelrc.agents build --lockfile_mode=error //crates/... //plugins/...`: ok
- `just docs-build`: ok
- Status (superseded): stopped while SPEC-2 was open; reopened after the manual test (see "Manual test follow-up").

## Round 5 (2026-11-04)

- Fix round 7 (task 2.4b): a117f3ee. Checks: fmt/clippy ok, cargo df-core 129 and df-web 567 passed, Bazel ok, docs-build ok, openspec validate ok, e2e 9/9; layout not checked in a browser
- Reviewed range: f49384bd...a117f3ee
- Security (find): 0 findings

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| STD-34 | Standards | Blocking | `crates/df-web/src/config_pages/providers.rs:400,450,456,465` | Scheme switch loses entered values: htmx skips disabled controls in `hx-include`, and the subscription branch renders headers empty; switching away and back (or a stray change on edit) wipes custom header, prefix and extra headers | fix-now | fixed (1b9a2f08) |
| STD-35 | Standards | P1 | `providers.rs:382` | `EXAMPLE_TOKEN` const between functions | fix-now | fixed (1b9a2f08) |
| STD-36 | Standards | P1 | `providers.rs:401,451-464` | hand-rolled aria attributes: describedby never points at the error, headers lacks aria-invalid; reuse forms.rs helpers | fix-now | fixed (1b9a2f08) |
| STD-37 | Standards | P1 | `providers.rs:414-428,474-480`, `df-gateway/src/inference/headers.rs:78` | scheme prefix/header spelling duplicated; new scheme silently shows `Bearer` | fix-now | fixed (1b9a2f08) |
| SPEC-23 | Spec | P1 | `providers.rs:428` | preview shows `Bearer` without the trailing space the scheme sends | fix-now | fixed (1b9a2f08) |
| SPEC-24 | Spec | P1 | `docs/content/using/model-providers.md:35` | Required column reads as if the prefix were required | fix-now | fixed (1b9a2f08) |
| SPEC-25 | Spec | P1 | proposal.md, specs/model-providers "Credential schemes" | "no extra headers" for claude-subscription only in the templates requirement | fix-now | fixed (1b9a2f08) |
| STD-38 | Standards | P3 | `components/forms.rs:180-188` | 7-parameter `select_field_with_hint` with two adjacent Options | | cosmetic |

User decision: fix Blocking + P1. STD-38 cosmetic.

- Fix round 8: 1b9a2f08. STD-34 fixed client-side (Alpine state on the form, scheme-fields route removed, verified with Playwright), STD-35 to STD-37, SPEC-23 to SPEC-25 fixed. Checks: fmt/clippy/machete ok, df-core 130, df-gateway 331, df-web 567 passed, Bazel 9/9, docs-build ok, e2e 9/9; container tests ran

## Round 6 (2026-11-04)

- Reviewed range: a117f3ee...1b9a2f08
- Security (find): 0 findings (x-data built with serde_json, server rules decided from the posted scheme)
- Round 5 rows re-checked: STD-34, STD-35, STD-36, STD-37, SPEC-23, SPEC-24, SPEC-25 fixed (1b9a2f08)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| STD-39 | Standards | P1 | `crates/df-web/src/config_pages/providers.rs:463-510` | `field()` wrapper hand-written three times in `scheme_fields` | fix-now | fixed (42eedf62) |
| STD-41 | Standards | P1 | `providers/tests.rs:577-626` | test promises the prefix is kept but asserts no prefix binding | fix-now | fixed (42eedf62) |
| STD-40 | Standards | P1 | `providers.rs:461-483` | `scheme === 'header'` written out four times instead of built from `CredentialScheme` | fix-now | fixed (42eedf62) |

User decision: fix all three, then finish.

- Fix round 9: 42eedf62 (STD-39, STD-40, STD-41; no behavior change; df-web 567 passed, Bazel ok). Diff read by the orchestrator, no reviewer round.
- Finish checks (2026-11-04): `bazel build --lockfile_mode=error //crates/... //plugins/...` ok, `just docs-build` ok, `openspec validate` valid.
- Task 2.6: the user walked the model provider guide through `just serve-local` and confirmed the new form (2026-11-04). Ticked.
- PenPot: the user split Config into Config - Projects, Config - Personas, Config - Credentials and Config - Model providers; `docs/design-workflow.md` (page table and mapping) and the `components/table.rs` board reference updated.
- `/opsx:update` (2026-11-04): deferred spec batch folded in: inference-endpoint (no-route 403, `Accept-Encoding: identity`, `Content-Length` re-framing, stalled requests and responses, plain-HTTP model upstreams, refused records dropped), fake-acp-agent (one chunk, sandbox environment dump), proposal (BREAKING setting rename, git-proxy modified), new git-proxy delta (`network.allowPlainHttp`). `openspec validate` clean.
- Final status: done. No blocking finding open; container-backed tests ran in every round; tasks all done.
