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
3. Confirm the scope (skip on a resumed run, i.e. if `openspec/changes/<name>/review-log.md` exists). Read the change's proposal, `design.md`, specs and `tasks.md`, then summarize for the user in a few bullets: the goal, the behavior the specs require, the main design decisions, and the open tasks (`n/m`). Ask with `AskUserQuestion`: "Start implementation (Recommended)" / "Update the change first (say what)" / "Stop". On an update request, suggest `/opsx:update` with the user's notes and stop; the user restarts this skill once the artifacts are committed.
4. Require a clean working tree (`git status --porcelain`). If it isn't clean, the user must commit or stop; there is no "continue anyway", because pre-existing edits would enter BASE and the implementation commits.
5. Ledger: `openspec/changes/<name>/review-log.md`.
   - If it exists, this is a resumed run. Read it and take `BASE`, the round number and the open findings from it. Continue at the step the ledger records.
   - Otherwise create it with `BASE=$(git rev-parse HEAD)` and the template below.
6. Design tasks: read `tasks.md` and list the open tasks that produce or change a design (they name PenPot, a board, a mockup or the design mapping in `docs/design-workflow.md`). Call them `DESIGN`. If `DESIGN` is not empty, check that the PenPot MCP is connected to the right file, as in `docs/design-workflow.md`, "Check the setup": through `execute_code`, `penpot.currentFile.name` must be `App Design` and `penpotUtils.getPages()` must list the nine pages. If either check fails, ask the user to connect or select the right file before going on; do not start the implementer with open design tasks.
7. Container socket: probe for one, as described in `AGENTS.md` ("Containers and integration tests in a sandbox"): `$XDG_RUNTIME_DIR/podman/podman.sock` and `/var/run/docker.sock`, each with `curl -sf --max-time 3 --unix-socket <socket> http://d/_ping`. Remember the answering socket as `DOCKER_HOST=unix://<socket>` (`DOCKER_HOST` is none if nothing answers). Never start or install Docker or Podman.
8. Announce: change, store, BASE, progress (`n/m tasks`), design tasks open (`DESIGN` or none), `DOCKER_HOST` (or none), resumed or fresh.

## 1. Design (only if `DESIGN` is not empty)

The implementer cannot ask questions and has no judgement about the look, so the orchestrator does the open design tasks itself, before any code is written. Follow `docs/design-workflow.md`.

1. Read the change's `design.md` and specs, and the existing boards the task refers to.
2. Do each design task in PenPot (clone the nearest board, then change it). Export the board with `export_shape` and look at it.
3. Show the result to the user and ask with `AskUserQuestion`: "Approve" / "Change it (say what)" / "Stop". On a change request, iterate and ask again. Don't go on without approval.
4. Add the mapping row to `docs/design-workflow.md`, tick the design tasks in `tasks.md`, and commit them as `wip(<change>): design`. Record the SHA in the ledger.
5. If the design shows that `design.md` or a spec is wrong or incomplete, suggest `/opsx:update` and stop.

## 2. Implement

Spawn `subagent_type: implementer` with: change name, store id (or none), iteration number, and "implement the open tasks". Tell it the design tasks are done and the PenPot boards are the reference; it must not edit PenPot. With a `DOCKER_HOST`, pass it so it runs the container-backed tests and the browser check instead of skipping them (`AGENTS.md` allows using a socket that answers; the announce line shows it). Without one, pass "no container socket": it runs only the tests it has identified as needing no containers and reports the container-backed ones it skipped. Before every fix round, probe the socket again and pass the current status: a socket can stop answering, or come back, during a run. Remember its agent ID so later rounds can reach it with `SendMessage`.

When it reports:
- `BLOCKED`: ask the user the question with `AskUserQuestion`, then send the answer to the same implementer with `SendMessage`. If the implementer proposes an artifact change, suggest `/opsx:update` and stop.
- Any check failed or `COMMIT: none`: send it back once to fix. If it fails again, show the failure to the user and stop.
- Record the commit SHA as `HEAD_n` in the ledger.

## 3. Review

1. Read `~/.claude/skills/code-review/SKILL.md`. Use step 3 for the standards sources and the smell baseline. Here the standards sources are at least `AGENTS.md`, `~/.claude/CLAUDE.md` and `~/.claude/rules/*.md`.
2. Diff range:
   - Round 1: `BASE...HEAD`, the whole change.
   - Round 2 and later: `HEAD_prev...HEAD`, only what the last fix round changed, plus the ledger check.
3. Spawn both reviewers **in parallel** in one message:
   - `subagent_type: standards-reviewer`: diff command, commit list, standards files, the smell baseline pasted in full, the ledger rows.
   - `subagent_type: spec-reviewer`: diff command, commit list, the change's context files (`openspec instructions apply --change <name> --json`, with `--store`, → `contextFiles`), the ledger rows.

## 4. Classify

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

## 5. Gate (every round)

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

## 6. Fix round

- Build the fix list: ID, file:line, problem, expected fix. Include only the items the user approved. Describe the expected behavior and the test that proves it. Prescribe a mechanism only if you verified it against the code; an unverified "how" from the orchestrator becomes the next round's regression. Tests are named after the behavior they assert, never after a finding ID, round or review (`AGENTS.md`, "File structure"): the fix list says so for every test it asks for.
- Send it to the same implementer with `SendMessage` (`Fix round <n>`). Spawn a fresh implementer, with the change name and the fix list, only if the old one is gone or its reports become incoherent.
- Handle its report as in step 2, then go back to step 3.
- Limit: after 3 fix rounds with blocking findings still open, stop. Show the open items and ask the user how to continue.

## 7. Finish

When no blocking finding is open, or the user chose "Stop here" (or the fix-round limit was hit):

1. **Container tests.** Reviewers only read code, so a container-backed test that never ran leaves the paths it covers unverified. If any implementer report lists skipped container-backed tests, probe the socket again. If one answers, send the implementer a fix round that runs them (cargo crate by crate and `bazel --bazelrc=.bazelrc.agents test //crates/... //plugins/...`, which runs test binaries in parallel); a failure is a blocking finding. If none answers, ask with `AskUserQuestion`: "Start the socket and retry (Recommended)" / "Finish untested". Never record `done` on "Finish untested": the status is `untested`, with the skipped tests listed.
2. **Bazel build.** Reviewers only read code. If the change touched any `Cargo.toml` or `Cargo.lock`, run `bazel --bazelrc=.bazelrc.agents build --lockfile_mode=error //crates/... //plugins/...` (see the `rust` skill). A failure goes back to the implementer as a blocking fix round.
3. **Tech debt.** Append every P2 entry to `docs/tech_debt.md` in its existing format:
   - the next free ID from the header, and update the "next free ID" line
   - an "Origin of TD-x to TD-y: code review of `<change>` (<date>)" line
   - a summary table row and a `### TD-n (<finding IDs>). <title>` entry: what is wrong, when it matters, how to fix it
   
   Show the user the entries before you write them.
   - **Deferred work.** Follow-ups that are not defects (out-of-scope items and non-goals of the change, features cut during the rounds) go to `product/iterations/roadmap.md`, not to `docs/tech_debt.md`: a backlog row (item, status, reference to the change) unless an iteration or the backlog already lists them. Show the rows to the user before you write them.
4. **Ledger.** Set the status to `done` only if no blocking finding is open and the container-backed tests ran (step 1). If the user stopped with blocking findings open, set `stopped` and list them; never record `done` in that case. List the TD IDs created.
5. **Commit** the ledger, the tech-debt and the roadmap changes: `wip(<change>): review log and tech debt` (the ledger status is `done`, `untested` or `stopped` as above).
6. **Report**:
   - status (`done`, `untested` with the skipped container tests, or `stopped` with the open blocking findings), tasks `n/m`, rounds run
   - findings fixed, deferred (with TD IDs), cosmetic, rejected
   - container tests skipped (from the implementer reports): lead the report with them if the status is `untested`
   - next steps: squash the `wip(<change>)` commits (`git reset --soft BASE` and commit again; ask before doing this), then `/opsx:archive`. Before rebasing onto `main`, check whether commits below BASE already landed on `main` under other SHAs (`git log --oneline $(git merge-base HEAD main)..BASE`). If so, move only the squashed commit: `git rebase --onto main BASE`. The ledger is archived with the change.

## Ledger template

```markdown
# Review log: <change>

- BASE: <sha>
- Store: <id or none>
- Status: in-progress | stopped | untested | done
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
