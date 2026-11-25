---
name: prepare-change
description: Prepare an OpenSpec change for implementation. Find the UI views it adds or changes, design them in PenPot with the user, and record the approved boards in the change's design-boards.md, which implement-change and its reviewers read. Run before implement-change.
argument-hint: <change-name>
disable-model-invocation: true
---

Change: `$ARGUMENTS`

The design happens here, in its own session, so the `implement-change` orchestrator starts with a clean context. The implementer cannot ask questions and has no judgement about the look; this skill settles the look with the user before any code is written.

## 0. Preflight

1. Resolve the store and the change as in `implement-change` step 0 (`openspec store list --json`, `openspec status --change "<name>" --json`, with `--store` if there is one). If no name was given, run `openspec list --json` and ask.
2. Run `git status -sb`. The working tree must be clean. On `main`, create the change branch: `git switch -c feat/<name>` (`fix/<name>` for a bug fix).
3. If `openspec/changes/<name>/design-boards.md` exists with status `partial`, this is a resumed run: keep the boards it lists, and continue with the views it doesn't list yet. A board in PenPot whose name matches a planned board but is missing from the file is a leftover of an unapproved group: show it for approval in step 3 instead of cloning it again, and delete it only if the user rejects it. With any other status the change is already prepared: report that and stop, unless the user asks to revise it.

## 1. Find the views

Read the proposal, `design.md`, the specs and `tasks.md`. List every view the change adds or changes that a user sees: pages, dialogs, list rows, cards, labels, hints, error texts, empty states, and their phone layout (390 px). Sources:

- spec requirements and scenarios that name UI elements, labels, routes or ordering (`task-ui`, `chat-ui`, `configuration-management`, `artifact-ui`, `request-inbox`, …);
- tasks in `crates/df-web` or naming PenPot, a board or a mockup;
- the proposal's Impact list ("Web").

For each view, name the nearest existing board (`docs/design-workflow.md`, "Mapping views to boards") and the states the scenarios need.

Done when every UI-affecting requirement and task maps to a view, or is listed as "no visual change" with a one-line reason (for example: the server validates, and the form shows an existing error style with new text).

If nothing maps to a view, write `design-boards.md` with status `none needed` (format below), tick the design tasks in `tasks.md` (a task asking whether a design is needed is answered by this), and go to step 5.

## 2. Confirm the list

Load skill `penpot` and measure each target page ("Before the first write", step 2). Show the user the views, the boards you plan (one line each: board name, derived from, states) and the shape count of each target page against the page budget. Ask with `AskUserQuestion`: "Design these views (Recommended)" / "Adjust the list (say what)" / "Stop". Where a page would exceed the budget, ask for a new page in the same question.

## 3. Design

Follow skill `penpot` for every board: clone, change, bind tokens, export light and dark, check overlaps. Show each finished group of boards (the exported images plus a short list of what changed) and ask with `AskUserQuestion`: "Approve" / "Change it (say what)" / "Stop". Iterate until approved. Don't go on without approval. After each approved group, add its rows to `design-boards.md` with status `partial (<date>)` (format in step 4) and commit it as `wip(<name>): design (partial)`, so a stop keeps the approved boards and leaves a clean tree.

If a board shows that `design.md` or a spec is wrong beyond its labels (a missing state, a different flow), suggest `/openspec-update-change` with the finding and stop.

## 4. Record

1. Write back into the spec scenarios every label, hint, error text and ordering the approved boards show. The implementer and the spec reviewer test against the specs, not against PenPot.
2. Write `openspec/changes/<name>/design-boards.md`:
   ```markdown
   # Design boards: <name>

   - Status: approved (<date>) | partial (<date>) | none needed (<reason>)
   - Tokens added: <names and sets, or none>
   - Components added or changed: <names, or none>

   ## Boards

   | Board (page / name) | Route | Data | Viewport |
   |---|---|---|---|
   | <rows in the format of docs/design-workflow.md, "Mapping views to boards"> |

   ## Decisions the specs don't carry

   - <one line per layout decision the implementer needs: placement, order, sizes, colour values of new tokens (light and dark), states that look alike>
   ```
3. Tick the design tasks in `tasks.md`.
4. Run `openspec validate <name> --strict` (with `--store`).

## 5. Commit and report

1. Commit `design-boards.md`, the spec edits and `tasks.md` as `wip(<name>): design`, with the attribution lines. `implement-change` sets its BASE before the first `wip(<name>)` commit, so `complete-change` squashes these commits with the implementation. `implement-change` refuses a record with status `partial`.
2. Report: the boards per page, each touched page's shape count, the tokens added, and the next step `/implement-change <name>`.
