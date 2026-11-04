# Design

## Context

See proposal.md (Why) and [ADR-0046](../../../docs/adr/0046-forge-activity-in-the-transcript.md) for the decision and the rejected alternatives.

Current state:

- `transcript_events` rows are written only by `df_agent_channel::transcript::insert`, keyed `UNIQUE (run_id, seq)` with `CHECK (seq >= 1)`; `id` (BIGSERIAL) is the task-wide order used by the UI and SSE resume (ADR-0042). Payloads are canonical protobuf JSON of `transcript.v1.Event`.
- The gateway reports pushes through `RecordPush`; `df_db::git_pushes::record_push` stores them and writes the `git.push` audit entry in one transaction once the verdict is final.
- `df_agent_channel::pr` opens or finds the PR (`try_ensure_draft` → `record_pr`, inside a transaction with a per-task advisory lock) and marks it ready in `on_run_end`.
- Readers of `transcript_events` that feed the agent (`assign::transcript_summary`) or the runtime session (`session::stored_tool_call`, `last_turn_event`) filter by runtime kinds; the web folder (`df_web::transcript::Folder`) renders every kind.

## Goals / Non-Goals

**Goals:**
- One write path for control-plane transcript events, used by the push and PR code.
- Forge activity events committed atomically with the fact they describe (push record, PR record).

**Non-Goals:**
- Events outside a run: backlog, "Forge activity outside runs" in `product/iterations/roadmap.md`.
- Commit or compare links: backlog, "Commit links in forge activity" in `product/iterations/roadmap.md`.
- Backfilling forge activity for pushes and PRs recorded before the migration: rejected (pre-MVP, no production data).
- A runtime-side or ACP representation of forge activity: rejected (control-plane only, never sent to the agent).

## Decisions

### D1 — Proto: one `ForgeActivity` kind with a `oneof`

```proto
// In Event.kind:
ForgeActivity forge_activity = 19;

// Written by the control plane, never by the runtime. Display-only.
message ForgeActivity {
  oneof kind {
    BranchPushed branch_pushed = 1;
    PullRequestOpened pull_request_opened = 2;
    PullRequestReady pull_request_ready = 3;
  }
}

message BranchPushed {
  string ref = 1;          // full ref name
  string old_sha = 2;      // zero SHA for a created branch
  string new_sha = 3;
  PushVerdict verdict = 4; // ACCEPTED, REJECTED, UPSTREAM_FAILED
  string reason = 5;       // df: reason code of the ref, empty if accepted
}

message PullRequestOpened { uint64 number = 1; string url = 2; bool draft = 3; }
message PullRequestReady  { uint64 number = 1; string url = 2; }
```

`kind` column value: `forge_activity`. `turn_id` is empty: forge activity belongs to no turn. Additive within v1 (ADR-0017). One kind instead of three top-level kinds keeps "written by the control plane" in one place for readers that switch on `kind`.

*Alternative:* three top-level kinds. Rejected: every exhaustive `match` on `event::Kind` grows by three for one concept.

### D2 — Schema: nullable `seq`, new `source_key`

Migration:

```sql
ALTER TABLE transcript_events ALTER COLUMN seq DROP NOT NULL;
ALTER TABLE transcript_events ADD COLUMN source_key TEXT;
ALTER TABLE transcript_events ADD CONSTRAINT transcript_events_source_check
    CHECK (num_nonnulls(seq, source_key) = 1);
ALTER TABLE transcript_events ADD CONSTRAINT transcript_events_run_source_key_unique
    UNIQUE (run_id, source_key);
```

`UNIQUE (run_id, seq)` and `CHECK (seq >= 1)` stay: NULLs are distinct and pass the check. Source keys: `push:<push_id>`, `pr-opened:<number>`, `pr-ready:<number>`. Inserts use `ON CONFLICT (run_id, source_key) DO NOTHING`.

`pr-opened` and `pr-ready` are per run by the unique constraint, so a task whose PR is marked ready in run 2 and again in run 4 shows both; "opened" happens once per task because `record_pr` only writes when `pr_number` was unset.

*Alternative:* negative `seq` for the control plane. Rejected in ADR-0046.

### D3 — One insert function in `df-db`

`df_db::transcript::append_control_event(tx, task_id, run_id, source_key, &Event) -> Result<Option<i64>, DbError>` serializes the event to canonical JSON, inserts with `occurred_at = now()` and, if a row was inserted, publishes `Notification::TranscriptEvent { task_id, id }` in the same transaction (`pg_notify` is delivered on commit). Both callers already hold a transaction; `df-db` already depends on `df-proto`.

Before allocating the id it locks the run row and then the task row (`FOR UPDATE`), the order `session::lock_run_and_task` uses. Event ids are the cursor of the live transcript (ADR-0042) and must become visible in id order: without the task lock, a control-plane transaction could take an id, commit after a runtime transaction with a higher id, and the SSE cursor would already be past it. A caller that writes the task row itself (`record_pr`) takes these locks before that write, so the order stays run, task.

### D4 — Push events in `record_push`

Inside the existing transaction, when `record.verdict.is_final()` (the same condition as the audit entry):

1. Load the run's task id and key (`runs` → `tasks`); the task branch is `refs/heads/` + `TaskKey::task_branch()`.
2. Find the `RefUpdate` with that name. None → no event (WIP branch only, or a ref outside the scope).
3. Append `BranchPushed` with that ref's SHAs, **that ref's** verdict and reason, key `push:<push_id>`.

The gateway currently writes the push's overall verdict into every `RefUpdate`: a push whose task ref the forge accepted while it refused another ref is `upstream-failed` for all refs, and only the `df:forge-refused` reason is per ref. `Entry::record` in `df-gateway` therefore records each ref's own outcome: `accepted` for a ref the forge did not refuse when the forge answered with a `report-status`, otherwise the overall verdict (a whole-push failure, such as an unreachable forge, fails every ref). The push record's overall verdict is unchanged.

Runs on `Inserted` and `Advanced`; on `Unchanged` (retried final verdict) the function returns before this point, and the source key makes a second write a no-op anyway.

### D5 — PR events in `pr.rs`

- `record_pr`: when `rows_affected() > 0`, append `PullRequestOpened { number, url, draft }` with key `pr-opened:<number>` to the current run, in the same transaction. Needs the run id passed in (`RunContext` is available at both call sites). `draft` comes from the forge's `PullRequest.draft` (in the WIT record and `df_core::seams::PullRequest`).
- `on_run_end`: after `update_pull_request` succeeds, append `PullRequestReady` with key `pr-ready:<number>` in its own short transaction. `try_ensure_draft` returns the recorded number **and** URL (`current` was loaded before it may have recorded the PR, so `current.task.pr_url` can be empty); the event uses that URL.

### D6 — Runtime cannot write forge activity

`Event` is shared with the runtime's transcript channel, so a runtime could send a `ForgeActivity` itself. `transcript::prepare` skips it: the event is not stored, the sequence number is still acknowledged (the outbox moves on), and a warning is logged with the run id. Forge activity therefore only exists with a `source_key`, never with a `seq`.

### D7 — Agent isolation

No code change is needed: agent-facing readers select runtime kinds by name. Task 4.3 adds a test that `transcript_summary` ignores `forge_activity` rows, so a later change that widens the filter fails.

### D8 — Rendering

`ItemKind::Forge(ForgeItem)` in `df_web::transcript`, pushed like a notice (`ev-<id>`, no author). `render_item` reuses the notice row markup with:

| Variant | Icon | Text | Tint |
|---|---|---|---|
| `BranchPushed` accepted | `Icon::GitCommit` (`git-commit-horizontal`) | `Pushed <branch> <old7>..<new7>` (`<new7>` alone for a created branch) | `accent` |
| `BranchPushed` rejected / upstream-failed | `Icon::GitCommit` | `Push to <branch> rejected <reason>` | `danger` |
| `PullRequestOpened` draft | `Icon::PullRequestDraft` (`git-pull-request-draft`) | `Opened draft PR #<n>` | `accent` |
| `PullRequestOpened` non-draft | `Icon::PullRequest` | `Opened PR #<n>` | `accent` |
| `PullRequestReady` | `Icon::PullRequest` | `PR #<n> ready for review` | `accent` |

The PR number is an `<a>` only if the URL's scheme is `http` or `https` (task-ui "Forge activity links are safe"). Forge rows share divider lines with tool and notice rows, as notices do. Live updates need no change: SSE already folds every new event.

## Risks / Trade-offs

- [A reader assumes `seq` is non-null] → `query!` macros infer nullability from the schema, so typed readers fail to compile after the migration; untyped readers (`query_as` with `i64`) are found with `grep -rn "seq" crates/*/src` in task 1.3 and the existing readers all filter by runtime kinds.
- [Push event written before the forge shows the commits] → only final verdicts are used; `accepted` is set after the forge answered the forwarded push.
- [PR "opened" time is the record time, not the forge's creation time, when an existing PR is found] → accepted; the row marks when Dark Factory took ownership of the PR.
- [Clock: `occurred_at = now()` of the control plane next to runtime timestamps] → ordering uses `id`, not `occurred_at`; the timestamp is display only.

## Migration Plan

One additive migration (D2); no backfill. Rollback: drop the constraints and column and restore `NOT NULL` after deleting `forge_activity` rows. Older runtimes are unaffected: they never read transcript events.
