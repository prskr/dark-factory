# Review log: web-assets-typescript

- BASE: 3f7b0e3d17e865a0531eed1da5d0def3c01e1860
- Store: none
- Status: done
- Implementer agent: a91392124d6deb9f1

## Round 1 (2026-11-06)

- Implementer: eca8a9ed, tasks 16/17 (5.2 open), checks fmt/clippy/machete/xtask/tsc/docs-build ok; cargo df-web no tools 32 ignored, swc only 29 ignored; bazel df-web 670 pass 0 ignored. Container tests: df-web ran; full `bazel test //...` 9 container-backed targets failed after the Podman socket died (df-agent-channel x3, df-db, df-gateway, df-web, claude-harness/opencode-harness image tests). 5.2 only checked on a static page (compose ports taken by another worktree).
- Deviation: Bazel passes `WEB_SCRIPTS` (file list), not `WEB_SCRIPTS_DIR` (design D1/D3, task 3.1).
- Reviewed range: BASE...eca8a9ed
- User decision (socket/deviation): socket restarted; implementer reruns `just bazel-test` and 5.2 in parallel with review. `WEB_SCRIPTS` deviation accepted; implementer updates design.md D1/D3 and task 3.1.

Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-web/src/task_pages/tests/detail.rs:294`, `layout.rs:257` | `workspace.ts` source assertions share a test with `app_css()`, gated on styles, so they don't run in every build | split: source part ungated | fixed (38f7e886) |
| P3 | Spec | Blocking | `crates/df-web/src/assets.rs:134` | "Vendored script unchanged" byte-for-byte untested; existence check is styles-gated | ungated byte comparison test | fixed (38f7e886) |
| P4 | Spec | Blocking | `crates/df-web/build.rs:458` | "Compiler missing" warning, `DF_WEB_REQUIRE_ASSETS`, stale-output wipe have no automated test | testable decision fn or accept manual | fixed (38f7e886) |
| P2+S5 | Spec/Standards | P1 | `crates/df-web/build.rs:56` | `PermissionDenied` → "not found" also for explicit `SWC_BIN`/`TAILWIND_BIN`; D3 says found-but-failing is an error | only for PATH lookup | fixed (38f7e886) |
| P6 | Spec | P1 | `design.md` D1/D3, `tasks.md` 3.1 | still `WEB_SCRIPTS_DIR` | fix-now | fixed (ecd60885) |
| P7+S2 | Spec/Standards | P1 | `docs/development.md:254` | third hard-coded `typescript@7.0.2` not covered by Renovate | `typescript@<version>`, see lefthook.yml | fixed (38f7e886) |
| S1 | Standards | P1 | `tools/xtask/src/main.rs:630` | test `swc_asset_names` named after topic | rename to behavior | fixed (38f7e886) |
| S3 | Standards | P1 | `crates/df-web/scripts/egress-rule.ts:25` | lost line break | restore | fixed (38f7e886) |
| S6 | Standards | P1 | `crates/df-web/build.rs:100` | `"no file name"` error vague | say which entry | fixed (38f7e886) |
| S4 | Standards | P3 | `crates/df-web/build.rs:165` | 4-tuple drives per-asset decision | fix-now (user moved P3 → fix) | fixed (38f7e886) |
| P5 | Spec | Rejected | Bazel `df_web_test` | cfgs reaching Bazel test unconfirmed | rejected: implementer's `bazel test //crates/df-web/...` run: 670 pass, 0 ignored | rejected |

User decision: fix Blocking + P1 + S4 (P1, P3, P4, P2+S5, P7+S2, S1, S3, S6, S4; P6 already fixed).

Follow-up (implementer, ecd60885): `just bazel-test` //... 50/50 pass after recreating the shared `df-test-postgres-17-alpine-r1` container. Task 5.2 still open: `podman compose up` fails with netavark/aardvark-dns "failed to bind udp listener on 10.89.1.1:53"; Playwright MCP disconnected. Only a static-page check of the compiled components exists.

## Round 2 (2026-11-06)

- Implementer: 38f7e886, tasks 16/17 (5.2 open: Podman network/aardvark-dns failure, no browser tool), checks tsc/fmt/clippy/machete/xtask ok; cargo df-web no tools 32 ignored, swc only 29 ignored, both 0 ignored; bazel df-web 680 pass 0 ignored. Full `//...` not rerun this round.
- Reviewed range: ecd60885...38f7e886

Security: 0 findings. Spec: all round-1 rows confirmed fixed. Standards: S1–S6 confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P8+S8 | Spec/Standards | Blocking | `crates/df-web/build.rs:71`, `build_assets.rs:29,131` | `WEB_SCRIPTS` resolved, joined to a `String` via lossy `display()`, re-split on whitespace: breaks on paths with spaces/non-UTF-8; regression of round-1 refactor (verified) | `Option<Vec<PathBuf>>`, split only in build.rs | fixed (0a664a2c) |
| S7 | Standards | P1 | `crates/df-web/build_assets.rs:45` | private `Kind` and consts sit between public items (file-structure rule) | move private helpers below `stage_assets` | fixed (0a664a2c) |
| S9 | Standards | P3 | `crates/df-web/build_assets.rs:260` | tests rely on `true`/`false` from PATH, unstated | optional: executable sh stubs in tempdir writing output | cosmetic |

User decision: fix Blocking + P1 (P8+S8, S7). S9 cosmetic.

## Round 3 (2026-11-06)

- Implementer: 0a664a2c, checks fmt/clippy/machete/check-test-names ok; cargo df-web no tools 32 ignored, both 0 ignored; full `bazel test //...` 50/50 (after ENOSPC + cargo clean). One flaky both-tools cargo run (93 DB tests failed, passed on rerun; suspected Podman). 5.2 still open (podman compose network failure, no browser tool).
- Reviewed range: 38f7e886...0a664a2c

Security: 0 findings. P8+S8, S7 confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P9+S10 | Spec/Standards | P1 | `crates/df-web/build.rs:72` | `WEB_SCRIPTS` read via `var_os` + `to_string_lossy`: non-UTF-8 silently mangled, misleading copy error; inconsistent with `TAILWIND_APP_CSS` (`env::var`) | `env::var`, clear error on non-UTF-8 | fixed (f0288fe8) |
| P10+S11 | Spec/Standards | P1 | `crates/df-web/build.rs:72-78`, `build_assets.rs:311` | split-then-resolve order in `main` untested; test name promises Bazel paths with spaces work | extract `parse_web_scripts` into build_assets.rs, test with a manifest dir containing a space; rename test to what it checks | fixed (f0288fe8) |

User decision: fix P1 (P9+S10, P10+S11), then finish.

## Round 4 (2026-11-06)

- Implementer: f0288fe8 (all `build.rs` env reads via `optional_env`; `parse_web_scripts`/`resolve_path` moved to build_assets.rs with tests). Checks rerun after user deleted `target/`: fmt/clippy/machete/check-test-names ok; cargo df-web no tools 32 ignored, both 0 ignored; bazel df-web 682 pass 0 ignored. DB test timeouts were environmental (shared postgres container exited; recreated). 5.2 still open (`podman compose up` exit 125; container `admiring_thompson` stuck "Removing"; no browser tool).
- Reviewed range: 0a664a2c...f0288fe8

Security: 0 findings. P9+S10, P10+S11 confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P11 | Spec | P1 | `crates/df-web/build.rs:63` | `PWD` now read strictly: non-UTF-8 checkout path fails every df-web build (regression of round-3 fix; PWD is only a lookup hint) | `env::var_os("PWD").map(PathBuf::from)` | fixed (902f8d9c) |
| P12 | Spec | P3 | `crates/df-web/build.rs:15` | `optional_env` (NotUnicode → error naming var, NotPresent → None) untested | optional: move to build_assets.rs and test | cosmetic |
| S12 | Standards | P3 | `crates/df-web/build_assets.rs:336` | resolution test also re-checks copy step | drop staging half | cosmetic |
| S13 | Standards | P3 | `build.rs:23`, `build_assets.rs:57,68` | `(manifest_dir, pwd)` travel together | `Roots` struct if more callers | cosmetic |

User decision: fix P11, then finish. P12, S12, S13 cosmetic.

## Finish (2026-11-06)

- P11 fix 902f8d9c verified by orchestrator (one line, `PWD` via `env::var_os`). fmt/clippy ok, cargo df-web both tools 682/0/0, full `bazel test //...` 50/50 with the socket, `just docs-build` ok.
- No Cargo.toml/Cargo.lock change: lockfile Bazel build not needed.
- Published docs: no change (nothing in `docs/content/` or mounted crate docs covers the asset build). Contributor docs changed: `AGENTS.md`, `crates/df-web/AGENTS.md`, `crates/df-web/README.md`, `.agents/skills/rust/SKILL.md`, `docs/development.md`.
- Tech debt: no P2 entries. TD-75 resolved by this change (removed).
- Roadmap: backlog rows added for chat autocomplete, JS unit test runner, native TypeScript compiler.
- Task 5.2: done (e7937bce): `just serve-local` + Playwright: compiled minified scripts served; tabs (click, arrows), egress log (live rows, verdict filter, pause), egress-rule deny/allow, workspace collapse + mobile sheet, Mermaid in sandboxed iframe; 0 console errors from own scripts (one AbortError from vendored hx-sse on filter swap). Text search not narrowing in the check: server-side htmx `input changed` trigger (`egress/log/page.rs:114`), not touched by this change; synthetic event without value change. Tasks 17/17.
