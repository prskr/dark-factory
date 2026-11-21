# Design: iter4-skills

## Context

- A harness profile (`crates/df-core/src/harness.rs`, from `iter3-opencode-harness`) is a constant per harness: wire APIs, environment, `system_prompt_path`, `settings_path`, configuration document. A new harness adds a profile; F12 (`iter4-harness-registry`) moves profiles into the instance configuration.
- `Assign.harness.acp.files` (`HarnessFile`: path relative to `HOME`, content) carries the system prompt and Claude Code's settings. The runtime writes them below `/data/home` before every start, overwriting, creating directories, without following symlinks (`acp-harness`, "Harness files"). It writes no other harness file.
- `assign.rs` (`df-agent-channel`) builds the files (`harness_files`) and the system prompt (`system_prompt`: persona prompt plus the fixed "Sandbox" and hidden-path sections).
- Personas and projects are rows in `df-db` (`catalog.rs`, `projects.rs`), edited by board members in the settings area, audited (`configuration-management`).
- `Assign` is limited by `MAX_MESSAGE_BYTES` (16 MiB) on the agent channel.
- Today `HOME` is on the task volume and survives between runs. Chats move to `emptyDir` (separate change), so nothing may rely on files left by an earlier run.
- Claude Code reads personal skills from `~/.claude/skills/<name>/SKILL.md` and project skills from `<repo>/.claude/skills/<name>/SKILL.md`. A skill is loaded on demand: only name and description sit in the context until the model decides to use the skill.
- Iteration 4 decisions taken: 4 (skills are files the runtime writes on every run), 11 (database rows with versions and a size limit), 12 (factory skills win, warning for a shadowed repository skill).

## Goals / Non-Goals

**Goals:**
- Every run gets `dark-factory-basics`, plus the skills of its persona and project, on a reused or a fresh `HOME`.
- Adding a harness (F12, Generic ACP) needs only two settings for skills, no code.
- Skill text written by humans is trusted; the repository cannot replace it.

**Non-Goals:**
- Agent instance operations on skills → roadmap iteration 4, "Full permission model (ADR-0011)".
- Memory instructions → roadmap iteration 6, agent memory (ADR-0040).
- Per-persona tool permissions → roadmap iteration 4, "Full permission model (ADR-0011)", separate change.
- Pattern skills from playbook §2 as built-ins, import from Git or archives, version pinning and rollback, project exclusions of persona skills, filtering of non-colliding repository skills → roadmap backlog (rows added at archive).
- Binary supporting files → rejected.
- Skills in full-harness mode (built-in loop) → rejected for this change: the built-in loop has no skill concept; the `full-harness-mode` change decides how it reads skills.

## Decisions

### D1: A skill is name, description, body and text files

Fields:
- `name`: 1–64 characters `[a-z0-9-]`, no leading, trailing or double hyphen (the Agent Skills naming rule that Claude Code and OpenCode share). Unique across built-in and instance skills. Not editable after creation, because the name is the directory name and other text refers to it.
- `description`: 1–1024 characters, one line. The harness shows it to the model to decide when to load the skill.
- `body`: Markdown, the `SKILL.md` text without frontmatter.
- `files`: supporting files, each a relative path (no `..`, no absolute path, not `SKILL.md`, at most 8 components), UTF-8 content and an `executable` flag.

Dark Factory renders `SKILL.md` as YAML frontmatter (`name`, `description`) followed by the body. The user never writes frontmatter, so it is always valid for every harness.

Limits: one skill at most 256 KiB (body plus files) and 50 files; all skills of a run at most 4 MiB. The run limit keeps `Assign` far below `MAX_MESSAGE_BYTES`. When the limit is exceeded at run start, the run fails before provisioning with a reason naming the limit (instance configuration that the board must fix, like a missing model).

Alternative: store the user's `SKILL.md` verbatim. Rejected: users would have to write frontmatter, and a wrong one breaks the skill in one harness only.

### D2: Versions in their own table, runs use the latest

Tables in `df-db`, module `skills`:
- `skills` (`id`, `name` unique, `current_version`, `created_at`, `updated_at`)
- `skill_versions` (`skill_id`, `version`, `description`, `body`, `files` JSONB, `sha256`, `created_by`, `created_at`; primary key `skill_id, version`)
- `persona_skills` (`persona_id`, `skill_id`), `project_skills` (`project_id`, `skill_id`), both `ON DELETE CASCADE`.

Each save writes a new version and moves `current_version`. A run gets the current version at run start; a run in progress keeps what it got. The skills page shows the version list (number, author, time) read-only.

Built-in skills are not rows (D3). Assignment tables reference instance skills only.

Deleting a skill deletes its versions and assignments. The audit entry keeps the last content. The confirmation dialog lists the personas and projects that use it.

### D3: Built-in skills are embedded in the binary

Built-in skills live as files in `crates/df-core/skills/<name>/` and are embedded with `include_str!` in a constant list. Their version is the Dark Factory version. They are shown on the skills page with a "Built-in" label and are read-only.

Each built-in has a scope: `always` (every run, cannot be assigned or removed) or `assignable` (behaves like an instance skill for assignment). This change ships one built-in, `dark-factory-basics`, scope `always`. `assignable` exists so playbook §2 patterns can follow without a schema change; the assignment tables then need a built-in reference (backlog row).

`dark-factory-basics` is written from playbook §1 (tasks and branches, commits, rejected pushes, pull requests and issues, artifacts, deciding/asking/requesting, network and tools, finishing, delegating, escalating). The playbook stays the source; the skill file is the runtime form. The agent memory change (ADR-0040) adds a section or a sibling skill; nothing here constrains that.

Built-in names are reserved: creating an instance skill with a built-in name fails. To avoid collisions when a later release adds a built-in, built-in names start with `dark-factory-` and instance skill names must not (decision Q1).

### D4: The run gets the union

Skills of a run = `always` built-ins ∪ skills of the persona ∪ skills of the project, deduplicated by name, ordered by name. Chat runs get the same set as task runs of the same persona and project: the chat persona works in the same repository and follows the same rules.

### D5: Skill paths are harness profile data

`HarnessProfile` gets two fields:
- `skills_dir: Option<&'static str>`: the personal skill directory, relative to `HOME`.
- `repository_skill_dirs: &'static [&'static str]`: directories, relative to the workspace root, where the harness reads project skills.

| Harness | `skills_dir` | `repository_skill_dirs` |
|---|---|---|
| `claude-code` | `.claude/skills` | `.claude/skills` |
| `opencode` | `.config/opencode/skills` (spike 1.2) | `.opencode/skills`, `.claude/skills`, `.agents/skills` (spike 1.2) |

F12 turns these into two settings of a harness registry entry (and of "Generic ACP"). The control plane passes both in `Assign`; the runtime has no harness names in its code. A harness without `skills_dir` gets no skill directory (D9).

### D6: Contract

```proto
message AcpHarness {
  // ...
  // Skill directory relative to HOME. Empty: the harness reads no skills.
  string skills_dir = 6;
  // Directories relative to the workspace root that the harness reads project skills from.
  repeated string repository_skill_dirs = 7;
  repeated Skill skills = 8;
}

message Skill {
  string name = 1;
  // "17" for an instance skill, the Dark Factory version for a built-in.
  string version = 2;
  // SKILL.md (rendered with frontmatter) and the supporting files. Empty when skills_dir is
  // empty: the runtime then only names the skill in the skills.unsupported notice.
  repeated SkillFile files = 3;
}

message SkillFile {
  // Relative to the skill's directory, normal components only.
  string path = 1;
  string content = 2;
  bool executable = 3;
}
```

Why not reuse `AcpHarness.files`: the runtime must own the whole directory (remove stale skills) and must know skill names for the shadow check. Plain files carry neither.

### D7: The runtime replaces the skill directory

After workspace prepare and before the harness starts, the runtime:
1. validates `skills_dir`, every skill name and every file path: relative, one or more normal components, no empty, `.` or `..` component. So `skills_dir` can never be `HOME` itself. The run fails before anything is removed and before the harness starts otherwise;
2. removes `<HOME>/<skills_dir>` completely, without following symlinks (a symlink at that path or at a parent component below `HOME` is removed as a link, its target is not touched). It first makes entries writable again, because step 3 leaves them read-only;
3. writes each skill to `<HOME>/<skills_dir>/<name>/`, creating directories, without following symlinks; files get mode `0444` (`0555` if executable), directories `0555`;
4. emits a `skills.delivered` notice (level `INFO`) listing `name@version`.

The directory belongs to the runtime (decision Q6). Skills the agent wrote there in an earlier run are gone in the next run. With no skills in `Assign` and a non-empty `skills_dir`, the directory is removed and stays absent.

Write lock (decision Q6): the read-only modes tell the agent that it cannot change skills there; a write fails with `Permission denied` instead of being lost silently at the next run. They are a signal, not a boundary: the harness runs as the same user and can `chmod`. A real lock is not possible with Landlock, because a Landlock rule on `/data/home` grants access to everything below it and Landlock has no deny rule for a subdirectory. Since the runtime replaces the directory on every run, a change by the agent lasts one run at most. The system prompt section (D9) says the directory is managed by Dark Factory.

Alternative: write only delivered skills and keep others. Rejected: a skill removed from a persona would stay on a reused home, and the agent could plant a skill that looks like a factory skill.

### D8: Clashes are reported, not resolved by the runtime

Trust: skill text from the factory is written by board members (or ships with Dark Factory) and is trusted. Skills in the repository are untrusted input, like `AGENTS.md` and every other file the agent reads. They stay enabled: the agent reads the repository anyway, and many projects keep useful skills there.

On a name clash the runtime does not hide, rename or remove the repository skill (decision Q2). Which copy the harness uses follows the harness's own lookup order. This keeps the behavior predictable: the workspace is always the repository's content, and the user resolves a clash by renaming one of the two skills. For Claude Code, personal skills take precedence over project skills, so the factory skill wins there, as iteration 4 decision 12 intends (spike 1.1 confirms this through the ACP adapter). Spike 1.2 records OpenCode's order; the docs state the order per harness.

After writing the skill directory the runtime checks, for each delivered name and each entry of `repository_skill_dirs`, whether `<workspace>/<dir>/<name>/SKILL.md` exists. Each hit gives a `skills.repository_skill_shadowed` notice (level `WARNING`) naming the repository path and the skill. The notice shows in the transcript, so the user sees the clash and can resolve it.

The check covers the listed directories at the workspace root only. Claude Code also reads `.claude/skills` in subdirectories while the agent works there; those are not checked (Risks).

### D9: The system prompt points to `dark-factory-basics`

Skills load on demand. Rules such as "never `--no-verify`" must apply before the model thinks it needs them. So when `dark-factory-basics` is delivered, the system prompt gets a fixed "Skills" section after the persona prompt: "Load the skill `dark-factory-basics` before your first action and follow it. The skill directory `<skills_dir>` is managed by Dark Factory; changes to it are discarded." The skill's description says the same (decision Q4).

A harness without `skills_dir` gets no skill files. `Assign` still names its skills (name and version, no files). Its system prompt gets the body of every `always` built-in appended as a section instead (decision Q3), and a `skills.unsupported` notice (level `WARNING`) lists every skill of `Assign` by name and version.

### D10: UI

- Settings area: a "Skills" entry. The list uses the two-line settings rows (name, description; "Built-in" label for built-ins). Rows open the form modal; built-ins open read-only.
- Form: name (create only), description, body (textarea with Markdown), supporting files (rows with path, content, executable checkbox; add and remove), the current version and the version list. Size shown against the limit; save rejected above it with a field error.
- Persona form: a "Skills" field with a checkbox list of instance skills (built-ins with scope `always` are shown checked and disabled, with the hint "every run").
- Project settings page: a "Skills" tab with the same list.
- Every create, edit and delete is audited: `skill.create`, `skill.update` (before and after), `skill.delete`; assignment changes are part of `persona.update` and `project.update`.
- Who: board members, like personas. `iter4-permissions` maps this to `admin`.

PenPot boards first (tasks group 2).

## Risks / Trade-offs

- [The model does not load `dark-factory-basics`] → the system prompt line (D9) and a description that says "before every task"; spike 1.1 checks a real Claude Code run.
- [Nested repository skills in subdirectories are not checked for clashes] → accepted; personal skills win by name anyway in Claude Code, the warning is information only.
- [In a harness that prefers repository skills, a repository skill with a factory name replaces the factory skill for that run] → accepted (decision Q2); the warning names the clash and the docs state each harness's order. A repository could use this to replace `dark-factory-basics` in such a harness; the Git proxy and the tools still enforce the rules the skill describes.
- [A harness reads the repository's `.claude/skills` although its profile does not list it (OpenCode's Claude compatibility)] → spike 1.2 lists every directory OpenCode reads.
- [A large skill set fills the context with descriptions] → descriptions are limited to 1024 characters; the per-run limit of 4 MiB is about transfer, not context. Accepted for now.
- [Removing the skill directory deletes files an agent wrote there on purpose] → accepted; the directory is documented as runtime-owned. Agent-written knowledge belongs to memory (ADR-0040).
- [Editing a skill changes behavior of all personas that use it at once] → versions and audit make the change traceable; pinning is in the backlog.

## Migration Plan

1. Migration adds the four tables. No existing data changes.
2. Old runtimes ignore the new `AcpHarness` fields (proto3, additive); new runtimes with an old control plane get no skills and leave `skills_dir` empty, so they write nothing.
3. Rollback: drop the tables, then remove the delivered skill directories from the task volumes before downgraded runtimes start runs (delete the volumes, or remove `.claude/skills` and `.config/opencode/skills` below each home without following symlinks). An old runtime never touches these directories, and a new runtime with an old control plane gets an empty `skills_dir`, so neither removes them: the harness would keep reading stale skills. Pre-MVP no installation needs this; the upgrade docs say so.

## Decisions (2026-11-21, Peter Kurfer)

The open questions of the first draft are decided. The recommendation was taken unless noted.

- **Q1: Reserved prefix for built-in names.** Built-in names start with `dark-factory-`; instance skill names must not. Alternative was a migration that renames a colliding instance skill when a release adds a built-in.
- **Q2: Clash with a repository skill.** Changed from the recommendation: the runtime does not exclude the repository skill from the checkout. Users resolve clashes themselves; the behavior stays predictable (D8). The warning stays.
- **Q3: Harness without a skill directory.** Append the `always` built-ins (only `dark-factory-basics` today) to the system prompt and skip all other skills with a warning (D9).
- **Q4: `dark-factory-basics` as a skill plus a pointer.** Skill plus the system prompt section (D9), not the whole text in the prompt.
- **Q5: Opting out of `dark-factory-basics`.** Not possible.
- **Q6: Skill directory ownership.** The runtime owns the whole directory and removes unknown entries (D7). Follow-up question from the review: a write lock. Read-only modes signal it; a real lock is not possible with Landlock (D7).
- **Q7: Chats get the same skills as tasks.** Yes (D4).

## Open Questions

None.
