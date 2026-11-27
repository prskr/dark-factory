# Tasks

Write each test first and watch it fail (TDD). Run the xtask tests with `cargo test --manifest-path tools/xtask/Cargo.toml` and lint with `cargo clippy --manifest-path tools/xtask/Cargo.toml --all-targets -- -D warnings`. Follow the existing checks (`check_file_length.rs`: `run(root, …)`, `common::report`, tests on a `tempfile` tree).

## 1. Format, parser and `check`

- [ ] 1.1 Write unit tests in a new `tools/xtask/src/tech_debt.rs`, on a `tempfile` repo tree. Valid cases: an entry with front matter and a body passes, and `paths` with a file and a directory (`crates/x/`) passes. Each failure of design, Decision 3 (`check`) fails with the file name in the message: an unknown field, an unknown severity, empty `paths` and `locations`, a missing path, a directory path without `/`, a body without `**Fix:**`, a stray file, a file name with a leading zero (`TD-05.md`, scenario "Leading zero in the file name"), `NEXT_ID` at or below the highest ID (scenario "Counter not above every ID"). A renamed file that an entry lists covers the scenario "Entry names a removed file". Then implement the `Entry` struct (`serde`, `deny_unknown_fields`), front matter splitting at `+++` and `check`, and the dispatch `["tech-debt", "check"]` in `main.rs`. Verify: the tests pass.
- [ ] 1.2 Write `docs/tech-debt/README.md`. It covers:
  - the format (design, Decision 1, with the example);
  - the rules: add with `next-id`, delete the file in the change that fixes it, IDs are never reused;
  - the commands.

  Verify: the example in the README passes `check`, because a test reads it from the README.

## 2. `list`, `for`, `next-id`

- [ ] 2.1 Write tests:
  - `list` prints the rows sorted by ID.
  - `for crates/df-db/src/runs/tests.rs` finds an entry with `paths = ["crates/df-db/src/"]` and one with the exact file, and does not find an entry with `crates/df-db/src/run` (no prefix match inside a name).
  - `for` without a hit prints nothing and succeeds.
  - With `NEXT_ID` 7, `next-id` prints 7 and leaves `NEXT_ID` at 8.

  Then implement them and the dispatch: `for --since <rev>` through `git diff --name-only <rev>...HEAD`, and `for <path>…`. Verify: the tests pass. `for --since HEAD~1` on a scratch commit in a `tempfile` git repo lists the entry of the changed file.
- [ ] 2.2 Write tests for `for --change <dir>` on a `tempfile` tree with a change directory:
  - A backticked existing file that matches an entry, without the ID in any of the change's files: the entry is printed, and the run fails.
  - The same file written with the crate name only (`factory-agent/src/tests/connection.rs` for `crates/factory-agent/…`, and a `plugins/` case): it resolves and matches.
  - The same file with a line reference (`factory-agent/src/tests/connection.rs:786` and `…:786-790`): the suffix is stripped, and it matches.
  - The same, with `TD-<n>` in the proposal: the run succeeds.
  - `TD-<n>0` does not count as a mention of `TD-<n>`.
  - A backticked new file in an existing directory (`crates/x/src/new.rs`) matches an entry with `paths = ["crates/x/src/"]`.
  - A backticked path whose directory does not exist, a symbol (`Session::create_chat`) and a command (`cargo xtask tech-debt check`) are ignored.
  - A backticked directory (`crates/x/`) matches an entry for a file below it.

  Then implement it (design, Decision 3). Verify: the tests pass. On the real tree, after group 3, check against a change that is known to hit. `cargo xtask tech-debt for --change openspec/changes/test-waits-on-conditions` (if it is not merged yet) succeeds, because it names TD-5, TD-50 and TD-101. With the three IDs removed from a scratch copy of that change, the run fails and lists at least TD-5 and TD-101, so the gate does find the paths.

## 3. Migration

- [ ] 3.1 Write a temporary converter (a `#[test] #[ignore]` function or a throwaway subcommand in `tech_debt.rs`). It reads `docs/tech_debt.md` and writes `docs/tech-debt/TD-<n>.md`:
  - `title`: the heading without the review IDs;
  - `severity` and `area`: from the summary row;
  - `paths`: the backticked paths of the files line, braces expanded, directories with `/`;
  - `locations`: for entries without a repo path;
  - `issue`: the "Tracked in" line;
  - `origin`: the review IDs from the heading, plus the "Origin of TD-x" line that covers the ID. That line is either in the list at the top or a paragraph directly before the heading (design, Context);
  - the body: the rest of the section, unchanged, including the symbol notes of the files line. An inline "Origin of" paragraph is not part of any body.

  Before running `check`, list the entries whose body has no `**Fix:**` line (TD-7 on `main` today). Show the user the proposed marker for each, usually on the paragraph that already says what to do, and apply it once approved.

  Every `### TD-<n>` heading becomes an entry, also the ones below `## Accepted decisions`. `docs/tech-debt/accepted.md` gets only the section's intro and its `- **…**` bullets. Write `NEXT_ID` from the "next free ID" line of the old file (TD-134 on `main` today). Recount first: entries are added and deleted while this change waits. Verify: `cargo xtask tech-debt check` passes, the number of entry files equals `grep -c '^### TD-' docs/tech_debt.md`, and `accepted.md` has as many bullets as the old section and no `### TD-` heading.
- [ ] 3.2 Compare old and new. Every old section's prose appears in its new body (a test or a script that diffs them, normalizing only the removed heading and files lines and the approved `**Fix:**` markers of task 3.1). `cargo xtask tech-debt list` has the same IDs, severities and areas as the old summary table. If the table and the headings disagree (an entry without a row, or a row without an entry), list the differences for the user. Fix by hand any path that `check` rejects: a path that no longer exists means the entry is stale, so ask the user whether to delete it or update it. Verify: the comparison passes, and the user has decided each stale entry.
- [ ] 3.3 Delete `docs/tech_debt.md` and the converter. Verify: `grep -rn "tech_debt.md" --include=*.md --include=*.yaml --include=*.rs . | grep -v -E "openspec/changes/archive|openspec/changes/tech-debt-register/|review_findings.md"` shows only the places that group 4 changes. This change's own artifacts name the old file on purpose and are excluded.

## 4. Checks in hooks and CI, rules, links

- [ ] 4.1 Add `tech-debt-check: run: build/with_tools.sh cargo xtask tech-debt check` to the pre-commit section of `lefthook.yml`, without a glob. Add `cargo xtask tech-debt check` to "Repository checks" in `.github/workflows/ci.yaml`. Verify: `lefthook run pre-commit` passes. Renaming a file that an entry names makes it fail with the entry's name.
- [ ] 4.2 Update the agent rules:
  - `openspec/config.yaml`: one sentence in `context` (design, Decision 4, Planning): paths from the repo root or with the crate name, and the `for --change` run.
  - `.claude/skills/implement-change/SKILL.md` preflight: a new step after "Confirm the scope" that runs `cargo xtask tech-debt for --change openspec/changes/<name>`. On failure it stops, lists the entries and suggests `/openspec-update-change` (design, Decision 4, Gate).
  - `.claude/skills/implement-change/SKILL.md` step 7.4: a file per P2 entry with `next-id`, then `check`. Remove the "next free ID", "Origin of" and summary row steps.
  - `.claude/skills/code-review/SKILL.md` step 3: run `cargo xtask tech-debt for --since <fixed point>`. Step 4: pass the output to the Standards sub-agent.
  - `.claude/agents/standards-reviewer.md`: the input, and the three report rules of Decision 4.

  Do not edit the generated OpenSpec skills (`.agents/skills/openspec-*`). Verify by reading each against design, Decision 4 (skill `writing-for-agents`). Run `cargo xtask tech-debt for --change` on every open change in `openspec/changes/`, and list the changes that fail in the PR description, so that their owners know before the gate stops them.
- [ ] 4.3 Update the links to the old file:
  - `docs/remote-execution.md` (TD-27 links `tech-debt/TD-27.md`);
  - `product/iterations/roadmap.md` (two rows link `docs/tech-debt/`);
  - `product/iterations/iteration-3.md` (TD-91, TD-28, TD-31, TD-32 as `docs/tech-debt/TD-<n>.md`, or "fixed" if the entry is gone).

  If `test-waits-on-conditions` is not merged yet, change its "delete TD-n from `docs/tech_debt.md`" steps (tasks 1.3, 4.1, 4.2) to "delete `docs/tech-debt/TD-<n>.md`". Verify: `just docs-build` passes, and the grep of task 3.3 is empty.

## 5. Record the DF-46 sweep

- [ ] 5.1 Add entries with `next-id` for items 6–10 of the DF-46 artifact `cleanup-patterns.md`:
  - cast opt-outs for display values (`df-web/src/egress/log/rows.rs`, `df-web/src/artifacts/view.rs`);
  - the wit-bindgen opt-out on the whole plugin crate (`plugins/{forgejo,github,test-probe}/src/lib.rs`);
  - the icons follow-up in the lint reason (`df-web/src/components/icons.rs`);
  - the hand-kept `excluded_tests` (`deploy/e2e/e2e-local.sh`);
  - tests that run only in a manual environment (`df-k8s`, the live tests of `df-plugins`).

  Re-check each against the code first and leave out any that is gone. Show the entries to the user before writing them. Verify: `check` passes, and `cargo xtask tech-debt for crates/df-web/src/components/icons.rs` lists the icons entry.

## 6. Final checks

- [ ] 6.1 In `docs/links_test.sh`, the check "Contributor docs … are not published" lists the names `development tech_debt adr design-system releasing`. Replace `tech_debt` with `tech-debt`. Verify: `build/with_tools.sh docs/links_test.sh` passes. With a temporary `docs/content/tech-debt/x.md`, it fails. Remove the file.
- [ ] 6.2 Run `cargo test` and `cargo clippy` for `tools/xtask` (see the header), `cargo xtask tech-debt check`, `just docs-build`, `build/with_tools.sh docs/links_test.sh` and `openspec validate --all --strict`. Verify: all pass.
- [ ] 6.3 Published docs: none. The register is contributor documentation. `docs-site` keeps it unpublished, which `docs/links_test.sh` confirms (tasks 6.1, 6.2). `just docs-build` alone does not check this.
