# Proposal: iter4-skills

## Why

Agents get their working rules only from the persona's system prompt and the repository's `AGENTS.md`. The rules of [`product/agent-playbook.md`](../../../product/agent-playbook.md) (one task one branch, rejected pushes, asking and requesting, finishing) reach no agent today. Each operator would have to copy them into every persona prompt. Harnesses already have a skill concept: Markdown instructions in a skill directory that the model loads when they apply. ACP has none (iteration 4, decision 4). So the factory must write skills into the harness's skill directory itself, on every run (F13).

## What Changes

- **Skills as factory data.** A skill has a name, a description, a body (the `SKILL.md` text) and optional supporting text files. Every save makes a new version. Skills are database rows (iteration 4, decision 11), with size limits.
- **Built-in skills** ship with Dark Factory and are read-only. The first one is `dark-factory-basics`, written from §1 of the agent playbook. Every run gets it. Its content is versioned with the release, so later changes (for example agent memory, ADR-0040) can add to it.
- **Instance skills** are created, edited and deleted by board members on a new "Skills" page in the settings area. Changes are audited.
- **Assignment** of instance skills per persona (persona form) and per project (project settings). A run gets the built-in skills, the persona's skills and the project's skills (union).
- **Delivery on every run.** `Assign` carries the run's skills and the harness's skill directory. The runtime replaces the skill directory under `HOME` before the harness starts. This works on a reused home and on a fresh one (planned `emptyDir` chats).
- **Skill paths are harness profile data.** `HarnessProfile` gets the skill directory (relative to `HOME`) and the repository skill directories (relative to the workspace). Claude Code: `.claude/skills` and `.claude/skills`. OpenCode: values from the spike in task 1. The harness registry and "Generic ACP" (F12) set the same two values as settings. No code per harness.
- **Factory skills win over repository skills** with the same name (iteration 4, decision 12). Human-edited skills are trusted; skills from the repository are untrusted like any repository content. Repository skills with other names stay usable. A shadowed repository skill gives a `skills.repository_skill_shadowed` warning in the transcript.
- **Pointer in the system prompt.** When a run has skills, the system prompt tells the agent to load `dark-factory-basics` before its first action, because harnesses load skills on demand only.

Out of scope:
- Agents changing skills or assignments as an instance operation (`instance-operation` approval): roadmap iteration 4, "Full permission model (ADR-0011)". It needs the permission modes first.
- What agents put into memory and how: roadmap iteration 6, agent memory (ADR-0040). That change adds to `dark-factory-basics` or adds a dedicated skill.
- Per-persona tool permissions (allowed, ask, denied): roadmap iteration 4, "Full permission model (ADR-0011)", separate change.
- The `admin` role for skill management: roadmap iteration 4, "Full permission model (ADR-0011)". Until then board members manage skills, like personas.
- Pattern skills from playbook §2 (for example `dependency-updates`) as built-ins: roadmap backlog (row added at archive). This change provides the mechanism and `dark-factory-basics` only.
- Import of skills from a Git repository, an archive upload or a marketplace: roadmap backlog (row added at archive).
- Pinning a persona or project to an old skill version, and rollback from the UI: roadmap backlog (row added at archive).
- A project removing a persona's skill (assignment exclusions): roadmap backlog (row added at archive).
- Hiding or filtering repository skills that do not collide with a factory skill: roadmap backlog (row added at archive).
- Binary supporting files (images, archives): rejected. Skills are text that reaches the model's context.

## Capabilities

### New Capabilities
- `skills`: what a skill is, validation and size limits, versions, built-in skills, the skills page, assignment per persona and project, which skills a run gets, audit.

### Modified Capabilities
- `acp-harness`: the runtime replaces the harness's skill directory before the harness starts, warns about shadowed repository skills, and the system prompt points to `dark-factory-basics`.
- `opencode-harness`: OpenCode's skill directory and repository skill directories.
- `run-orchestration`: `Assign` carries the run's skills and the harness's skill paths, also for chat runs.
- `configuration-management`: the persona form and the project settings page get a skill selection.

## Impact

- **Contract** (`contract/proto/darkfactory/agent/v1/agent.proto`): `AcpHarness` gets `skills_dir`, `repository_skill_dirs` and `repeated Skill skills` (`Skill`: name, version, files with path, content and executable flag). Additive.
- **Core** (`df-core`): `Skill` domain type with validation, `SKILL.md` rendering, built-in skills embedded from `crates/df-core/skills/`, `HarnessProfile.skills_dir` and `repository_skill_dirs`.
- **Database** (`df-db`): new module `skills` with tables `skills`, `skill_versions`, `persona_skills`, `project_skills`; migration and `.sqlx` metadata.
- **Agent channel** (`df-agent-channel`, `assign.rs`): loads the run's skills and fills `AcpHarness`; adds the skill line to the system prompt.
- **factory-agent**: writes the skill directory (no symlink following, stale entries removed), shadow check against the workspace, notices `skills.delivered` and `skills.repository_skill_shadowed`.
- **Web** (`df-web`): skills page (list, form modal, read-only built-ins, version list), skill selection on the persona form and on the project settings page.
- **Seeds / e2e** (`df-e2e`, demo data): a seeded instance skill; an e2e test for delivery.
- **Docs**: new how-to `docs/content/using/skills.md`, `docs/content/using/setup.md`, `docs/content/using/opencode.md`, `crates/factory-agent/docs/` (notices), `product/agent-playbook.md` (§1 is now the source of `dark-factory-basics`), roadmap F13 line.
- **Depends on** nothing unmerged. F12 (`iter4-harness-registry`) moves the two profile fields into the registry; `iter4-permissions` later maps skill management to `admin`.
