# Review log: persona-commit-signing

- BASE: 6bcbfe82bc0e00b60e4b3bcc51417a783187826a
- Store: none
- Status: done

## Round 1 (2026-11-25)

- Implementer: c74f1992 (earlier ef703590, 5eb30d8a), tasks 18/19 (6.1 open, BLOCKED: Forgejo shows API-added SSH keys as unverified, so the "verified" badge cannot be asserted), checks clippy/machete/deny/fmt/docs-build/helm/bazel test+build green, container tests ran (unix:///var/run/docker.sock)
- Step: 2. User decision (6.1): update the artifacts first. Run /openspec-update-change to reword the Forgejo parts (tasks 6.1/6.2, proposal, commit-signing spec) to "not verifiable with Forgejo's own verification", keeping GitHub as the verified-badge target. Revised in /openspec-update-change instead: full Forgejo support ("Verify key on Forgejo", gated on the `forgejo` plugin, design D7, tasks 1.2, 3.4, 6.1, 6.2), shim path corrected. Task 1.2 needs /prepare-change before resuming. Then resume this skill at step 2: the implementer closes 6.1 against the updated artifacts, then round 1 review over BASE...HEAD.
- Iteration 2: 8f1e3d55, tasks 19/21 (3.4 done; 6.1, 6.2 open), BLOCKED: Forgejo 15 verifies SSH keys only with namespace = its DOMAIN setting (e2e: `forgejo-http.forgejo.svc`), not `gitea`; token signed without newline, valid 1 min; no API, verify is a form POST to `/user/settings/keys` (`type=verify_ssh`). Checks: fmt, clippy, machete, deny, docs-build ok; df-core, df-db, new df-web tests pass; other df-web tests fail on stale `node_modules` (htmx 4.0.1 vs 4.0.2, environment); e2e not run. Tests were not written red-first.
- User decision (namespace): a "Forgejo domain" field in the verify dialog, prefilled with the host of the `forgejo` plugin's `base-url`, editable; the domain is audited with the signature. Needs /openspec-update-change (spec, design D7, tasks) and /prepare-change (boards), then resume at step 2 (finish 3.4 change, 6.1, 6.2).
- Iteration 3 (HEAD_1): 6cd80200, tasks 21/21 (3.4 and 6.1 ticked by the orchestrator; the implementer reported both done). Checks: fmt, clippy all-features, machete, deny, docs-build ok; df-core 332, df-db 489, df-web 1042 pass; 1 df-web fail `assets::tests::installed_packages_have_the_versions_of_package_json` (stale `node_modules`, pnpm unavailable, environment); e2e full suite green incl. verified-on-Forgejo and `df:signature` rejection. Container tests ran (unix:///var/run/docker.sock). Deviation reported: "Forgejo domain" prefill uses the host of the first `forgejo` route's `apiUrl` or claimed host (no instance-level `base-url` exists); empty for a default plugin without `hosts`.
- Reviewed range: 6bcbfe82...6cd80200
- Security (find): 0 findings. Note (not a finding): the Helm Role's unconditional `create`/`update` on Secrets covers every Secret in the namespace.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S10 | Standards | Blocking | `crates/factory-agent/src/git_sign.rs:75` | shim's `read_frame` waits on the runtime with no timeout (CODING_STANDARDS "Concurrency" MUST); verified | fix-now | fixed (9ac81ecb) |
| S1 | Standards | Blocking | `crates/df-e2e/tests/commit_signing.rs:68` | `forge_verify` audit assert counts entries of earlier runs (e2e skill "Isolation", `E2E_REPEAT=2`) | fix-now | fixed (9ac81ecb) |
| P1 | Spec | Blocking | `specs/commit-signing/spec.md` "Verify dialog", D7, tasks 1.2/3.4, `design-boards.md:36`; how-to | prefill source in artifacts says `base-url`; code uses first `forgejo` route's `apiUrl` host, else claimed host, empty for a default plugin (verified `routing.rs:250`). Accept code; reword artifacts (orchestrator), how-to names the config keys (implementer) | fix-now | fixed (9ac81ecb) |
| P2 | Spec | Blocking | `crates/df-core/src/routing.rs:250` | `host_of` untested: `apiUrl` preference, `None` for a default plugin (verified) | fix-now | fixed (9ac81ecb) |
| P3 | Spec | Blocking | `crates/df-web/src/config_pages/tests/signing_key.rs:124` | scenario "No key yet": stored persona without key not tested | fix-now | fixed (9ac81ecb) |
| P4 | Spec | Blocking | `df-agent-channel` assign tests | scenario "Key not in the sandbox": no assertion that `Assign` carries no private key (verified: no such test) | fix-now | fixed (9ac81ecb) |
| S2 | Standards | P1 | `crates/df-gateway/src/git/checks.rs:78`, `gateway.proto:201` | GLOSSARY _Avoid_ "commit identity" in edited lines → "persona identity" | fix-now | fixed (9ac81ecb) |
| S3 | Standards | P1 | `crates/df-db/src/push_checks.rs:676` | const after fn | fix-now | fixed (9ac81ecb) |
| S4 | Standards | P1 | `crates/factory-agent/src/git.rs:114` | const after struct | fix-now | fixed (9ac81ecb) |
| S5 | Standards | P1 | `df-core/src/signing_key.rs:30,190`; `df-web/src/config_pages/signing_key.rs:256-432` | public before private order | fix-now | fixed (9ac81ecb) |
| S7 | Standards | P1 | `df-web/src/config_pages/personas.rs:469,495,562`; `signing_key.rs:413,452` | `SigningKeySection::load` match block repeated 5× | fix-now | fixed (9ac81ecb) |
| S8 | Standards | P1 | `crates/df-db/src/persona_keys.rs:140` | `sign_forgejo_token` is a use case in df-db; move composition to the df-web handler | fix-now | fixed (9ac81ecb) |
| S9 | Standards | P1 | `crates/df-agent-channel/src/signing.rs:97` | `Result<String, String>` instead of a typed `thiserror` refusal enum | fix-now | fixed (9ac81ecb) |
| S11 | Standards | P1 | `crates/factory-agent/src/sign_relay.rs:51` | doc names `SIGN_TIMEOUT` but takes `timeout` param | fix-now | fixed (9ac81ecb) |
| P5 | Spec | P1 | `crates/df-web/README.md:181` | route table lacks `POST /personas/{id}/verify-key` | fix-now | fixed (9ac81ecb) |
| P6 | Spec | P1 | `crates/df-gateway/README.md:267` | empty key: unsigned commit is "missing signature", not "invalid signature" | fix-now | fixed (9ac81ecb) |
| P7 | Spec | P1 | `crates/df-agent-channel/src/signing.rs:241`, D3 | header parser instead of gix-object; amend D3 (orchestrator) | fix-now | fixed (a33db4e1) |
| P8 | Spec | P1 | `tasks.md` 4.3 | stale `/opt/factory/bin/df-git-sign` (orchestrator) | fix-now | fixed (a33db4e1) |
| P9 | Spec | P1 | `docs/content/using/verify-persona-commits.md:121` | refusals listed under "signing is unavailable" | fix-now | fixed (9ac81ecb) |
| S6 | Standards | P2 | `df-db/src/catalog.rs:123`; `df-web/.../signing_key.rs:93` | key + fingerprint as two `Option<String>`; `PublicSigningKey` exists. Matters when more code reads the pair | fix-now | fixed (9ac81ecb) |
| S12 | Standards | P3 | `df-gateway/src/git/checks/tests.rs:18`, `policy_checks/tests.rs:20` | duplicated signed-commit test helper | fix-now | fixed (9ac81ecb) |
| S13 | Standards | P3 | `crates/df-core/src/signing_key.rs:36` | literal 253 repeats `MAX_DOMAIN_CHARS` | fix-now | fixed (9ac81ecb) |
| P10 | Spec | P3 | commit 26a4403a, `routing.rs` kingfisher comments | not spec scope; keep 26a4403a out of the squash | cosmetic | cosmetic |

User decision: fix blocking + P1 + S6 + S12 + S13 (S6 moved from P2, S12 and S13 from P3 to fix-now). P10 stays cosmetic. P1 (artifact wording), P7 and P8 are edited by the orchestrator; the rest is fix round 1 for the implementer.

## Round 2 (2026-11-25)

- Implementer: 9ac81ecb (HEAD_2), all 20 fix-now items reported fixed. Checks: fmt, clippy all-features, machete, deny, file length, docs-build ok; tests of df-core, df-db, df-web (1043/1044, same stale `node_modules` assets test), df-agent-channel, df-orchestrator, df-gateway, df-gateway-control, factory-agent pass; e2e `commit_signing` tests pass with `E2E_REPEAT=2` (full suite not rerun). Container tests ran. Implementer notes corrected by the spec review: the `persona.update` audit takes before/after from `PersonaInput`, so the S6 nesting reaches no audit JSON; a persona without a key gets 404 on `verify-key`, not 422 (P11).
- Reviewed range: 6cd80200...9ac81ecb
- Security (find): 0 findings. Ledger check: all round 1 fix-now rows fixed (S7 partly: 3-line match per handler remains, accepted).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P11 | Spec | Blocking | `crates/df-web/src/config_pages/signing_key.rs:229-235` | `verify_forgejo` signs and audits `forge_verify` before checking that the persona has a key in the DB; with Secret but NULL columns the token is signed and audited, then 404 hides the signature; no test for a persona without key; README row omits the case (verified) | fix-now | fixed (4c138e1b) |
| P12 | Spec | P1 | `docs/content/using/verify-persona-commits.md:65`; spec "Verify dialog", D7 | `host_of` takes the alphabetically first claimed host (`RoutingTable.hosts` is a `BTreeMap`, verified), not the first configured entry | fix-now | fixed (1b3c6351, 4c138e1b) |
| P13 | Spec | P1 | `crates/factory-agent/docs/configuration.md:39` | "Fixed settings" lacks the shim's 12 s wait (`RUNTIME_TIMEOUT`) | fix-now | fixed (4c138e1b) |
| S14 | Standards | P1 | `crates/df-gateway/README.md:267` | "commit identity" (GLOSSARY _Avoid_) in the line rewritten for P6 | fix-now | fixed (4c138e1b) |
| S15 | Standards | P1 | `crates/df-gateway/src/git/policy_checks/tests.rs:13-20` | `use` and `fn signed` between constants (File structure) | fix-now | fixed (4c138e1b) |
| S16 | Standards | P1 | `crates/df-agent-channel/src/signing.rs:68-88` | `Refusal` variants hold `String` of the source error; keep `#[source]` `DbError`/`KeyError` | fix-now | fixed (4c138e1b) |
| S17 | Standards | P3 | `crates/df-web/src/config_pages/signing_key.rs:877-888` | `load` vs `read` near-synonyms | fix-now | fixed (4c138e1b) |
| S18 | Standards | P3 | `crates/factory-agent/src/tests/git_sign.rs` | 200 ms real wait instead of `tokio::time::pause()` (reviewer: acceptable) | fix-now | fixed (4c138e1b) |

User decision: fix blocking + P1 + S17 + S18 (all 8). P12 spec and D7 wording edited by the orchestrator; the rest is fix round 2 for the implementer.

## Round 3 (2026-11-25)

- Implementer: 4c138e1b (HEAD_3), all 8 items reported fixed (P11 test failed first). Checks: fmt, clippy all-features, machete, deny, file length, docs-build ok; df-web (1044/1045, same stale `node_modules` assets test), df-agent-channel, df-gateway, factory-agent pass; full `just e2e-local` green. Container tests ran; the implementer removed the test PostgreSQL container and ran `docker system prune` to free disk space.
- Reviewed range: 9ac81ecb...4c138e1b
- Security (find): 0 findings. Ledger check: P11–P13, S14–S18 all fixed in 4c138e1b.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P14, S21 | Spec, Standards | Blocking (regression in changed code) | `crates/df-agent-channel/src/signing.rs:61-63,200` | `Refusal::AuditDetails` displays `{0}`, dropping the "the signature could not be audited:" prefix that the refusal reason and `commit.sign_refused` carried before (path practically unreachable) | fix-now | fixed (cfff4058) |
| P15 | Spec | P1 | `crates/df-core/src/routing.rs:773-815` | no `host_of` test with two claimed hosts for the "alphabetically first" promise | fix-now | fixed (cfff4058) |
| P16, S19 | Spec, Standards | P1 | `crates/df-web/README.md:42` | `verify-key` row states the 422-on-unreadable-Secret case twice | fix-now | fixed (cfff4058) |
| S20 | Standards | P1 | `crates/factory-agent/src/git_sign.rs:25-34` | private `run_with_timeout` has one caller passing a constant; inline into `run` | fix-now | fixed (cfff4058) |

User decision: fix blocking + P1 (all 4) in fix round 3, the last allowed.

## Round 4 (2026-11-25)

- Implementer: cfff4058 (HEAD_4), all 4 items reported fixed. Checks: fmt, clippy all-features, machete, deny, file length, docs-build ok; df-core, df-agent-channel, factory-agent pass; df-web 1044/1045 (same stale `node_modules` assets test); no e2e rerun (no e2e change). No `docker system prune`.
- Reviewed range: 4c138e1b...cfff4058 (4 files, +19 −12)
- Security (find): 0 findings. Ledger check: P14/S21, P15, P16/S19, S20 fixed in cfff4058.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P17 | Spec | P2 | `crates/df-web/src/config_pages/tests/signing_key.rs` | `verify-key` 404 for an unknown persona and 422 for an unreadable Secret (README row) have no test; code correct. Matters when the `verify_forgejo` handler changes | tech-debt | tech-debt (TD-131) |
| S22 | Standards | P3 | `crates/df-agent-channel/src/signing.rs:60-64` | `AuditDetails` and `Audit` now identical in message and payload | cosmetic | cosmetic |

User decision: finish. P17 → TD-131, S22 cosmetic.

## Finish (2026-11-25)

- Container tests: all ran (unix:///var/run/docker.sock); none skipped. Only failing test: df-web `assets::tests::installed_packages_have_the_versions_of_package_json` (stale `node_modules`, htmx 4.0.1 vs 4.0.2, pnpm unavailable; environment, not this change).
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ok.
- Docs: `just docs-build` ok. Pages added or changed: `docs/content/using/verify-persona-commits.md` (new), `docs/content/using/_index.md`, `docs/content/using/setup.md`, `docs/content/security/git-proxy.md`, `docs/content/security/_index.md`, `docs/content/operations/backup.md`, `docs/content/operations/upgrades.md`, `crates/df-agent-channel/docs/configuration.md`, `crates/factory-agent/docs/configuration.md`.
- Tech debt: TD-131. Roadmap: backlog rows for forge-plugin key registration and external signers already exist.
- Squash note: commit 26a4403a (PenPot skill helpers) is not part of the change; keep it separate (P10).
