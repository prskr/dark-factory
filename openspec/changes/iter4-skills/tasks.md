# Tasks

> **Spikes and design first:** group 1 answers the open questions Q2–Q4 of `design.md`; write the results back into `design.md` and the spec deltas before group 5 starts. Group 2 (PenPot) must be approved before group 7. Groups 3 and 4 depend on neither.

## 1. Spikes

- [ ] 1.1 Claude Code through the ACP adapter, `HOME=/data/home`: does it list a skill from `~/.claude/skills/<name>/SKILL.md`; does a personal skill win over `<repo>/.claude/skills/<name>` of the same name; does the model load `dark-factory-basics` before its first action with the system prompt line of design D9. Verify: a short note with the commands and transcripts in `design.md` (Context), Q2 and Q4 answered for Claude Code.
- [ ] 1.2 OpenCode (`opencode acp`): the global skill directory below `HOME`, every repository directory it reads skills from (including Claude-compatible ones), and the precedence on a name collision. Verify: `design.md` D5 table and `specs/opencode-harness/spec.md` updated with the observed paths, Q2 answered for OpenCode.

## 2. Design agreement (PenPot)

- [ ] 2.1 Draw in PenPot, in light, dark and at 390 px width: the skills page (rows with "Built-in" label, empty state), the skill form modal (name, description, body, supporting file rows, size against the limit, version list), the read-only built-in view, the delete confirmation listing users, the "Skills" field of the persona form and the "Skills" tab of the project settings page. Verify: the user approves the boards, and every changed label is written back into `specs/skills/spec.md` and `specs/configuration-management/spec.md` before group 7 starts.

## 3. Skill model and storage

- [ ] 3.1 Write failing unit tests first for `Skill` in `df-core` (design D1): name rule, reserved `dark-factory-` prefix for instance skills, description length and single line, file path rules (relative, no `..`, at most 8 components, not `SKILL.md`), the 256 KiB / 50 file limits, and `SKILL.md` rendering (frontmatter with `name` and `description`, a description containing `:` or quotes stays valid YAML). Then implement. Verify: `cargo nextest run -p df-core skill`.
- [ ] 3.2 Write a failing test first that every embedded built-in skill passes the validation of 3.1 and that `dark-factory-basics` has scope `always`. Then add `crates/df-core/skills/dark-factory-basics/SKILL.md`, written from `product/agent-playbook.md` §1, with a description that asks to load it before every task, and the constant list of built-ins (design D3). Verify: `cargo nextest run -p df-core skill` and a review of the skill text against playbook §1.
- [ ] 3.3 Write failing migration and query tests first: create, save a new version, read the current version, list versions, delete (versions and assignments gone), set persona and project skills, and the skills of a run for a persona and a project (union, by name, design D4). Then add the migration and the `df-db` module `skills`, and regenerate `.sqlx/`. Verify: `cargo nextest run -p df-db skills` (container-backed).
- [ ] 3.4 Write failing tests first for the audit entries `skill.create`, `skill.update` (old and new content), `skill.delete`, and the skill list in `persona.update` and `project.update`. Implement. Verify: `cargo nextest run -p df-db skills catalog` (container-backed).

## 4. Harness profile and contract

- [ ] 4.1 Write a failing test first that the `claude-code` profile has `skills_dir` `.claude/skills` and `repository_skill_dirs` `[".claude/skills"]`, and the `opencode` profile the values from spike 1.2. Then add both fields to `HarnessProfile` (design D5). Verify: `cargo nextest run -p df-core harness`.
- [ ] 4.2 Add `skills_dir`, `repository_skill_dirs`, `Skill` and `SkillFile` to `AcpHarness` in `agent.proto` (design D6), with field comments. Verify: `cargo build --workspace` and `bazel --bazelrc=.bazelrc.agents build //contract/...`.

## 5. Skills in Assign

- [ ] 5.1 Write failing `df-agent-channel` assign tests first for the run-orchestration scenarios "Claude Code run", "Rendered SKILL.md" and "Chat run", and for skills-spec "Basics in every run" and "Edit during a run" (the version at run start). Then load the run's skills in `run_context.rs` and fill `AcpHarness` in `assign.rs`. Verify: `cargo nextest run -p df-agent-channel assign`.
- [ ] 5.2 Write failing tests first for "Pointer present" (acp-harness) and "Generic harness without skill support" (system prompt part, profile without `skills_dir`). Implement in `system_prompt` / `harness_files` (design D9). Verify: `cargo nextest run -p df-agent-channel assign`.
- [ ] 5.3 Write a failing test first for "Run over the limit": a run whose skills exceed 4 MiB fails before provisioning with a reason naming the limit. Implement where run start validates the persona's model today. Verify: `cargo nextest run -p df-orchestrator` (container-backed).

## 6. Runtime delivery

- [ ] 6.1 Write failing `factory-agent` tests first for the acp-harness scenarios "Skills written", "Removed skill on a reused home", "Skill planted by the agent", "Fresh home", "Symlinked skill directory" and "Path escapes the skill". Then implement the skill directory writer (design D7), reusing the no-follow helpers of the harness file writer, and the `skills.delivered` notice. Verify: `cargo nextest run -p factory-agent skills`.
- [ ] 6.2 Write failing tests first for "Repository skill with a factory name", "Other repository skill" (acp-harness) and "Claude-compatible repository skill" (opencode-harness), and for the `skills.unsupported` notice. Implement the shadow check after workspace prepare (design D8). Verify: `cargo nextest run -p factory-agent skills`.
- [ ] 6.3 Extend `fake-acp-agent` (if needed) so a scripted step can list files below `HOME`, for the e2e test of 7.4. Verify: `cargo nextest run -p fake-acp-agent`.

## 7. Web UI

- [ ] 7.1 Write failing `df-web` tests first for the skills page: list with built-ins labeled and the empty state, create with a supporting file, field errors (name rule, name taken, reserved prefix, path, size limit), name read-only on edit, version list, built-in read-only, delete confirmation naming personas and projects. Implement per the approved boards. Verify: `cargo nextest run -p df-web skills`.
- [ ] 7.2 Write failing tests first for "Assign a skill to a persona" and "Basics cannot be removed" (persona form). Implement. Verify: `cargo nextest run -p df-web config_pages`.
- [ ] 7.3 Write failing tests first for "Assign a skill to a project" (project settings "Skills" tab, tab in the URL like the other tabs). Implement. Verify: `cargo nextest run -p df-web project_settings`.
- [ ] 7.4 Seed one instance skill in the demo data and the e2e seed, and add an e2e test (skill `e2e`): create a skill in the UI, assign it to a persona, run a task with the fake ACP agent, and check that the `skills.delivered` notice in the transcript lists it next to `dark-factory-basics`. Verify: `just e2e-local`.

## 8. Docs and roadmap

- [ ] 8.1 Write the how-to `docs/content/using/skills.md` (create a skill, assign it to a persona or project, what a run gets, built-in skills, versions, precedence over repository skills and the shadow warning, the runtime-owned skill directory, limits). Update `docs/content/using/setup.md` (persona "Skills" field, project "Skills" tab), `docs/content/using/opencode.md` (skill directory), `docs/content/architecture/agent-runs.md` (skill delivery step), `docs/content/security/sandbox.md` (trusted factory skills, untrusted repository skills), and the notices `skills.delivered`, `skills.repository_skill_shadowed`, `skills.unsupported` in `crates/factory-agent/docs/configuration.md`. Verify: `just docs-build`.
- [ ] 8.2 Mark §1 of `product/agent-playbook.md` as the source of `dark-factory-basics` (edit both together), and update the F13 line of `product/iterations/roadmap.md` to reference the archived change. Verify: the diff is reviewed, and `openspec validate iter4-skills --strict` passes.
