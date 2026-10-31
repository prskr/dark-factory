# Review log: iter2-git-proxy

- BASE: fd5cf704727f80481521d58d51ef9ff9ca67c57b
- Store: none
- Status: done
- Implementer agent: fresh agent per chunk (stateless subagents; the environment ends subagents after about 10 minutes)
- DOCKER_HOST: unix:///var/run/docker.sock (OrbStack)
- Decisions before start: spike 1.2 against a local Forgejo container instead of the dev Forgejo; 6.1 (dev cluster) is done by Peter.

## Implementation (2026-10-31)

| Chunk | Tasks | Commit | Checks |
|---|---|---|---|
| ref rules | 2.1 | face185 | 13 unit tests, clippy (the implementer ran out of time before committing; the orchestrator verified and committed) |
| pkt-line parser, `report-status` | 2.2 | b73fbb3 | 22 unit tests, `--test report_status` 2/2 with the real `git push`; Bazel test target added but not run (terminal shared with spikes) |
| spikes (scratch projects under `target/spikes`, not committed) | 1.1, 1.2 | bd93f7f (docs) | gix quarantine + ancestry, pkt-line + report-status with real git, gix fetch over an own hyper/ring `Http` backend, local Forgejo 13.0.5 container behind a pass-through. Results in `crates/df-gateway/README.md` "Git proxy"; design D5 refspec narrowed to `refs/heads/*`, gitoxide risk closed |
| contract | 1.3 | 9d04a23 | protoc, `buf format`, df-proto 5 tests (redacting `Debug` of `ForgeCredential`), workspace `cargo check`; `GitRoute` is field 8 (7 is `run_number`). Implementer cut off before committing; orchestrator verified and committed |
| sandbox wiring | 5.2 | 13647cc | df-k8s 64 tests (snapshots updated), df-core, clippy; config `gateway.gitPort` (8443), `gateway.gitHost`. Implementer cut off before committing; orchestrator verified and committed |
| chart | 5.3 | f762c19 | helm lint, helm-unittest 75/75, README values check, `//deploy:helm_chart_test`. Renders `gateway.mirrorDir` and `gateway.maxPushSizeBytes`, which `GatewayConfig` does not read yet (task 3.3) |
| Git endpoint identification, snapshot Git routes | 3.1, 4.2 | e222e7f | df-gateway lib 122+ tests, df-db egress 5 (container), clippy, fmt, `.sqlx` regenerated, Bazel `--lockfile_mode=error` ok. Both implementers cut off before committing; their edits of `main.rs` interleave, so one commit |
| `git_pushes`, `RecordPush` server, stale scan | 4.1 | c61bdd8, 5d9512a, 63e3a70 | df-db `git_push` 5 tests (container), clippy. Orchestrator committed the `Uuid` re-export the implementer left uncommitted |
| HTTPS upstream forwarding for fetches, fetch-side error mapping | 3.2, 3.5 (fetch part) | 8b7f705, 63e3a70 | `git::upstream` 5 unit tests, `--test git_fetch` (real blobless clone through the endpoint against `git http-backend` over TLS, protocol v2, injected credential only). Orchestrator flattened a too-deep test closure and committed the Bazel lock |
| credential helper removed from the runtime | 5.1 (first part) | 50fe77d | factory-agent 112 tests |
| proxy URL in `Assign`, denied credential requests | 4.3 | 8e8e540 | df-agent-channel 121+26 tests |
| bare mirror with gix fetch over hyper | 3.3 (first part) | db5c17d, a4f7b2a | df-gateway integration tests, Bazel lock, deny |
| retry of transient Git failures | 5.1 (second part) | 08b0cc6, e243b0c | factory-agent 122 tests; scenarios run against a fake `git` script in front of real git, not an HTTP stub |
| spool, quarantine | 3.3 (second part) | 69c31dc, e8ea8da, 81935b5 | quarantine tests with thin packs from the git CLI; orchestrator fixed 4 clippy findings in the tests and committed the Bazel lock |
| receive-pack push path | 3.3 | 867de93, 74f5ea0 | `--test git_push` 5/5 with real git (fast-forward, force push, push to main, oversized, concurrent tasks) |
| push records over `RecordPush`, push-side upstream failures | 3.4, 3.5 | c912dfc, 5450bc2 | fake `GatewayGit` server tests, `git_push` 9/9 |
| Git endpoint reference, coverage | 3.6 | ee68655, 5abc9b6 | `cargo llvm-cov -p df-gateway`: 93.18 % lines (git/* 77–99 %), the two flaky tests skipped |

Open: 6.1 (dev cluster, Peter).

Pre-existing: `df-gateway` tests `control::relay::tests::the_streams_to_the_pods_stay_open_for_the_grace_period_and_serve_the_next_viewer` and `proxy::tests::an_upstream_that_never_answers_ends_with_502_and_upstream_timeout` fail intermittently, also on `main` (3 of 4 runs of `cargo test -p df-gateway --lib` at fd5cf70). Candidate for tech debt or a fix in the review round.

## Round 1 (2026-10-31)

- Implementer: b112b93, tasks 17/18 (6.1 open, dev cluster); checks green except the two pre-existing flaky tests
- Reviewed range: fd5cf70...b112b93, split into three areas (A gateway Git endpoint, B control plane/db/contract/assignment, C runtime/k8s/chart/docs), one Standards and one Spec reviewer each

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S-A1 | Standards | Blocking | `df-gateway/src/git/protocol.rs:67`, `push.rs:148` | commands ending at the 64 KiB head boundary → `pack_offset` None, pack skips quarantine and checks | fix-now | fixed (fix round 1) |
| S-A2, S-A8 | Standards | Blocking | `git/spool.rs:105`, `server.rs:220`, `upstream.rs:137` | client body reads (spool, forwarded upload-pack/info-refs bodies) without timeout or cap | fix-now | fixed (fix round 1) |
| S-A3 | Standards | Blocking | `git/mirror.rs:77` | mirror lock dropped on cancellation while the blocking fetch continues; no overall fetch deadline | fix-now | fixed (fix round 1) |
| S-A4 | Standards | Blocking | `git/push.rs:29`, `quarantine.rs:88`, `mirror.rs:132` | pack indexing: 1 GiB allocation limit, unbounded concurrency, no interrupt | fix-now | fixed (fix round 1) |
| S-A5 | Standards | Blocking | `git/protocol.rs:63` | push options forwarded with the gateway credential | fix-now: reject pushes with push options | fixed (fix round 1) |
| P-A1 | Spec | Blocking | `git/push.rs:228` | forge unreachable during the mirror fetch → 502 and no record instead of 503 + `upstream-failed` | fix-now | fixed (fix round 1) |
| P-A4, P-A2, P-A3 | Spec | Blocking | `git/push.rs:150, 205` | push attempts without record (malformed, invalid pack, body errors, oversize without refs/bytes); 413 does not show the reason to git | fix-now: record every attempt; amend the spec for 413 | fixed (fix rounds 1 and 2) |
| P-A5 | Spec | Blocking | `git/records.rs:128` | 5 s record budget not enforced (attempt may run past the deadline) | fix-now | fixed (fix round 1) |
| S-C7 | Standards | Blocking | `factory-agent/src/git.rs:51` | first clone can get `403 df: unknown source` before the snapshot with the pod IP reaches the gateway (verified: `pod_ip` set at `connecting`, 100 ms debounce) | fix-now: retry that 403 | fixed (fix round 1) |
| P-C1, S-C3, S-C4 | Both | Blocking | `design/agent-run.md`, `design/tech-stack.md`, `docs/tech_debt.md` | docs still describe the credential helper; §8/§11 not updated | fix-now | fixed (fix round 1) |
| S-C1, S-C2, P-C3 | Both | P1 | `factory-agent/src/connection.rs:102`, `git.rs:399` | dead `CredentialRequest` tracking and runtime socket for git | fix-now | fixed (fix round 1) |
| S-C8 | Standards | P1 | `factory-agent/src/git.rs:267` | `http.sslCAInfo` applies to every remote | fix-now: scope to the proxy URL | fixed (fix round 1) |
| S-C6, P-C5 | Both | P1 | `factory-agent/src/git.rs:41` | classifier reads `remote:` lines; misses mid-transfer connection errors | fix-now | fixed (fix round 1) |
| P-C4 | Spec | P1 | `factory-agent/src/tests/workspace.rs:23` | foreign-CA failure and `.git/config` sslCAInfo path untested | fix-now | fixed (fix round 1) |
| P-A6 | Spec | P1 | `git/upstream.rs:28` | 30 s response-head timeout includes the upload | fix-now | fixed (fix round 2) |
| S-A6, S-A7 | Standards | P1 | `git/server.rs:42, 220` | `Content-Encoding` forwarded on receive-pack; raw query forwarded | fix-now | fixed (fix round 2) |
| S-A9, S-A10, P-A11 | Both | P1 | `git/records.rs:16, 76` | queue task aborted on drop; doc says oldest dropped | fix-now | fixed (fix round 2) |
| S-A11 | Standards | P1 | `git/push.rs` | stale spool/quarantine after a crash | fix-now | fixed (fix round 2) |
| S-A13, S-A14, P-A10, S-A15 | Both | P1 | `mirror.rs:91`, `upstream.rs:179`, `server.rs:112`, `refs.rs:72`, `spool.rs:79` | URL check three times, dead code, file order | fix-now | fixed (fix round 2) |
| P-A7, P-A8 | Spec | P1 | `tests/git_push.rs`, `tests/git_fetch.rs` | REJECTED record and partial-clone filter untested | fix-now | fixed (fix rounds 1 and 2) |
| P-B4, S-B1 | Both | P1 | `df-db/src/git_pushes.rs:130` | `push_id` reused with another run advances the record | fix-now: reject mismatch | fixed (fix round 1) |
| S-B2 | Standards | P1 | `df-proto/build.rs` | pbjson `Serialize` of snapshot types includes the password | fix-now | fixed (fix round 1) |
| S-B8 | Standards | P1 | `control/snapshot.rs:143` | credentials read for runs without a route | fix-now | fixed (fix round 1) |
| P-B1, P-B2, S-B6, P-B3 | Both | P1 | `control/tests.rs`, `policy_service.rs:247`, `git_pushes/tests.rs` | RecordPush over mTLS, periodic rebuild through the stream, realistic retries untested | fix-now | fixed (fix round 1) |
| S-B3, S-B5, S-B7, S-B10, S-C11 | Standards | P1 | various | `Uuid` re-export, `serve` naming/docs, stale doc, duplicated default, chart `gitHost` helper | fix-now | fixed (fix round 1) |
| P-A9, P-B5, P-B6, P-C6, P-C7, P-C2 | Spec | P1 | specs, docs | spec/doc wording: relayed headers, `unknown` → final, full ref names, retry timing, Git host name, upgrade note + iter1-demo-e2e seed | fix-now | fixed (fix round 1) |
| flaky | — | P2 | `df-gateway` relay and proxy tests | pre-existing timing sensitivity (also on `main`) | tech-debt | tech-debt (TD-83) |
| S-A12, S-A16–S-A18, S-B9, S-B11–S-B13, S-C5, S-C9, S-C10, S-C12 | Standards | P3 | various | naming, small duplication, low-value tests | cosmetic | cosmetic |
| P-B7 | Spec | Rejected | `df-agent-channel/src/tools.rs:342` | comment URL returned to the agent | rejected (no credential; pre-existing) | rejected |

User decision: Fix blocking + P1.

Fix round 1: 01260a8, 6d39b84, cd51795, 3dc9285, 221f496, 91b2a0c, e6c92a0, f95facf, 5276990, e3a1e4e, e7615f7, 6dfac36, bf59a5f. Checks before round 2: fmt, workspace clippy, 706 tests across df-db, df-proto, df-core, df-k8s, df-agent-channel, factory-agent, dark-factory, df-gateway (containers via OrbStack; the two pre-existing flaky gateway tests skipped), Bazel `--lockfile_mode=error`, `openspec validate --all --strict`, `cargo deny`, check-crates, check-file-length. The orchestrator did not dispatch the planned gateway P1 chunk (F2), so S-A6, S-A7, S-A9, S-A10/P-A11, S-A11, S-A13, S-A14/P-A10, S-A15, P-A6 and P-A8 stayed open.

## Round 2 (2026-10-31)

- Reviewed range: b112b93...bf59a5f (fix round 1) plus the ledger check: 40 of 54 fix-now rows fixed; the 14 not fixed are the undispatched F2 items above, and P-A4 is only partly fixed (see P2-1)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P2-1 | Spec | Blocking | `git/push.rs:202`, `control/git_records.rs:93` | records of pushes without parsed commands (malformed, early body failure) have no refs; the control plane rejects them and the recorder retries for 5 s | fix-now | fixed (fix round 2) |
| P2-2 | Spec | Blocking | `git/push.rs:334`, `mirror/transport.rs:258` | forge 404/5xx during the mirror fetch → 502 + REJECTED instead of relayed or `upstream-failed` | fix-now | fixed (fix rounds 2 and 3) |
| P2-4 | Spec | Blocking | `specs/git-hardening/spec.md` | spec still describes a global `http.sslCAInfo` (now scoped to the proxy URL) | fix-now | fixed (fix round 2) |
| extra | Spec | Blocking | `specs/run-orchestration/spec.md` | "Assign content" description 523 characters (> 500) | fix-now | fixed (fix round 2) |
| S2-2 | Standards | Blocking (fix code) | `git/upstream.rs:298` | idle timer of `LimitedBody` starts before connect and TLS | fix-now | fixed (fix round 2) |
| S2-3 | Standards | Blocking (fix code) | `git/push.rs:291` | gateway disk error recorded as `df:unreadable-push` | fix-now | fixed (fix round 2) |
| S2-6 | Standards | Blocking (fix code) | `control/tests.rs:376` | refresh test depends on build time | fix-now | fixed (fix round 2) |
| S2-8 | Standards | Blocking (fix code) | `templates/certs.yaml:86` | `<fullname>-gateway.<namespace>` SAN dropped without an upgrade note | fix-now | fixed (fix round 2) |
| S2-9 | Standards | Blocking (fix code) | `factory-agent/src/git.rs:317` | `url_prefix` guard untested against section injection, needlessly `pub` | fix-now | fixed (fix round 2) |
| S2-1, S2-4, S2-5, S2-7, S2-10, P2-3 | Both | P3 | various | file order, string literal, two failure cells, name, actor of the denial in docs | fix-now (user: fix everything) | fixed (fix round 2) |

User decision: fix everything listed, including the undispatched round 1 items.

Fix round 2: f6ce416, c132c40, e55a5ca, 1f1a947, cbb09c2, 010f07b, 1c4dbc9, 0182694, 1af8145, a12e50a, 607b90f, b248135, 5a68d37, 0f5083d. A partial G2 change (`ForgeUrl` checked against the request URI with its query) broke 8 `git_push` tests; the orchestrator stashed it, the next implementer fixed and applied it. Checks before round 3: fmt, workspace clippy, 719 tests (containers; the two flaky gateway tests skipped), Bazel lock, `openspec validate --all --strict`, deny, check-crates, check-file-length, check-test-names.

## Round 3 (2026-10-31)

- Reviewed range: bf59a5f...0f5083d plus the ledger check: all round 1/2 Standards rows fixed; Spec rows fixed except P2-2 (partial: behaviour changed, no test, spec not amended)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P3-2 | Spec | Blocking | `tests/git_push.rs` | "Forge down" untested on the real first contact (mirror fetch): refused connection, 401, 500 | fix-now | fixed (f74c652) |
| P3-1 | Spec | Blocking | `git/push.rs:342`, `specs/git-proxy/spec.md` | forge error during the mirror fetch → 502 + `upstream-failed`, spec still says relayed | fix-now: amend spec | fixed (d7d53bb) |
| P3-3 | Spec | Blocking | `tests/git_push.rs:353` | record untested for push options, invalid pack, spool failures | fix-now | fixed (f74c652) |
| S3-2 | Standards | P1 | `git/push.rs:341` | `MirrorError::Fetch` mixes forge status errors and local gix errors | fix-now | fixed (3440ebe) |
| S3-1 | Standards | P1 | `git/server.rs:98` | dead `GitRequest.query` field | fix-now | fixed (0c4fe51) |
| S3-5 | Standards | P1 | `tests/git_push.rs:582` | `curl` as a new host-tool dependency of a test | fix-now | fixed (f74c652) |
| S3-3, S3-4, S3-6 | Standards | P3 | `push.rs:153`, `records/tests.rs:294`, `factory-agent/src/git.rs:240` | blocking cleanup in a constructor, elapsed-time assertion, control characters dropped silently | cosmetic | cosmetic |

User decision: Fix blocking + P1 (fix round 3, the last one allowed by the skill).

Fix round 3: 3440ebe (S3-2: `MirrorError` split into `Unreachable`, `Rejected`, `ForgeStatus`, `Other`), f74c652 (P3-2, P3-3, S3-5), d7d53bb (P3-1: scenario "Forge error during the pre-push fetch"), 0c4fe51 (S3-1). The final check found `git_fetch_test`, `git_mirror_test` and `git_push_test` failing to build under Bazel (missing `//crates/df-proto:df_proto`; cargo was green): 37185bd.

Final checks at 37185bd: fmt, workspace clippy, 722 tests across df-db, df-proto, df-core, df-k8s, df-agent-channel, factory-agent, dark-factory, df-gateway (containers via OrbStack; the two flaky gateway tests skipped), Bazel `--lockfile_mode=error`, Bazel tests `//crates/df-gateway/...` and `//deploy:helm_chart_test` 7/7, `openspec validate --all --strict` 46/46, deny, check-crates, check-file-length, check-test-names. The relay test passed 4 of 4 runs on the branch and 3 of 4 on `main` (fd5cf70): flaky before this change, not a regression.

Close-out: the flaky tests are TD-83. Roadmap: no new rows; the non-goals are covered (Git LFS in the backlog, SSH repository URLs via `iter2-multi-forge`, branch rewrites ADR-0033 and the audit log view with push records in iterations) or rejected (dumb HTTP, Git traffic in the live egress log). Open: 6.1 (dev cluster, Peter).

Rebase onto `main` (80c17fd, 2026-10-31): conflicts with `iter1-demo-e2e` resolved (both config tests kept; `Git` gets the proxy CA and `allow_plain_http`; TD IDs renumbered, the flaky tests are TD-83). Semantic conflict: `forge.allowPlainHttp` (Forgejo over `http://` in `just e2e-local`) does not reach the Git proxy, which routes `https://` repositories only. User decision (after first choosing to push as is with a TD entry, revised with Peter): the gateway honors `forge.allowPlainHttp` for the Git upstream and the mirror fetch (checked by the control plane when it builds routes and by the gateway itself), sandboxes always use HTTPS to the gateway, and `DF_GIT_ALLOW_PLAIN_HTTP` is removed from the runtime and the sandbox pods. New requirement "Plain-HTTP forges only when allowed" in the `git-proxy` spec; tests: snapshot route, `ForgeUrl`, push to a plain-HTTP forge with and without the setting.

## PR review (#64, 2026-10-31)

Rebased onto `main` (9b9cd99); only `MODULE.bazel.lock` conflicted (regenerated).

| Finding | Fix |
|---|---|
| Refused pushes recorded as `ACCEPTED` (major) | the gateway buffers the forge's answer (≤ 4 MiB, 5 min), reads `report-status` (side-band demuxed) and records `UPSTREAM_FAILED` on `ng`, unpack error, band 3 or an incomplete report; test with a forge `pre-receive` hook |
| `http://` route without `allowPlainHttp` answered `500` on push | `MirrorError::InvalidUrl` → `502` `df: invalid forge URL`, `UPSTREAM_FAILED`; asserted for `info/refs` and the receive-pack POST |
| `GIT_SSL_CAINFO` undid the CA scoping | never set (removed from the git env); the clone gets `-c http.<proxy prefix>.sslCAInfo=<ca>` |
| Duplicated scheme/port handling | `ForgeConnector::connect(&ForgeUrl)`; the mirror transport keeps the parsed `ForgeUrl` |
| `ForgeTls`/`HttpsUpstream` names | renamed `ForgeConnector`/`ForgeUpstream` |
| Mirror fetch outside `CHECK_DEADLINE` | one deadline covers mirror fetch, queueing, quarantine and checks |
| Second sandbox lookup, `route` + `credential` clump | `identify` returns `ForgeTarget { route, credential }` and the run id |
| Mirror opened without isolation in the quarantine | `gix::open_opts(…, isolated())` |
| `serve_internal` in `policy_service.rs` | moved to `control/internal.rs` |
| Snapshot doc, seed Forgejo allowlist entry | doc fixed; `ensure_forge_entry` removed |
| Deprecated `CredentialRequest`/`CredentialGrant` (PR question) | removed, fields 14/16 reserved (pre-MVP) |
