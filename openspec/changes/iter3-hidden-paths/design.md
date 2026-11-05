# Design

## Context

ADR-0054 decides that hidden paths use sparse checkout plus the push check, and that Landlock is not used for them. Today:

- **Push policy**:
  - `project_push_policies` holds `protected_globs TEXT[]` and `protected_mode`, validated by `protected_glob()` (`df-db/src/push_checks.rs`, `globset` with `literal_separator(true)`).
  - The gateway compiles them in `CheckPolicy::new` (`df-gateway/src/git/checks.rs`) and checks every `PathChange` of every added commit, but only for `Target::TaskBranch`.
  - WIP pushes get only the secret scan and the size limit.
- **Workspace** (`factory-agent/src/workspace.rs`):
  - `clone_workspace` runs `clone --filter=blob:none --no-checkout`, `rewrite_git_config`, `checkout -B`, then `restore --source=<wip> --worktree -- .`.
  - `sync_existing_workspace` rewrites the config, fetches, checks out and fast-forwards.
  - `rewrite_git_config` (`git.rs`) writes a fixed `[core]` section and deletes `.git/config.worktree`.
- **WIP tree**: `write_wip_tree` builds the tree in a fresh `GIT_INDEX_FILE=/run/factory/wip.index` with `GIT_LITERAL_PATHSPECS=1`:
  1. `read-tree HEAD`;
  2. `add -A`;
  3. secret files are reset or removed from the index;
  4. `write-tree`.
- **System prompt**: the persona's prompt, sent as `Assign.harness.system_prompt` by `build_assign` (`df-agent-channel/src/assign.rs`).

## Goals / Non-Goals

**Goals:**
- Tracked files matching a hidden glob are absent from `/data/repo` during every run, and their blobs aren't downloaded by a fresh clone.
- No push, whether to the task or the WIP branch, changes a hidden path.
- The runtime's own Git operations never record hidden paths as deleted.
- The agent knows which paths are hidden and why they are missing.

**Non-Goals:**
- Hiding content from the repository history (`git show`, lazy blob fetches through the Git proxy): backlog (`roadmap.md`).
- Landlock: `iter3-sandbox-landlock` (iteration 3).
- Instance-wide hidden paths: rejected until a use case shows up.

## Decisions

### D1: One glob syntax, one builder in `df-core`

Hidden globs use the protected-path syntax. The glob builder (`literal_separator(true)`) moves from `df-db` to `df-core`, so `df-db` (validation), `df-gateway` (check) and `factory-agent` (sparse patterns and removal) compile globs identically.

A hidden glob is additionally rejected if it contains `{` or `}`, because gitignore-style patterns have no alternation. The validation reuses the existing per-line error of the push-policy form.

Alternative: gitignore syntax for hidden paths. Rejected, because then protected and hidden paths on the same page would use two different syntaxes.

### D2: Translation to sparse patterns

`.git/info/sparse-checkout` is generated as:

```
/*
!/<glob 1>
/<glob 1>/
!/<glob 2>
```

Every glob is anchored with a leading `/`, because protected globs match the whole repo-relative path (`CODEOWNERS` means only the root file).

The two syntaxes differ on directories. A sparse pattern that matches a directory excludes its whole subtree, while a globset glob matches only the paths it names: `!/configs/*` would also hide `configs/nested/visible.txt`, which `configs/*` doesn't match. So every glob not ending in `/**` is followed by the re-include `/<glob>/`, which matches only directories and brings their subtrees back. Globs ending in `/**` cover the subtree in both syntaxes and need no re-include. Checked with Git 2.56: `/*`, `!/configs/*`, `/configs/*/` hides `configs/top.txt` and keeps `configs/nested/visible.txt`.

The runtime always enables non-cone mode in the rendered `.git/config` (`core.sparseCheckout = true`, `core.sparseCheckoutCone = false`), also for projects without hidden paths: the file is then just `/*`, a full checkout. Without that, removing the last hidden glob would need a separate step to bring the skipped files back; with it, the next `reapply` restores them. The runtime writes the file itself instead of calling `git sparse-checkout set`, because that command would write `config.worktree`, which `rewrite_git_config` deletes and `extensions.*` can't be passed through.

The file is rewritten on every `rewrite_git_config`, so on prepare and on finalize.

Alternative: cone mode. Rejected: it can only include directories, so hiding `talos/` would mean listing every sibling, and new top-level directories would be missing.

### D3: Clone and reuse

- **Fresh workspace:** the sparse file is written between `clone --no-checkout` and `checkout -B`. Checkout then fetches only blobs inside the sparse set. The WIP restore (`git restore --worktree`) respects the skip-worktree bits by default.
- **Reused workspace:** after the config rewrite, `git sparse-checkout reapply` updates the working tree. A path that is no longer hidden comes back. Git refuses to drop modified files, which D4 handles.

### D4: Removing files under newly hidden paths

After `reapply`, the runtime lists the matches with the D1 matcher:

- every tracked path that still exists on disk (`ls-files -z`), with or without the skip-worktree bit. `reapply` leaves modified files in place and clears their bit (status `H`), so filtering on `S` would miss exactly the modified files;
- every untracked path, ignored ones included (`ls-files --others -z`). An ignored file under a newly hidden path can hold content derived from the hidden files.

It deletes each match, resets the tracked ones in the index (`reset -q -- <paths>`, literal pathspecs) and runs `reapply` again, which sets their skip-worktree bit (checked with Git 2.56: status `S`, `git status` clean). It emits one `workspace.hidden_paths_removed` notice listing the paths (truncated after 50).

This runs during prepare only, before the harness starts, so no agent process exists that could swap path components during the removal. At finalize the harness is already stopped and the next prepare removes anything it restored. The runtime is not restricted by Landlock and the agent controlled the working tree in earlier runs. So before deleting, it checks every component below `/data/repo` with `symlink_metadata` and skips a path if any parent component is a symlink. The final component is removed with `remove_file`, which never follows a symlink.

Alternative: fail the run. Rejected: it would block the task until someone cleans up by hand (decision Q17 in the design session).

### D5: WIP tree leaves hidden paths alone

After `add -A`, `write_wip_tree` lists the staged changes (`diff --cached --name-only -z HEAD`) and filters them with the D1 matcher. It resets each match to HEAD, or removes it from the index if HEAD lacks it. This is the same literal-path handling the secret denylist already uses, so `GIT_LITERAL_PATHSPECS=1` stays.

This holds even if the agent cleared the skip-worktree bits in `.git/index`. The WIP push therefore can't fail on `df:hidden-path` because of the runtime.

Alternative: seed `wip.index` from a copy of `.git/index`. Rejected as the only measure, because the bits are agent-controlled.

### D6: Gateway check

`CheckPolicy` gets a second glob set for hidden globs. Every `PathChange` matching it becomes a `df:hidden-path` finding with mode `reject` and fingerprint `path:<path>`. The check runs for `Target::TaskBranch` and `Target::Wip`, before the protected-path check.

A path that is both protected and hidden yields only `df:hidden-path`, so no exception request is raised for it. The gateway policy proto gets `repeated string hidden_globs`.

### D7: Contract and delivery

- `darkfactory.agent.v1.Workspace` gets `repeated string hidden_paths` (globs as configured, next free field number).
- `build_assign` reads them from the project's push policy row; a project without a row has none.
- The runtime treats an invalid glob (only possible through a contract mismatch) as a prepare failure, not as a glob to skip.

### D8: System prompt section

When `hidden_paths` is non-empty, `build_assign` appends a fixed block after the persona prompt:

```
## Sandbox

These repository paths are hidden from your workspace on purpose: <globs>. Do not restore, check out or read them through Git (`git show`, `git checkout`, `git sparse-checkout`); pushes changing them are rejected.
```

`iter3-sandbox-landlock` extends the same block with the writable directories. Whichever change lands second merges into it.

## Risks / Trade-offs

- **Content stays reachable through Git history:** `git show HEAD:talos/cp.yaml` lazily fetches the blob through the Git proxy. The prompt section, the docs warning and the backlog item (proxy-side refusal) cover it.
- **Git version:** non-cone sparse checkout through `info/sparse-checkout` with `reapply` needs Git 2.27 or newer. The reference image ships 2.47; the docs name the minimum. Prepare fails with a clear error on older Git.
- **Translation edge cases:** apart from directories (D2), a glob can mean slightly different things in globset and gitignore, for example backslash escapes. The push check stays the enforcing layer, and a test covers the translation of the documented examples.

## Migration Plan

Additive. The migration adds `hidden_globs TEXT[] NOT NULL DEFAULT '{}'`, so existing projects get no hidden paths. Their workspaces switch to non-cone sparse checkout with the pattern `/*` on the next prepare, which keeps every file checked out.

## Open Questions

None.
