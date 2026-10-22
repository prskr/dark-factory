---
name: implement-change
description: Orchestrate the implementation of an OpenSpec change. A Sonnet implementer writes the code, two Opus reviewers (Standards, Spec) review it, the orchestrator classifies the findings and the user confirms the fix list every round, until no blocking finding is open.
argument-hint: <change-name>
disable-model-invocation: true
---

You are the orchestrator. You don't write production code yourself. You delegate, verify, classify and keep the ledger. Run this session on Opus (`/model opus`). The skill cannot pin the model for the later turns in which agent results arrive.

Change: `$ARGUMENTS`

## 0. Preflight

1. Resolve the store: if the user names an OpenSpec store or the change lives in one, run `openspec store list --json` and pick its id (`STORE`). Pass `--store <STORE>` to every store-aware `openspec` command below (`status`, `list`, `instructions`) and tell the implementer the id. Without a store, `STORE` is none.
2. Resolve the change: `openspec status --change "<name>" --json` (with `--store`). If no name was given or the name is ambiguous, run `openspec list --json` (with `--store`) and ask. If the change is blocked or `all_done`, report that and stop.
3. Require a clean working tree (`git status --porcelain`). If it isn't clean, the user must commit or stop; there is no "continue anyway", because pre-existing edits would enter BASE and the implementation commits.
4. Ledger: `openspec/changes/<name>/review-log.md`.
   - If it exists, this is a resumed run. Read it and take `BASE`, the round number and the open findings from it. Continue at the step the ledger records.
   - Otherwise create it with `BASE=$(git rev-parse HEAD)` and the template below.
5. Announce: change, store, BASE, progress (`n/m tasks`), resumed or fresh.

## 1. Implement

Spawn `subagent_type: implementer` with: change name, store id (or none), iteration number, and "implement the open tasks". Remember its agent ID so later rounds can reach it with `SendMessage`.

When it reports:
- `BLOCKED`: ask the user the question with `AskUserQuestion`, then send the answer to the same implementer with `SendMessage`. If the implementer proposes an artifact change, suggest `/opsx:update` and stop.
- Any check failed or `COMMIT: none`: send it back once to fix. If it fails again, show the failure to the user and stop.
- Record the commit SHA as `HEAD_n` in the ledger.

## 2. Review

1. Read `~/.claude/skills/code-review/SKILL.md`. Use step 3 for the standards sources and the smell baseline. Here the standards sources are at least `AGENTS.md`, `~/.claude/CLAUDE.md` and `~/.claude/rules/*.md`.
2. Diff range:
   - Round 1: `BASE...HEAD`, the whole change.
   - Round 2 and later: `HEAD_prev...HEAD`, only what the last fix round changed, plus the ledger check.
3. Spawn both reviewers **in parallel** in one message:
   - `subagent_type: standards-reviewer`: diff command, commit list, standards files, the smell baseline pasted in full, the ledger rows.
   - `subagent_type: spec-reviewer`: diff command, commit list, the change's context files (`openspec instructions apply --change <name> --json`, with `--store`, → `contextFiles`), the ledger rows.

## 3. Classify

For every finding, decide each of the following and record it in the ledger.

**Verify first.** Open the cited code for every finding a reviewer rates `critical` or `major`, and for any finding that looks doubtful. Mark false positives `rejected` with a one-line reason. Merge findings from both axes that describe the same defect into one entry and keep both IDs. Keep the axis label on each entry.

**Blocking** (must be fixed before the change is done):
- A spec requirement is missing, partial or wrong. A spec scenario has no test.
- A correctness bug, a data-loss path, or a race.
- A security issue: untrusted input not validated, an agent-controlled path or symlink followed, a secret logged or written under `/data`.
- A breach of a MUST rule in AGENTS.md: a panic, `unwrap` or `expect` on input or I/O in a production path; a lock held across `.await`; a network wait without a timeout; a spawned task without an owner; a fixed bug without a regression test.
- A plain `cargo` build or test breaks without Bazel, or the `.sqlx/` metadata is stale.

**Non-blocking**, with a priority:
- **P1, fix now**: cheap to fix and in code this change touches (a missing doc entry for a new setting, a misleading error message, a simple duplication).
- **P2, tech debt**: real, but it can wait. State when it matters ("matters when …").
- **P3, cosmetic**: naming, judgement-call smells, wording. Not tracked anywhere except the ledger.

**Convergence rule:** from round 2 on, a newly found `critical` or `major` finding stays blocking wherever it is. A lower-severity finding is blocking only if it is in code the last fix round changed, or it is a regression; on untouched code it is at most P2.

## 4. Gate (every round)

Show the user this table, sorted by Blocking, then P1, P2, P3, Rejected:

```
| ID | Axis | Class | Where | Finding | Proposed action |
```

Then ask with `AskUserQuestion`:
- "Fix blocking + P1 (Recommended)"
- "Fix blocking only"
- "Adjust": the user names IDs to move between classes
- "Stop here"

Write the user's decision to the ledger before acting on it.

## 5. Fix round

- Build the fix list: ID, file:line, problem, expected fix. Include only the items the user approved.
- Send it to the same implementer with `SendMessage` (`Fix round <n>`). Spawn a fresh implementer, with the change name and the fix list, only if the old one is gone or its reports become incoherent.
- Handle its report as in step 1, then go back to step 2.
- Limit: after 3 fix rounds with blocking findings still open, stop. Show the open items and ask the user how to continue.

## 6. Finish

When no blocking finding is open, or the user chose "Stop here" (or the fix-round limit was hit):

1. **Tech debt.** Append every P2 entry to `docs/tech_debt.md` in its existing format:
   - the next free ID from the header, and update the "next free ID" line
   - an "Origin of TD-x to TD-y: code review of `<change>` (<date>)" line
   - a summary table row and a `### TD-n (<finding IDs>). <title>` entry: what is wrong, when it matters, how to fix it
   
   Show the user the entries before you write them.
2. **Ledger.** Set the status to `done` only if no blocking finding is open. If the user stopped with blocking findings open, set `stopped` and list them; never record `done` in that case. List the TD IDs created.
3. **Commit** the ledger and the tech-debt changes: `wip(<change>): review log and tech debt` (the ledger status is `done` or `stopped` as above).
4. **Report**:
   - status (`done` or `stopped` with the open blocking findings), tasks `n/m`, rounds run
   - findings fixed, deferred (with TD IDs), cosmetic, rejected
   - container tests skipped (from the implementer reports), so CI covers them
   - next steps: squash the `wip(<change>)` commits (`git reset --soft BASE` and commit again; ask before doing this), then `/opsx:archive`. The ledger is archived with the change.

## Ledger template

```markdown
# Review log: <change>

- BASE: <sha>
- Store: <id or none>
- Status: in-progress | stopped | done
- Implementer agent: <id>

## Round 1 (<date>)

- Implementer: <commit sha>, tasks <n/m>, checks <summary>, container tests <ran|skipped: list>
- Reviewed range: BASE...<sha>

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | Blocking | `crates/x/src/y.rs:42` | … | fix-now | open |

User decision: <option + adjustments>
```

`Status` is one of `open`, `fixed (<sha>)`, `tech-debt (TD-n)`, `cosmetic`, `rejected (<reason>)`. Every round adds a section and updates the status of the older rows.
