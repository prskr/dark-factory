# Review log: faster-test-loop

- BASE: 6c81da48a5771978f6a277097d226847d334379f
- Store: none
- Status: done
- Implementer agent: ab302355e36006eab

## Round 1 (2026-11-16)

- Implementer: 85072ad8, tasks 21/21, checks fmt/clippy/machete/xtask checks/openspec validate/docs-build/bazel build ok; `just test` 2978 passed, 1 failed (df-web `installed_packages_have_the_versions_of_package_json`: stale sandbox node_modules, pnpm missing); `cargo xtask update-tools --check` not run (sandbox TLS), container tests ran (nextest workspace, unlanded_lookup, e2e REPEAT=2 on kind)
- Reviewed range: BASE...85072ad8

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S2+P1 | Standards+Spec | P1 | `docs/demo.md:157`, design.md Risks, `crates/df-e2e/src/browser.rs` `report` | Docs say re-login logged at `info` with test name; code prints `login: …` to stderr without name | fix-now | fixed (3bc307ec) |
| P2 | Spec | P1 | `crates/df-e2e/src/browser.rs:132` | `opens_the_ui` goto uses default timeout, not `UI_TIMEOUT` (D3) | fix-now | fixed (3bc307ec) |
| S1 | Standards | P1 | `crates/df-e2e/src/browser.rs:98` | Mutex held across login; deliberate (D1), all steps time-bounded; exception not named | fix-now (comment + skill e2e line) | fixed (3bc307ec) |
| S6 | Standards | P1 | `AGENTS.md:75` | Test command still `cargo test`, skill `rust` says nextest | fix-now | fixed (3bc307ec) |
| P5 | Spec | P1 | `docs/development.md:209` | "20 to 30 %" vs measured 18–31 % | fix-now | fixed (3bc307ec) |
| P3 | Spec | P1 | tasks.md 3.2 | #123 tracks 7 other TDs; PR must say "Refs #123 (TD-81)", not close it | fix-now (tasks wording; PR text) | fixed (3bc307ec) |
| S3 | Standards | P3→fix | `browser.rs` / `fixture.rs` `report` | Two `eprintln!` report helpers | fix-now (user) | fixed (3bc307ec) |
| S4 | Standards | P3→fix | `browser.rs:94-108` | Stored-state try branch duplicated | fix-now (user) | fixed (3bc307ec) |
| S5 | Standards | P3 | `forgejo.rs` `ReviewEvent` | Only `RequestChanges` used | — | cosmetic |
| P6 | Spec | P3 | `tests/demo.rs` | Login test failure saves fixture page diagnostics | — | cosmetic |
| P4 | Spec | Rejected | tasks/design | Check results not recorded | — | rejected (in implementer report and this ledger) |
| — | Security | — | — | 0 findings (nextest hashes not verified offline) | — | — |

User decision: Fix blocking + P1, plus S3 and S4.

## Round 2 (2026-11-16)

- Implementer: 3bc307ec, all 8 round-1 items fixed; `cargo test -p df-e2e --lib` 22 passed, clippy/fmt/check-test-names/check-file-length/openspec validate ok; e2e `demo.rs` on a fresh cluster passed (2 Dex submissions); full suite and re-login check not repeated
- Reviewed range: 85072ad8...3bc307ec

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S7+P7 | Standards+Spec | Blocking (minor, code changed by last fix round) | `crates/df-e2e/src/browser.rs:98-101`, `.agents/skills/e2e/SKILL.md:50`, `Session::login` ~167 | Comment/skill claim every step under the lock is bounded by `within`/`UI_TIMEOUT`/`LOGIN_TIMEOUT`; `goto`/`fill`/`click` in `login` only have Playwright defaults | fix-now | fixed (7260e5ff) |
| S8 | Standards | P1 | `crates/df-e2e/src/browser.rs:203` | `is_logged_in` navigates and stores a cookie; predicate name hides side effects | fix-now (rename `try_stored_login`) | fixed (7260e5ff) |
| S9 | Standards | P3 | `crates/df-e2e/src/browser.rs:706` | Shared `report` helper lives in `browser.rs` | — | cosmetic |
| — | Security | — | — | 0 findings | — | — |

User decision: Fix blocking + P1.

## Round 3 (2026-11-16)

- Implementer: 7260e5ff, S7+P7 and S8 fixed; `cargo test -p df-e2e --lib` 22 passed, clippy/fmt/check-test-names/check-file-length/openspec validate ok; e2e `demo.rs` on a fresh cluster passed (2 Dex submissions); no cluster left
- Reviewed range: 3bc307ec...7260e5ff

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S10 | Standards | Blocking (minor, code changed by last fix round) | `crates/df-e2e/src/browser.rs:99-104`, `Session::login` `fill`/`click`, `.agents/skills/e2e/SKILL.md:50` | `fill`/`click` under the lock rely on the server-enforced 30 s action timeout; a server that stops answering is not bounded client-side (cf. `CONNECT_TIMEOUT` doc) | fix-now | fixed (695591d3) |
| S11 | Standards | P1 | `.agents/skills/e2e/SKILL.md:50` vs `browser.rs:97-104` | Timeout list duplicated in skill and comment | fix-now | fixed (695591d3) |
| — | Spec | — | — | 0 findings; S7+P7, S8, D1, D3 confirmed | — | — |
| — | Security | — | — | 0 findings | — | — |

User decision: Fix blocking + P1 (fix round 3, last allowed).

## Round 4 (2026-11-16)

- Implementer: 695591d3, S10 and S11 fixed; `cargo test -p df-e2e --lib` 22 passed, clippy/fmt/check-test-names/check-file-length/openspec validate ok; e2e `demo.rs` on a fresh cluster passed; no cluster left
- Reviewed range: 7260e5ff...695591d3
- Standards 0, Spec 0, Security 0 findings.

## Finish

- Container tests: ran in iteration 1 (workspace nextest, `unlanded_lookup`, full e2e `E2E_REPEAT=2`, re-login check); fix rounds re-ran `demo.rs` e2e only (changes limited to `Session::login` timeouts, helper rename/merge).
- Bazel build `--lockfile_mode=error //crates/... //plugins/...`: ok in iteration 1; no `Cargo.toml`/`Cargo.lock` change after it.
- `just docs-build`: ok. Published docs changed: none (contributor docs and skills only).
- Not verified: `cargo xtask update-tools --check` (sandbox TLS; `cargo-nextest` lock entry written by hand, hashes unchecked); df-web `installed_packages_have_the_versions_of_package_json` failed on stale sandbox `node_modules` (no df-web change).
- Tech debt: none (no P2). Deferred work: roadmap backlog row for one test binary per crate added in iteration 1.
- PR description: "Refs #123 (TD-81)", not a closing keyword.
