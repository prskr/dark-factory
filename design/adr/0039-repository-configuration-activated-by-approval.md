# ADR-0039: Repository configuration file, activated by approval

- **Status:** Accepted (future option F11)
- **Date:** 2026-10-19
- **Related:** [ADR-0011](0011-authentication-authorization-personas.md), [ADR-0015](0015-webhook-filtering-and-routing.md), [ADR-0020](0020-configuration-in-database.md), [ADR-0022](0022-project-images-runtime-volume-no-warm-pools.md), [ADR-0029](0029-egress-gateway-and-generated-network-policies.md), [ADR-0034](0034-task-hierarchy-subtask-branches-and-merging.md), [ADR-0038](0038-agent-questions-and-approval-requests.md)

## Context

Project settings that shape the sandbox — commit hooks, lifecycle hooks (F9), sidecars (F10), the project image, egress hosts, Git config entries — live in the database and are edited in the UI (ADR-0020). Repository maintainers want them versioned and reviewed together with the code they belong to.

The repository is not a trust anchor Dark Factory controls: agents write to it (task branches), the v1 forge token in the sandbox isn't limited to one branch, and branch protection, auto-merge and other bots on the default branch are forge settings Dark Factory can neither enforce nor verify. Reading configuration from "the protected default branch" alone would let anything that reaches that branch change the environment agents run in.

## Decision

- **One file:** `.dark-factory/config.toml`. No other names or formats. The file carries `version = 1`; unknown fields are rejected; the size is capped; symlinks, LFS pointers and submodule entries at the path are rejected.
- **Read path:** the control plane reads the file through the forge plugin at the project's base branch as stored in the database — never from the worktree or PVC, never from a task or parent branch, never from the forge's default-branch setting.
- **Activation by approval:** the database stores the approved version (content and commit SHA). A new version becomes active only after a human approves it.
  - The control plane checks for a new version when it provisions a run, and on push events for the base branch (ADR-0015).
  - A changed, valid version opens a `project-config` approval request on the project (new type, ADR-0038), showing the diff of the effective settings rather than raw TOML. A newer version replaces a pending request. An invalid version opens no request; the validation error is shown on the project.
  - Runs never wait for the approval: they use the last approved version, and the run record stores which version that was. No approved version yet → database settings only. Forge unreachable → last approved version.
  - Deciding needs `approval:decide`; only humans decide. Once approved, the content is trusted — entries such as egress hosts need no further approval.
- **Contents (closed list):**
  - commit hook commands (`design/agent-run.md` §7.1) and lifecycle hooks (F9);
  - non-secret environment variables;
  - Git config entries, validated against the same denylist as entries made in the UI;
  - project image and sidecars (F10), checked against the image allowlist (below);
  - egress `http-host` entries;
  - cluster targets, by name only, from targets an admin defined (ADR-0029).
- **Image allowlist** (instance setting): glob patterns over image references, e.g. `harbor.example.com/**` (registry), `harbor.example.com/dev/*` (path), `harbor.example.com/dev/rust:1.99` (single image). It applies to every project and sidecar image, whether it comes from the repository or the UI: UI entries are checked when saved, repository versions when validated, and every image again at run provisioning, so a tightened allowlist takes effect on the next run. An **empty allowlist allows every image** (implicitly open).
- **Never from the repository** (UI or declarative import F4 only): credentials and Secret references, runtime class, security context, service account, node placement, resources, PVC size, budgets, branch rewrite policy, subtask settings, push checks, MCP servers and tool grants, persona settings and permissions, raw pod spec fragments.
- **Layering:** instance (database) → project (database) → approved repository config → task. List entries (hooks, egress hosts, Git config entries) add up; for single values (image, an environment variable, a Git config key) the later layer wins; runtime-owned Git keys stay non-overridable. Hook commands run in the same order: instance, project, repository. Repository lifecycle hooks default to the `warn` failure policy. The UI shows the effective configuration with the source of every value.
- **Execution:** everything the file triggers runs inside the sandbox as the harness user ("teardown" is the `run-end` hook). Nothing from the repository runs in the control plane or the gateway.
- **Per-task setting `repo_config`:** `approved` (default), `ignore` (database settings only) or `ref` (testing). Resolved like the settings of ADR-0034: the task's own value, else the nearest ancestor's, else `approved`; subtasks inherit it. Humans set it at task creation without an extra permission; agents creating subtasks may only set `ignore`.
  - `ref`: the human picks a branch or commit; the control plane resolves it to a commit SHA at task creation and pins it, so later pushes to that branch (including by the agent) don't change it. The creation form shows the effective diff against the approved version; creating the task counts as approval for that task's subtree and is recorded in the audit log.
- **Self-protection:** `.dark-factory/**` is always a protected path in the Git proxy's push checks (F1); a project can't remove it.
- **Validation:** `dark-factory config validate <file>` checks a file offline, so CI can reject broken configs before they are merged.

## Consequences

- Maintainers change hooks, image and egress hosts through a reviewed PR plus one approval in Dark Factory; forge branch protection becomes defense in depth instead of the guarantee.
- A broken or compromised default branch can't change running projects without a human seeing the effective diff.
- Configuration comes from the base branch, code from the task branch: a hook calling a command an older task branch doesn't have yet fails — hence `warn` as the default, and `ref` to test a change before it's merged.
- ADR-0020 is amended: the database stays authoritative; the approved repository version is stored there as one more layer.
- An empty image allowlist leaves images unrestricted; instances that take images from repositories should set one. Digest pinning is left for when the declarative import (F4) exists and a bot like renovate can update the allowlist.

## Alternatives considered

- **Trust forge branch protection alone:** not enforceable or verifiable by Dark Factory; in v1 the sandbox's forge token can push to the default branch if the forge allows it.
- **Read from the task branch:** the agent would configure its own environment.
- **Several file names and formats (`.dark-factory.{toml,yaml}`):** ambiguous precedence a reviewer can miss; YAML adds anchors and implicit typing.
- **Block runs until a pending version is approved:** one config change would stall every task of the project.
- **Ceilings or extra approvals for egress hosts from the repository:** redundant with the activation approval for now; can be added later.
- **A permission for `ignore`:** the security-relevant parts of the sandbox aren't configurable from the repository, so opting out only removes additions.
