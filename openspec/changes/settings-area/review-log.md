# Review log: settings-area

- BASE: 2a3ba54c3f7241e2acacdd069cf09406fc122ff5
- Store: none
- Status: done
- Implementer agent: abc983754c5c9d738
- DOCKER_HOST: unix:///run/user/501/podman/podman.sock
- Design: ec2e5bc0 (task 3.2, approved by user)
- Step: finished

## Round 1 (2026-11-04)

- Implementer: 5bab3bb7, tasks 9/9, checks fmt/clippy/machete ok, df-web 581 pass, bazel //... 46/46 (with .bazelrc.agents; plain `just bazel-test` failed once in //crates/dark-factory "can't find crate df_plugins", not investigated), e2e 10/10, docs-build ok, openspec validate ok; container tests ran
- Reviewed range: BASE...5bab3bb7

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `docs/content/authentication/keycloak.md:27` | "Settings → Credentials" edit changed a Keycloak client tab reference, not the Dark Factory page (verified) | revert to the client's **Credentials** tab | fixed (0b2fcb6a) |
| P2 | Spec | Blocking | `crates/df-web/src/layout.rs` | Scenario "No sidebar on phones" (390 px, no horizontal scroll) has no automated test; only a manual Playwright check | e2e check at 390 px | fixed (0b2fcb6a) |
| S1 | Standards | P1 | `crates/df-web/src/components/shell.rs:97` | Settings href `/personas` duplicates `SETTINGS_SECTIONS[0]`; reorder breaks it silently | derive from first entry | fixed (0b2fcb6a) |
| S2 | Standards | P3 | `crates/df-web/src/components/shell.rs:123` | `rail_link` takes two positional bools | — | cosmetic |
| S3 | Standards | P3 | `crates/df-web/src/layout.rs:11` | `is_settings` imported via `shell::` path, sibling via re-export | — | cosmetic |
| S4 | Standards | P3 | `crates/df-web/src/components/shell.rs:12,26` | `(Nav, &str, &str, Icon)` tuple in two arrays | — | cosmetic |
| P3 | Spec | P3 | `crates/df-web/src/layout.rs` | "Switch section" covered by render tests only, no navigation test | — | cosmetic |

Security: 0 findings.

User decision: Fix blocking + P1 (P1, P2, S1).

Side fix (user decision, separate `fix(df-test-postgres)` commit, not part of the settings-area squash): the fix round's `cargo test -p df-web` hung for about 48 min without DOCKER_HOST. Cause: `SERVER` OnceCell never caches a failed init, so each test retries the 60 s container start in turn. Fix: cache the failure, fail fast with a DOCKER_HOST hint when the Docker endpoint is unreachable, keep the retry only for the name-conflict race, regression test.

## Round 2 (2026-11-04)

- Implementer: 0b2fcb6a (fix round 1), a2c659a4 (`fix(df-test-postgres)`, separate), checks: df-web 581 pass, clippy ok, e2e 12/12 (incl. phone test), docs-build ok, df-test-postgres 11 pass, df-web without DOCKER_HOST fails in ~20 s; container tests ran
- Reviewed range: 5bab3bb7...a2c659a4

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S5 | Standards | P1 | `crates/df-test-postgres/src/lib.rs:98-139` | private helpers placed between public fns (file structure) | move below public items | fixed (0f211242) |
| S6 | Standards | P1 | `crates/df-test-postgres/src/lib.rs:108-109` | `use` lines inside `fn is_unreachable` | move to top-level imports | fixed (0f211242) |
| S7 | Standards | P3 | `crates/df-test-postgres/src/lib.rs:203-215` | error classified twice (retry predicate and message) | — | cosmetic |
| S8 | Standards | P3 | `crates/df-test-postgres/src/lib.rs:73` | `Result<_, String>` in OnceCell; acceptable for a test utility | — | cosmetic |
| S9 | Standards | P3 | `crates/df-e2e/tests/demo.rs:58-64` | overflow check compares JS bool as string | — | cosmetic |
| P4 | Spec | P3 | `crates/df-test-postgres/src/lib.rs:112` | `ClientError::Init` branch untested (bollard error not constructible); verified by real run | — | cosmetic |

Security: 0 findings. Round 1 P1, P2, S1 fixed.

User decision: Fix blocking + P1 (S5, S6) as fixup of a2c659a4.

## Round 3 (2026-11-04)

- Implementer: 0f211242 (fixup of a2c659a4), df-test-postgres 11 pass, clippy ok, fmt ok; container tests ran
- Reviewed range: a2c659a4...0f211242, verified by the orchestrator (code move and imports only, no behavior change); S5, S6 fixed. No reviewer agents spawned for a move-only diff.

## Finish

- Container tests: all ran (df-web, df-test-postgres, e2e 12/12); none skipped.
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ok (no Cargo.toml/Cargo.lock change). The `can't find crate df_plugins` failure from the implementer's plain `just bazel-test` in round 1 did not reproduce.
- Docs: `just docs-build` ok. Pages changed: docs/content/using/{_index,setup,network-access,model-providers}.md, docs/content/integrations/forges/_index.md, docs/content/operations/upgrades.md, docs/content/authentication/keycloak.md (wording only), crates/df-plugins/docs/configuration.md.
- Tech debt: none (no P2 findings). Deferred work: none new (out-of-scope items are rejected in the proposal; instance/user settings are already roadmap iteration 5).
- Open: none. Cosmetic: S2, S3, S4, S7, S8, S9, spec P3, P4.
