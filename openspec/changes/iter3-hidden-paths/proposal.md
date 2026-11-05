# Proposal

## Why

Some repositories hold files an agent should not see while it works on the rest. An example is a homelab repository with Talos machine configs that contain certificate material. Today every tracked file lands in `/data/repo`, and the only path-based control is the push check, which stops changes but not reads. ADR-0054 decides that projects can hide paths from their agents through sparse checkout and the push check.

## What Changes

- **Hidden paths per project**:
  - A list of globs in the project's push policy, in the same syntax as protected paths (`globset`, `*` within one directory, `**` across directories).
  - Globs with `{a,b}` alternation are rejected on save, because sparse patterns can't express them.
  - Edited as a textarea on the push-policy page, and audited like the rest of the policy.
- **Workspace**:
  - `Assign.workspace` carries the hidden globs.
  - The runtime excludes them from the checkout with non-cone sparse patterns. On a fresh blobless clone this happens before the first checkout, so the hidden blobs are never downloaded.
  - The patterns are re-applied on every prepare and finalize, because the agent can edit `.git/info/sparse-checkout`.
- **Newly hidden paths**: when a reused workspace still holds files under a newly hidden path, uncommitted changes included, the runtime removes them and emits a `workspace.hidden_paths_removed` notice listing the paths.
- **WIP snapshot**: hidden paths are never recorded as deleted or changed in the WIP commit.
- **Git config**: the runtime-owned `.git/config` keeps sparse checkout enabled (non-cone).
- **Push check**:
  - Every push touching a hidden path is a `df:hidden-path` finding with mode `reject`. This applies to task-branch and WIP pushes alike, regardless of the project's protected-path mode.
  - No exception request is raised.
- **System prompt**: when a project has hidden paths, the agent's system prompt gets a fixed section listing them, saying they are left out on purpose and must not be restored through Git.
- **Docs**:
  - a how-to "Hide files from agents", with the Talos scenario and the warning that hidden content stays reachable through the repository history and that committed secrets belong encrypted (for example with SOPS);
  - push-policy and Git proxy reference updates;
  - glossary terms Hidden path and Protected path (written).

Out of scope:
- **Hiding content from the repository history** (the Git proxy refusing blob fetches for hidden paths): backlog (`roadmap.md`).
- **Landlock restrictions** for the runtime and system directories: `iter3-sandbox-landlock` (iteration 3).
- **Hidden paths from the repository config file** (`.dark-factory/config.toml`): rejected (ADR-0039, ADR-0054).
- **Instance-wide hidden paths**: rejected until a use case shows up; every project sets its own.

## Capabilities

### New Capabilities
- `hidden-paths`: the hidden-path list of a project, its validation and form, delivery to the runtime, exclusion from the workspace, removal of newly hidden files and the system prompt section.

### Modified Capabilities
- `push-checks` ("Per-project push policy", "WIP snapshots"): adds hidden globs to the policy and the `df:hidden-path` check, also on WIP pushes.
- `workspace-lifecycle` ("Fresh workspace", "Existing workspace", "Finalize"): sparse checkout on clone and reuse, and a WIP snapshot that leaves hidden paths alone.
- `git-hardening` ("Runtime-owned .git/config"): the generated config keeps non-cone sparse checkout enabled.
- `egress-policy-distribution` ("Push policies and exceptions in the snapshot"): the snapshot carries the hidden globs.

## Impact

- `contract/proto`:
  - `darkfactory.agent.v1.Workspace.hidden_paths`;
  - `darkfactory.gateway.v1` push policy `hidden_globs`;
  - finding code `df:hidden-path`.
- `crates/df-db`:
  - migration `project_push_policies.hidden_globs TEXT[] NOT NULL DEFAULT '{}'`;
  - read and write in `push_checks.rs` with the shared glob validation;
  - `.sqlx/`.
- `crates/df-web`: hidden-paths textarea on `config_pages/push_policy.rs`.
- `crates/df-agent-channel`: `build_assign` fills `Workspace.hidden_paths` and appends the system prompt section.
- `crates/df-gateway`: `CheckPolicy` gets a reject-mode glob set for hidden paths that also runs for `Target::Wip`.
- `crates/factory-agent`:
  - sparse patterns in `clone_workspace` and `sync_existing_workspace`;
  - `core.sparseCheckout` / `core.sparseCheckoutCone` in `render_git_config`;
  - the WIP index in `write_wip_tree`;
  - the removal notice.
- `crates/fake-acp-agent` and `crates/df-e2e`: a project with a hidden path; the agent can't read the file, and a push deleting it is rejected.
- Requires Git 2.27 or newer in project images (`sparse-checkout reapply`); the reference image ships 2.47.
