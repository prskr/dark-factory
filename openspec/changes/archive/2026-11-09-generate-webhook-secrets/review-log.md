# Review log: generate-webhook-secrets

- BASE: 0d4a3e97a2994c047f3c60b4bc3dd844fb9fc16a
- Store: none
- Status: done
- Implementer agent: ad73483d5242b740f (replaces a4691ecf504c09bdf and adad61a9233e37493; both OOM-killed the VM with full-workspace cargo builds at 8 jobs. Now: jobs = 4 in ~/.cargo/config.toml, per-crate builds, target cleaned)
- DOCKER_HOST: unix:///var/run/docker.sock (2026-11-09)

## Round 1 (2026-11-09)

- Implementer: f00b34b, e34b996, e6bee1f (HEAD_1 = e6bee1f), tasks 21/21, checks: clippy per crate, machete, check-crates, helm-unittest 121/121, docs-build, openspec validate ok; cargo tests per crate ok (df-plugins integration tests only under Bazel); Bazel 31 targets, helm_chart_test fixed and rerun; container tests ran (docker.sock); `just e2e-local` not run (no cluster)
- Note: commits authored as "Dark Factory Agent <agent@example.invalid>" (no git identity configured); fix at squash
- Reviewed range: 0d4a3e9...e6bee1f

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/dark-factory/src/forge.rs:63-70` | Scenario "plugin without webhooks export needs no secret": the `capabilities().webhooks` filter in `resolve_webhook_secrets` is never exercised (test passes `false` into `resolve_secrets`) | fix-now | fixed (c5e6aea: comment at filter; no loadable component lacks a webhooks export) |
| P2 | Spec | Blocking | `crates/dark-factory/src/forge.rs:123-128` | Scenario "Create denied": no startup-level test that the error names `plugins.forgejo.webhookSecret` and the cause | fix-now | fixed (c5e6aea) |
| P3 | Spec | Blocking | `crates/dark-factory/src/forge.rs` | Rotation scenarios ("by deletion", "changed while running") have no test | fix-now | fixed (c5e6aea) |
| P4 | Spec | Blocking | `deploy/helm/dark-factory/tests/` | No Helm test for `webhookSecret: null` → generated (+ rbac `create`), none for "renders no Secret" / stable render | fix-now | fixed (c5e6aea) |
| S7 | Standards | P1 | `crates/dark-factory/src/forge.rs:638` | `plugin_without_webhook_secret_has_none_resolved` is vacuous: `load_forge_plugins` always leaves `webhook_secrets` empty | fix-now | fixed (c5e6aea) |
| P5 | Spec | P1 | `tasks.md:14,30` | Tasks 2.4 and 5.1 not ticked | fix-now | fixed (c5e6aea) |
| S1 | Standards | P1 | `crates/df-k8s/src/secrets.rs:74` | Second `impl K8sSecretStore` block not next to the type | fix-now | fixed (c5e6aea) |
| S2 | Standards | P1 | `crates/dark-factory/src/forge.rs:195-254` | New private items before `pub(crate)` functions | fix-now | fixed (c5e6aea) |
| S3 | Standards | P1 | `crates/dark-factory/src/forge.rs:195` | `WebhookSecretEntry` tuple needs a comment to explain positions; use a struct with named fields | fix-now | fixed (c5e6aea) |
| S4 | Standards | P1 | `crates/df-k8s/src/secrets.rs:83,96` | `timeout_error` helper has one caller; `fetch` builds the same message inline | fix-now | fixed (c5e6aea) |
| S8 | Standards | P1 | `deploy/helm/dark-factory/tests/plugins_test.yaml:226` | Test name claims the default case, which it never renders | fix-now | fixed (c5e6aea) |
| P6 | Spec | P1 | `docs/content/integrations/forges/webhooks.md:70` | `<fullname>` explanation omits `fullnameOverride` | fix-now | fixed (c5e6aea) |
| S9 | Standards | P1 (user) | `crates/df-core/src/seams.rs:483` | `c.secret.secret_name` stutter; `SecretKeyRef {name, key}` would read better | fix-now | fixed (c5e6aea) |
| S5 | Standards | P3 | `crates/df-gateway/src/control/{snapshot,tests}.rs` | Three test fakes repeat `get_or_create` → `get` | | cosmetic |
| S6 | Standards | P1 (user) | `crates/df-k8s/src/secrets.rs` tests | Repeated Secret response construction in 3 tests | fix-now | fixed (c5e6aea) |
| X0 | Security | Rejected | `values.schema.json` | Reviewer's residual: old `{name,key}` silently generates a new Secret | | rejected (schema `additionalProperties: false` rejects `name`; tested at `plugins_test.yaml:74`) |

User decision: fix blocking + P1, plus S9 and S6 (moved from P3 to fix-now). S5 stays cosmetic.

## Round 2 (2026-11-09)

- Implementer: c5e6aea (HEAD_2), tasks 21/21, checks: clippy per crate, machete, check-crates, helm-unittest 125/125, docs-build ok; cargo df-core, df-k8s; Bazel 31 targets pass (container tests ran via docker.sock)
- Reviewed range: e6bee1f...c5e6aea

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1-2, P2-2 | Standards, Spec | Blocking (changed code) | `crates/dark-factory/src/forge.rs:694` | `resolved_secret_is_not_reread_after_the_secret_changes`: first assert checks an already-returned owned value (cannot fail); scenario "Secret changed while running" not really tested | fix-now | fixed (34a7950) |
| S2-2 | Standards | Blocking (changed code) | `crates/dark-factory/src/forge.rs:675` | `denied_create_fails_startup_…` seeds a stored error the fake returns before any create; name overstates (403→Backend is tested in df-k8s) | fix-now | fixed (34a7950) |
| P1-2 | Spec | P1 | `crates/dark-factory/src/forge.rs:705` | "Rotation by deletion": no assertion that the old value is rejected (`401`) after regeneration | fix-now | fixed (34a7950) |
| S3-2 | Standards | P3 | `df-core/src/testing.rs:346`, `df-k8s/src/secrets.rs:101`, gateway tests | `SecretError::NotFound {secret_name, secret_key}` rebuilt field by field from `SecretKeyRef {name, key}` | fix-now | fixed (34a7950) |
| P3-2 | Spec | Rejected | `deploy/helm/dark-factory/templates` | "Rendering is stable" has no test | | rejected (holds by construction: no `rand*`/`lookup`/`genCA`; only hash is the deterministic ConfigMap checksum) |

User decision: fix blocking + P1, plus S3-2.

## Round 3 (2026-11-09)

- Implementer: 34a7950 (HEAD_3). S1-2 via the real webhook path (new test `running_forge_keeps_verifying_with_the_secret_read_at_startup`); checks: clippy per crate, cargo df-core/df-k8s/df-gateway, Bazel 24/24 (container tests ran), pre-commit hooks ok
- Reviewed range: c5e6aea...34a7950

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1-3 | Standards | P3 | `crates/dark-factory/src/forge.rs:784` | `forge_with_resolved_secret` repeats the setup of `signed_forgejo_delivery_is_verified_parsed_and_queued_for_its_task` | leave | cosmetic |
| S2-3 | Standards | P3 | `crates/dark-factory/src/forge.rs:837` | `webhook_key_ref()` restates `"wh-forgejo"`/`"secret"` already in `webhook_config` | leave | cosmetic |
| S3-3 | Standards | P3 | `crates/dark-factory/src/forge.rs:694` | `assert_ne!(old, new)` after removal checked in two tests | leave | cosmetic |

Spec: 0 new findings; all round 2 items fixed. Security: 0 findings.

User decision: finish; S1-3, S2-3, S3-3 cosmetic.

## Finish (2026-11-09)

- Container tests: ran in every round (docker.sock); none skipped. `just e2e-local` not run (no cluster); CI covers it.
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ok.
- Docs: `just docs-build` ok. Changed: `docs/content/integrations/forges/webhooks.md`, `crates/df-plugins/docs/configuration.md`, `deploy/helm/dark-factory/README.md`, `design/deployment.md`, `docs/adr/0056-control-plane-generates-missing-instance-secrets.md`, `docs/adr/README.md`.
- Tech debt: none (no P2 findings).
- Roadmap: no row; the admin page for webhook URL and secret is already covered by `ui-settings-and-task-sort` (user).
- Status: done. Rounds: 3 reviews, 2 fix rounds.
