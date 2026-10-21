# Design

## Context

- `iter2-git-proxy` spools each push, indexes it into a quarantine on top of a fresh mirror, applies the ref rules and calls `PushChecks` (no-op until now) before forwarding. Push records go to the control plane through `RecordPush`.
- `iter2-agent-requests` provides `ApprovalType`, the decision transaction and `request.decided` messages.
- The gateway has no database. Everything it needs per push (policy, exceptions, commit identity) has to be in the snapshot.

## Goals / Non-Goals

**Goals:**
- Each check is a pure function over a `PushView` (added commits with their parents, changed paths, added blobs). That makes every check unit-testable without Git or a network.
- The gateway decides alone from its snapshot; no per-push call to the control plane besides the record.

**Non-Goals:**
- `push-check` WASI plugins, signature verification, file-type rules, LFS.
- Exceptions wider than the task (project scope), and exceptions for hard-reject checks.

## Decisions

### D1 — Push view
From mirror + quarantine:
1. Added commits: walk from every pushed new SHA, stopping at commits reachable from any mirror ref (gix revision walk with the mirror tips as hidden). The walk is bounded by a maximum of 10,000 commits; above that the push is rejected with `df:push-too-large`.
2. For each added commit: its tree diffs against each parent (or the empty tree for a root commit), giving changed paths with old and new blob ids. Renames count as delete + add, so both paths are checked.
3. Added blobs: new blob ids from those diffs that are not in the mirror, each read once (with a size header first, so the size check doesn't load huge blobs).

### D2 — Checks and findings
```rust
pub struct Finding { check: Check, mode: Mode, fingerprint: String, path: String,
                     line: Option<u32>, detail: String /* masked */ }
fn check_push(view: &PushView, policy: &PushPolicy, identity: &Identity,
              target: Target /* TaskBranch | Wip */) -> Vec<Finding>
```
- Identity, `.dark-factory/**` and merge commits produce `Mode::Reject`; the others use the policy's mode. For `Target::Wip`, every finding is forced to `Reject` and only secret/size run.
- Fingerprints: `secret:<rule>:<blob>:<line>`, `path:<path>`, `size:<path>:<blob>`.
- Verdict: findings minus exceptions (matched by fingerprint). None left → forward. Any `Reject` left → reject. Only `Approval` left → reject and mark the record `approval_requested`.
- `report-status` reasons: `df:<check> <path>[:<line>] (<masked>)`, one `ng` line per rejected ref, with every finding joined (truncated at 1 KiB per ref).

### D3 — Secret scanning
Spike 1.1 decides between:
- (a) the `kingfisher` crate as a library, if it exposes a scanner API over byte slices;
- (b) Kingfisher's rule files (Apache-2.0), vendored under `crates/df-gateway/rules/` and compiled once into a `regex::bytes::RegexSet` plus per-rule `regex::bytes::Regex` for locating matches.

Option (a) is preferred if it adds no large dependency tree. Either way, every added blob is scanned as bytes, including binary ones: there is no NUL-byte heuristic, because keys in binary files (keystores, archives, compiled config) or after an early NUL must still be found. The line number is counted over `\n` bytes. Masking keeps the first 4 characters and replaces the rest with `*`, showing at most 12 characters in total. Non-UTF-8 match bytes are shown as `\xNN` escapes before masking.

### D4 — Policy storage and distribution
- `project_push_policies(project_id PK, protected_globs text[], protected_mode, secret_mode, size_mode, max_blob_bytes, linear_history, updated_at)`. A missing row means the defaults from the spec.
- `push_check_exceptions(task_id, fingerprint, request_id, created_at)`, primary key `(task_id, fingerprint)`.
- The snapshot carries `PushPolicy` per project (deduplicated like credentials) and `repeated string exceptions` per sandbox entry. Writes publish `egress_policy` (the existing snapshot trigger).
- Globs use `globset` with `literal_separator(true)` and are validated when saved.

### D5 — Control plane reactions to push records
In the `RecordPush` transaction, reacting only to final verdicts (`REJECTED`, `ACCEPTED`; never to `FORWARDING`):
- `verdict = REJECTED` with `approval_requested` → raise `push-check-exception` (payload: the findings with check, path, line, masked preview and fingerprint). Merging uses the sorted fingerprint set.
- Rejected by checks on the task branch → queue `push.rejected` unless an undelivered one exists or `tasks.push_rejections_since_accept >= 3`; increment the counter.
- Accepted task-branch push → reset the counter and close pending `push-check-exception` requests (`push-accepted`).

The approval type (`ApprovalType` from `iter2-agent-requests`) offers scope `Task` only. `apply` inserts one exception per finding fingerprint and publishes `egress_policy`.

### D6 — Runtime mapping
The runtime already parses `git push --porcelain` output per ref. A `!` line maps by its reason prefix:

| Reason | Status |
|---|---|
| `df:secret-scan` | `REJECTED_SECRET_DETECTED` |
| `df:identity`, `df:protected-path`, `df:blob-too-large`, `df:merge-commit`, `df:push-too-large` | `REJECTED_BY_CHECK` |
| `df:non-fast-forward` (ref rule) | `REJECTED_NON_FAST_FORWARD`, like a forge-side non-fast-forward |
| `df:audit-unavailable`, `df:ref-not-allowed`, unknown `df:` reasons | `FAILED` |

The reason text is always the detail. "Last pushed commit" is updated only on success, so the next turn retries.

## Risks / Trade-offs

- [False positives block a task] → secret findings are approval-gated per finding by default.
- [An approved secret exception lets a real secret reach the forge] → the card shows the masked match, rule, path and line. Approving is an explicit, audited human decision for exactly that finding.
- [Large pushes make checks slow, also because binary blobs are scanned] → the push size limit (256 MiB) bounds the scanned bytes, the commit walk is bounded, and the regex set runs once per blob. Pushes happen once per turn.
- [Agent loops on rejections] → feedback capped at 3 consecutive messages; budgets still apply.
