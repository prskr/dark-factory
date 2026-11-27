# Design

## Context

See proposal.md, "Why". Facts this design builds on:

- `docs/tech_debt.md` (858 lines) has 70 entries under `## Entries`. Each is a `### TD-<n> (<review IDs>). <title>` heading. Ten of them have a "Tracked in [#n]." line. 66 have a line with backticked paths, often followed by symbols in parentheses. The rest is prose: 50 have "**Matters when:**" and 69 have "**Fix:**". A few entries name no file but another location, for example TD-129 names a PenPot board. TD-101 uses brace syntax (`crates/df-db/src/{git_pushes,transcript,tasks,runs}/tests.rs`).
- The summary table repeats ID, severity (`low` 62, `medium` 8), area and title. The "Origin of TD-x" list gives the review and date for groups of IDs. The "next free ID" line says TD-132. `## Accepted decisions` holds about 6 bullets that are not debt.
- Hugo publishes only `docs/content` and the mounted crate docs (`docs/config/_default/module.yaml`), so `docs/tech-debt/` stays unpublished, like `docs/adr/`.
- `tools/xtask` is its own workspace with `toml` 1.1, `serde` and `anyhow`. Its checks (`check_file_length.rs`, `check_test_names.rs`) share `common::report` and `common::files_or_tracked`. `main.rs` dispatches on the argument slice. CI's "Repository checks" step and `lefthook.yml` call them.
- Only `.claude/skills/implement-change/SKILL.md` (step 7.4) writes entries. `code-review` and `standards-reviewer` do not know the register. `openspec/config.yaml` has `proposal` rules.

## Goals / Non-Goals

**Goals:**
- A tool can find an entry from a file path.
- Adding or deleting an entry touches only its own file, except for the ID counter.
- Planning and review see the entries for the files they touch.

**Non-Goals:** see proposal, "Out of Scope".

## Decisions

### Decision 1: one Markdown file per entry, TOML front matter

`docs/tech-debt/TD-<n>.md`. The file name is the ID, without padding, so `TD-5` in a text is `TD-5.md`. There is no `id` field, so the ID cannot disagree with the file name.

```markdown
+++
title = "Shutdown-while-disconnected test relies on a 100 ms sleep"
severity = "low"                      # low | medium | high
area = "factory-agent tests"
paths = ["crates/factory-agent/src/tests/connection.rs"]
locations = []                        # places that are not repo files, for example a PenPot board
issue = ""                            # "#122", or empty
origin = "code review of the iteration-1 agent runtime (R66), 2026-10-19"
+++

The test sleeps 100 ms so that the manager notices the dropped stream. …

**Matters when:** …

**Fix:** …
```

- `paths` holds repo-relative files or directories. A directory ends with `/` and matches every file below it. There are no globs and no braces: the migration expands TD-101's braces into four paths.
- Symbols stay in the body, so the front matter holds only what a tool reads.
- `origin` takes the review IDs from the old heading and the line of the "Origin of TD-x" list.
- The body needs "**Fix:**". "**Matters when:**" is recommended, but 20 entries do not have it today, so it is not checked.

Alternatives:
- Rejected: one TOML file. The prose would sit in `"""` strings that GitHub does not render, and every change would edit the same file.
- Rejected: keep the Markdown file with a strict files line. The parser is fragile, and the hand-kept summary table and the conflicts remain.
- Rejected: YAML front matter. The xtask has `toml` and no YAML crate, and `+++` is the Hugo convention the docs already use.

### Decision 2: the ID counter is a file

`docs/tech-debt/NEXT_ID` holds one number (132 after the migration). `cargo xtask tech-debt next-id` prints it and writes the number plus one. IDs are never reused, and a deleted entry leaves no file behind. So the counter cannot be computed from the files that exist.

When two branches add an entry at the same time, both change `NEXT_ID`, and git reports a conflict. That conflict is wanted: both took the same ID, and one must renumber.

- Rejected: computing the next ID from `git log` of deleted files. It needs the full history (shallow CI clones) and is harder to understand.

### Decision 3: the xtask subcommands

The subcommands live in `tools/xtask/src/tech_debt.rs`. All of them parse the same `Entry` struct (`serde`, `deny_unknown_fields`).

- `cargo xtask tech-debt check`. It fails, through `common::report`, for:
  - a file in `docs/tech-debt/` that is not `README.md`, `accepted.md`, `NEXT_ID` or `TD-<n>.md`;
  - front matter that does not parse or has an unknown field;
  - a severity outside `low | medium | high`;
  - empty `paths` and `locations`;
  - a path that does not exist (a directory path must end with `/`);
  - a body without `**Fix:**`;
  - `NEXT_ID` not above the highest ID.

  It runs in pre-commit on every commit, so that a rename in code is caught as well. Parsing 70 small files takes milliseconds.
- `cargo xtask tech-debt list` prints the summary table (ID, severity, area, title, issue), sorted by ID. It replaces the hand-kept table.
- `cargo xtask tech-debt for <path>…` and `cargo xtask tech-debt for --since <rev>` print one line per hit, `TD-<n> | <title> | <matching path>`. `--since` takes the changed files from `git diff --name-only <rev>...HEAD`. A path matches when it equals an entry path or lies below an entry directory. The order of the output is fixed (by ID).
- `cargo xtask tech-debt for --change <dir>`:
  - It reads every `.md` file below `<dir>` (an OpenSpec change).
  - It collects each backticked token that is an existing repo file, or an existing directory written with `/`. Tokens that do not exist are ignored: they are new files, symbols or commands.
  - It prints the matching entries whose ID (`TD-<n>`, as a whole word) appears in none of the change's files, and exits non-zero when there are any.

  Mentioning the ID is the whole test. Whether the mention is a fix or a reason to leave the entry is for the reviewer, not the tool.
- `cargo xtask tech-debt next-id` (Decision 2).

### Decision 4: where the register is read

- **Planning.** One sentence in the `context` of `openspec/config.yaml`: when an artifact names repo paths, run `cargo xtask tech-debt for --change openspec/changes/<name>`. Each reported entry then appears in the proposal, either under "What Changes" (the change fixes it, and a task deletes the entry) or under "Out of Scope" with the reason it stays. This is the decision point, because folding an entry in changes the tasks.
  - `context`, not a `proposal` rule: an update can bring in new paths through the design or the tasks alone, and `context` comes with the instructions of every artifact.
  - The prompt alone is not enough. `openspec-update-change` loads instructions only for a substantial rewrite or a new spec file. The skill is generated (`openspec update` overwrites it). The CLI 2.1.1 accepts `operations` only for `apply` and `archive`.
- **Gate before implementation.** `implement-change` preflight (a new step after "Confirm the scope") runs `cargo xtask tech-debt for --change openspec/changes/<name>`. When it fails, the skill stops, shows the entries and suggests `/openspec-update-change` with them. The skill is project-owned. The gate covers every way a change was written, including hand edits and updates after the proposal, so a requirement change that brings in new files is checked before any code is written.

  - Rejected: editing `openspec-update-change`. The next `openspec update` would silently remove the edit.
  - Rejected: a pre-commit check on `openspec/changes/**`. It would block committing a half-written plan, and a plan is allowed to be unfinished until implementation starts.
- **Review.** `code-review` step 3 runs `cargo xtask tech-debt for --since <fixed point>` and passes the output to the standards reviewer. A new reviewer rule says how to report:
  - a hunk that fixes what an entry describes, while the entry file stays: `hard`, because the register's rule is to delete it with the fix;
  - a hunk that changes an entry's files without fixing or mentioning the entry: `judgement`, `minor`;
  - an entry whose text no longer matches the code it names: `nit`.
- **Writing.** `implement-change` step 7.4 writes each P2 finding as `TD-<next-id>.md` with `origin = "code review of <change> (<finding IDs>), <date>"`, and runs `check`.

## Risks / Trade-offs

- [The migration loses or changes prose] → A temporary converter does the move (task 3.1), not a hand edit. Task 3.2 compares the bodies with the old sections, and the `list` output with the old summary table.
- [Branches in flight still edit `docs/tech_debt.md`, for example `test-waits-on-conditions` deletes TD-5, TD-50 and TD-101] → After the merge, the file is gone and their edit conflicts. The fix is to delete `docs/tech-debt/TD-<n>.md` instead. Task 4.3 updates `test-waits-on-conditions` if it is not merged yet. A change created later reads the README.
- [Hits from broad entries (a whole crate directory) make noise in review] → The reviewer reports a "touches without mention" hit only as `judgement`/`minor`. The migration keeps paths as narrow as the old entries named them.
- [Pre-commit fails on an unrelated rename] → Intended: the entry names a path that is gone. The fix is one line in the entry.

## Migration Plan

One change, in this order:
1. Tool and tests.
2. The converter run (the converter is removed again).
3. Old file deleted.
4. Rules and links updated.

Rollback: revert the change; the old file comes back with it.
