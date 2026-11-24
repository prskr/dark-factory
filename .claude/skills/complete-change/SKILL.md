---
name: complete-change
description: Complete an implemented OpenSpec change. Squash its WIP commits into one commit, archive the change, push the branch and open (or update) a GitHub pull request that summarizes the change. Run after implement-change has finished.
argument-hint: <change-name>
disable-model-invocation: true
---

Change: `$ARGUMENTS`

This skill rewrites local history and publishes the result. Every step that pushes or opens something on GitHub waits for the user's confirmation through `AskUserQuestion`. Never pass `--no-verify`: the pre-commit and pre-push hooks (`lefthook.yml`) are part of the checks.

## 0. Preflight

1. Resolve the store and the change as in `implement-change` step 0 (`openspec store list --json`, `openspec status --change "<name>" --json`, with `--store` if there is one). If no name was given, run `openspec list --json` and ask.
2. Branch: run `git status -sb`. Stop if HEAD is detached. Implementer and e2e runs can detach it, and pushing a branch name then pushes the stale branch ref. Stop if the working tree is not clean.
3. Ledger: read `openspec/changes/<name>/review-log.md` and take `BASE` and `Status` from it.
   - `done`: continue.
   - `untested` or `stopped`: show the open items and the skipped container tests from the ledger, and ask "Complete anyway" / "Stop". On "Complete anyway", the PR states the open items under **Not verified**.
   - No ledger: ask the user for BASE (the commit before the first WIP commit) and check it with `git log --oneline BASE..HEAD`.
4. Tasks: `openspec instructions apply --change "<name>" --json` must report `remaining: 0`. If not, show the open tasks and stop.
5. Commits: list them with `git log --oneline BASE..HEAD`. Every commit whose subject does not start with `wip(<name>)` is a separate change, for example a dependency bump the pre-push hook required. Ask whether to keep it as its own commit (the default) or fold it into the squash.
6. Main: run `git fetch origin main`. Check whether commits below BASE already landed on `main` under other SHAs: `git log --oneline $(git merge-base HEAD origin/main)..BASE`. Remember the result for step 3.

## 1. Squash

1. Write the commit message from the proposal, `design.md`, the ledger and `git diff --stat BASE..HEAD`:
   - Subject: a Conventional Commit, `<type>(<scope>): <summary>`, at most 72 characters. `feat` for new behavior, `fix` for a bug fix, `refactor` for neither. The scope is the main area of the change, for example `personas`, `sandbox` or `web`. See `git log --oneline origin/main` for the style.
   - Body: two to five lines on what changed and why, then `BREAKING CHANGE: …` lines for each breaking change in the proposal, then the attribution lines from the session's system reminder.
2. Show the message and ask with `AskUserQuestion`: "Squash with this message (Recommended)" / "Change the message (say what)" / "Stop".
3. Kept commits from step 0.5 must survive the squash. If there are any, squash only the WIP commits: rebase non-interactively with `git rebase --onto` per commit, or cherry-pick the kept commits after the squash commit. Otherwise:
   ```
   git reset --soft BASE
   git commit -F <message file in the scratchpad>
   ```
4. Check: `git diff <old HEAD> HEAD` must be empty (the tree is unchanged), and `git log --oneline BASE..HEAD` shows the squash commit plus the kept commits only.

## 2. Archive

1. Invoke the `openspec-archive-change` skill with the change name and store. Follow it. It moves the change, the ledger included, to `openspec/changes/archive/<date>-<name>/` and syncs the delta specs into `openspec/specs/`.
2. Run `openspec validate --all --strict` (with `--store`). A failure stops here.
3. Commit the archive on its own: `docs(openspec): archive <name>`, with the attribution lines.

## 3. Rebase (only if needed)

- If step 0.6 listed commits, or `origin/main` is not an ancestor of HEAD (`git merge-base --is-ancestor origin/main HEAD`), ask "Rebase onto origin/main (Recommended)" / "Push without rebase".
- On rebase: if commits below BASE landed on `main` under other SHAs, move only this change's commits with `git rebase --onto origin/main BASE`. Otherwise use `git rebase origin/main`. On conflicts, use the `resolving-merge-conflicts` skill. After the rebase, run `cargo build --workspace` and `openspec validate --all --strict`. If any `Cargo.toml` or `Cargo.lock` changed in the rebase, also run the Bazel build with `--lockfile_mode=error` (skill `rust`).

## 4. Push

1. Branch:
   - On `main`, or on a branch whose name does not fit the change, create a branch first: `git switch -c <type>/<name>`, where `<type>` is the commit type from step 1 (`feat/persona-identity`).
   - Run `git status -sb` again and confirm HEAD is on the branch.
2. Remote state: `git ls-remote --heads origin <branch>`.
   - No remote branch: `git push -u origin <branch>`.
   - The remote branch exists. The squash rewrote history, so the push needs `--force-with-lease=<branch>:<remote sha>`. Ask "Force-push with lease (Recommended)" / "Push to a new branch <branch>-2" / "Stop". Never use a plain `--force`.
3. The pre-push hook runs clippy and `cargo deny`. If it fails, stop and report. Don't bypass it. A finding from `cargo deny` in a dependency outside this change goes into its own commit (as in PR #316). Ask the user before adding one.

## 5. Pull request

1. Check whether a PR exists: `gh pr view <branch> --json number,url,state`.
2. Title: the squash commit subject.
3. Body: follow the format of the recent feature PRs (for example #316). Write it in the style of `~/.claude/CLAUDE.md`: technical and concise, without filler. Sources: the proposal, `design.md`, the archived ledger (`openspec/changes/archive/<date>-<name>/review-log.md`, Finish section) and `git diff --stat`.
   ```markdown
   <One or two sentences: what users or operators get. OpenSpec change `<name>` (archived in this PR).>

   ## Changes

   - **<Area>:** <what changed, from the user's or operator's view; implementation detail only where a reviewer needs it>
   - **Docs:** <pages added or changed>
   - **OpenSpec:** the change is archived, its deltas are synced into the `<capability>` specs, and the follow-ups are in the roadmap backlog.

   ## Breaking changes

   - <one bullet per BREAKING item in the proposal, with the upgrade step; omit the section if there is none>

   ## Testing

   - <checks and suites that passed, with counts from the ledger: cargo tests, clippy, machete, xtask checks, Bazel build and test, docs-build, e2e>

   **Not verified:**
   - <container tests skipped, known unrelated failures, open items from an `untested` or `stopped` ledger; omit if empty>

   <attribution line from the session's system reminder>
   ```
   Add `Closes #<n>` only when the proposal or the ledger names an issue.
4. Show the title and body, then ask "Open the PR (Recommended)" / "Change it (say what)" / "Stop". Write the body to a file in the scratchpad.
5. Create it: `gh pr create --base main --head <branch> --title "<title>" --body-file <file>`. If a PR already exists, update it instead: `gh pr edit <number> --title … --body-file …`.
6. Check: `gh pr view <branch> --json headRefOid,url` must match `git rev-parse HEAD`. If the SHAs differ, the push went wrong; report it, and do not force anything without asking.

## 6. Report

- The PR URL, the branch and the commits (`git log --oneline origin/main..HEAD`).
- What the PR lists as not verified.
- Next steps: wait for CI (`gh pr checks <number> --watch`), then merge after review.
