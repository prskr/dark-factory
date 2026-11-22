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
- Adding a harness (F12, Generic ACP) needs only three path settings for skills, no code.
- Skill text written by humans is trusted; the repository cannot replace it.

**Non-Goals:**
- Agent instance operations on skills → roadmap iteration 4, "Full permission model (ADR-0011)".
- Memory instructions → roadmap iteration 6, agent memory (ADR-0040).
- Per-persona tool permissions → roadmap iteration 4, "Full permission model (ADR-0011)", separate change.
- Pattern skills from playbook §2 as built-ins, import from Git or archives, version pinning and rollback, project exclusions of persona skills, filtering of non-colliding repository skills → roadmap backlog (rows added at archive).
- Binary supporting files → rejected.
- Skill support in full-harness mode (built-in loop): not needed here. Its profile has no `skills_dir`, so it gets the D9 fallback (`dark-factory-basics` in the system prompt, `skills.unsupported` for the rest). Reading a skill directory is up to the `full-harness-mode` change.

## Decisions

### D1: A skill is name, description, body and text files

Fields:
- `name`: 1–64 characters `[a-z0-9-]`, no leading, trailing or double hyphen (the Agent Skills naming rule that Claude Code and OpenCode share). Unique across built-in and instance skills. Not editable after creation, because the name is the directory name and other text refers to it.
- `description`: 1–1024 characters, one line. The harness shows it to the model to decide when to load the skill.
- `body`: Markdown, the `SKILL.md` text without frontmatter.
- `files`: supporting files, each a relative path (1–8 normal components, at most 255 bytes, no NUL byte, unique, no path an ancestor of another, not `SKILL.md` or below it), UTF-8 content and an `executable` flag.

Dark Factory renders `SKILL.md` as YAML frontmatter (`name`, `description`) followed by the body. The user never writes frontmatter, so it is always valid for every harness.

Limits: one skill at most 256 KiB and 50 files; all skills of a run at most 4 MiB. Both count the encoded size of everything that goes into `Assign` for the skill (name, version, rendered `SKILL.md`, paths and contents of the supporting files), not only the contents, so long paths cannot push `Assign` past `MAX_MESSAGE_BYTES` (16 MiB); the remaining 12 MiB are headroom for the rest of `Assign`. `build_assign` checks the run limit when it first builds `Assign` for a run (at the first agent connect). Over the limit, the channel ends the run with outcome `FAILED` and a reason naming the limit through the run end handling (`df_orchestrator::runs::finish_run`, which also tears the sandbox down), and closes the stream with `FAILED_PRECONDITION`. It must not just return a `ChannelError`: that becomes `Status::internal`, the runtime reconnects, and because the connect timeout has already stopped, the run would hang until its budget ends with no reason shown. (The existing missing-model check in `persona_model` has the same gap; it is not fixed here.) A reconnect rebuilds `Assign` from the run's snapshot (D2), so a later edit cannot make a reconnect fail.

Alternative: store the user's `SKILL.md` verbatim. Rejected: users would have to write frontmatter, and a wrong one breaks the skill in one harness only.

### D2: Versions in their own table, runs use the latest

Tables in `df-db`, module `skills`:
- `skills` (`id`, `name` unique, `created_at`, `updated_at`)
- `skill_versions` (`skill_id`, `version`, `description`, `body`, `files` JSONB, `created_by`, `created_at`; primary key `skill_id, version`). The current version is the highest one.
- `persona_skills` (`persona_id`, `skill_id`), `project_skills` (`project_id`, `skill_id`), both `ON DELETE CASCADE`.
- `run_skills` (`run_id` referencing `runs` with `ON DELETE CASCADE`, `name`, `version`, `files` JSONB with the rendered `SKILL.md` and the supporting files): a snapshot of every skill the run got, built-in and instance, written by the first `build_assign` of the run. Primary key `run_id, name`; no reference to `skills`. Two connects of the same run can build `Assign` at the same time and could select different skills if an assignment changes in between. So `build_assign` locks the run row (`SELECT … FOR UPDATE`) in one transaction, reads the snapshot, and only if there is none selects the skills and inserts them, then commits. A connect that waited for the lock finds the snapshot and returns it without selecting skills; both connects deliver the same content.

Each save writes a new version. The first `build_assign` of a run (in `assign.rs`, not when the run context is loaded before provisioning) reads the current versions and the built-ins of the serving control plane and writes the snapshot; every later `build_assign` of the run (reconnects) reads only the snapshot. So `Assign` stays identical for the same `run_id`, as the contract requires: also when a reconnect reaches a control plane replica of a newer release during a rollout, and also when the skill was edited or deleted in between. A version saved after provisioning but before the first `Assign` is delivered. The run end handling in `df-orchestrator` deletes the snapshot when the run reaches a final state (`build_assign` is its only reader); the `skills.delivered` notice in the transcript keeps the names and versions. The skills page shows the version list (number, author, time) read-only.

Built-in skills are not rows in `skills` (D3). Assignment tables reference instance skills only.

Deleting a skill deletes its versions and assignments; snapshots of active runs stay until those runs end. The audit entry keeps the last content. The confirmation dialog lists the personas and projects that use it.

### D3: Built-in skills are embedded in the binary

Built-in skills live as files in `crates/df-core/skills/<name>/` and are embedded with `include_str!` in a constant list. Their version is the Dark Factory version. They are shown on the skills page with a "Built-in" label and are read-only.

Each built-in has a scope: `always` (every run, cannot be assigned or removed) or `assignable` (behaves like an instance skill for assignment). This change ships one built-in, `dark-factory-basics`, scope `always`. `assignable` exists so playbook §2 patterns can follow without a schema change; the assignment tables then need a built-in reference (backlog row).

`dark-factory-basics` is written from playbook §1 (tasks and branches, commits, rejected pushes, pull requests and issues, artifacts, deciding/asking/requesting, network and tools, finishing, delegating, escalating). The playbook stays the source; the skill file is the runtime form. The agent memory change (ADR-0040) adds a section or a sibling skill; nothing here constrains that.

Built-in names are reserved: creating an instance skill with a built-in name fails. To avoid collisions when a later release adds a built-in, built-in names start with `dark-factory-` and instance skill names must not (decision Q1).

### D4: The run gets the union

Skills of a run = `always` built-ins ∪ skills of the persona ∪ skills of the project, deduplicated by name, ordered by name. Chat runs get the same set as task runs of the same persona and project: the chat persona works in the same repository and follows the same rules.

### D5: Skill paths are harness profile data

`HarnessProfile` gets three fields:
- `skills_dir: Option<&'static str>`: the global skill directory the runtime writes, relative to `HOME`.
- `other_home_skill_dirs: &'static [&'static str]`: further directories below `HOME` the harness reads global skills from. The runtime empties them on every run and writes nothing there (decision Q8).
- `repository_skill_dirs: &'static [&'static str]`: directories, relative to the workspace root, where the harness reads project skills.

| Harness | `skills_dir` | `other_home_skill_dirs` | `repository_skill_dirs` |
|---|---|---|---|
| `claude-code` | `.claude/skills` | none | `.claude/skills` |
| `opencode` | `.config/opencode/skills` (spike 1.2) | `.claude/skills`, `.agents/skills` (spike 1.2) | `.opencode/skills`, `.claude/skills`, `.agents/skills` (spike 1.2) |

F12 turns these into three settings of a harness registry entry (and of "Generic ACP"), next to the session state directories of `chat-emptydir-session-restore`. The control plane passes them in `Assign`; the runtime has no harness names in its code. A harness without `skills_dir` gets no skill directory (D9).

Path rules, checked by a `df-core` test for every profile, by the F12 configuration validation, and again by the runtime:
- every entry is relative with one or more normal components (no empty, `.` or `..` component);
- `other_home_skill_dirs` is empty when `skills_dir` is empty: a harness without a skill directory gets the D9 fallback, and the runtime touches no skill directory for it;
- `skills_dir` and `other_home_skill_dirs` are pairwise neither equal nor ancestor or descendant of each other, nor of any harness file path (`system_prompt_path`, `settings_path`, every `AcpHarness.files` path) or session state directory. So `skills_dir = .claude` is rejected, because it would remove `.claude/CLAUDE.md`.

### D6: Contract

```proto
message AcpHarness {
  // ...
  // Placeholder numbers <n1>…<n4>: the final field numbers are assigned when the change is
  // implemented (task 4.2), as the next free numbers of AcpHarness at that time.
  // Skill directory relative to HOME. Empty: the harness reads no skills.
  string skills_dir = <n1>;
  // Directories relative to the workspace root that the harness reads project skills from.
  repeated string repository_skill_dirs = <n2>;
  repeated Skill skills = <n3>;
  // Further directories relative to HOME the harness reads global skills from; the runtime
  // empties them and writes nothing there.
  repeated string other_home_skill_dirs = <n4>;
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
1. checks that `HOME` is a real directory with no symlink among its path components (the same rule as "Aliases from unrestricted runs" in `sandbox-filesystem-isolation`, but here, before any removal, and also with Landlock disabled); validates `skills_dir`, `other_home_skill_dirs`, `repository_skill_dirs`, every skill name and every file path with the path rules of D5, and the overlap rule against the harness files of `Assign`. The run fails before anything is removed and before the harness starts otherwise;
2. removes `<HOME>/<skills_dir>` and every `<HOME>/<other_home_skill_dirs>` completely. All operations are relative to a directory descriptor of `HOME` and open every component with `O_NOFOLLOW` (the `rustix` `openat` helpers that `chat-emptydir-session-restore` introduces; until then, the same helpers added here). A symlink at the directory or at one of its components below `HOME` is removed as a link, its target untouched. Because step 3 leaves directories read-only, the runtime makes each real directory writable before it removes its entries: it opens it with `O_DIRECTORY | O_NOFOLLOW` and uses `fchmod` on that descriptor. It never changes the mode of a file or a symlink, so a planted link or hard link cannot make the runtime change another file's mode (removing a file only needs write access to its directory);
3. writes each skill to `<HOME>/<skills_dir>/<name>/`, creating directories, with the same no-follow operations; files get mode `0444` (`0555` if executable), directories `0555`;
4. writes the harness files of `Assign` (after the skill directories, so an error in the skill step fails the run before any harness file changes);
5. emits a `skills.delivered` notice (level `INFO`) listing `name@version`.

The directories belong to the runtime (decisions Q6, Q8). Skills the agent wrote there in an earlier run are gone in the next run, also after the persona switched to another harness on the same task volume, as long as the new harness lists the old directory. With no skills in `Assign` and a non-empty `skills_dir`, the directory is removed and stays absent.

Write lock (decision Q6): the read-only modes tell the agent that it cannot change skills there; a write fails with `Permission denied` instead of being lost silently at the next run. They are a signal, not a boundary: the harness runs as the same user and can `chmod`. A real lock is not possible with Landlock, because a Landlock rule on `/data/home` grants access to everything below it and Landlock has no deny rule for a subdirectory. Since the runtime replaces the directory on every run, a change by the agent lasts one run at most. The system prompt section (D9) says the directory is managed by Dark Factory.

Alternative: write only delivered skills and keep others. Rejected: a skill removed from a persona would stay on a reused home, and the agent could plant a skill that looks like a factory skill.

### D8: Clashes are reported, not resolved by the runtime

Trust: skill text from the factory is written by board members (or ships with Dark Factory) and is trusted. Skills in the repository are untrusted input, like `AGENTS.md` and every other file the agent reads. They stay enabled: the agent reads the repository anyway, and many projects keep useful skills there.

On a name clash the runtime does not hide, rename or remove the repository skill (decision Q2). Which copy the harness uses follows the harness's own lookup order. This keeps the behavior predictable: the workspace is always the repository's content, and the user resolves a clash by renaming one of the two skills. For Claude Code, personal skills take precedence over project skills, so the factory skill wins there, as iteration 4 decision 12 intends (spike 1.1 confirms this through the ACP adapter). Spike 1.2 records OpenCode's order; the docs state the order per harness.

After writing the skill directory the runtime checks, for each delivered name and each entry of `repository_skill_dirs`, whether `<workspace>/<dir>/<name>/SKILL.md` exists. Each hit gives a `skills.repository_skill_shadowed` notice (level `WARNING`) naming the repository path and the skill. The notice shows in the transcript, so the user sees the clash and can resolve it.

The check covers the listed directories at the workspace root only. Claude Code also reads `.claude/skills` in subdirectories while the agent works there; those are not checked (Risks).

### D9: The system prompt points to `dark-factory-basics`

Skills load on demand. Rules such as "never `--no-verify`" must apply before the model thinks it needs them. So when `dark-factory-basics` is delivered, the system prompt gets a fixed "Skills" section after the persona prompt: "Load the skill `dark-factory-basics` before your first action and follow it. The skill directory `<skills_dir>` is managed by Dark Factory; changes to it are discarded." The skill's description says the same (decision Q4).

Order of the system prompt sections: the Skills section goes right after the persona prompt; the rest keeps today's order. For tasks: persona prompt, Skills, then the Sandbox and hidden-path sections (`system_prompt` in `assign.rs` appends them last). For chats: persona prompt, Skills, the project context and the chat instructions (`chat_system_prompt` of `iter3-project-context`), then the Sandbox and hidden-path sections. So the Skills section is inserted into whatever `chat_system_prompt` builds, after the persona prompt; neither change moves the other's sections. Chats (decision Q9): the chat instructions get one more sentence: "They take precedence over `dark-factory-basics`: sections about branches, commits, pushes, pull requests and `complete_task` do not apply in a chat." `dark-factory-basics` stays one text for tasks and chats.

A harness without `skills_dir` gets no skill files. `Assign` still names its skills (name and version, no files). Its system prompt gets the body of every `always` built-in appended as a section instead (decision Q3), and a `skills.unsupported` notice (level `WARNING`) lists every skill of `Assign` by name and version.

### D10: UI

The skill UI takes the shape of the persona settings page of the change `markdown-editor` (its design D2b and decision Q10). It reuses that change's parts; it does not describe the editor again.

- Settings area: a "Skills" entry. The list uses the two-line settings rows (name, description; "Built-in" label for built-ins). A row links to the skill's settings page.
- Create: a "New skill" modal with name and description only. Saving answers `HX-Redirect: /skills/{name}?tab=body`, like a new persona opens on its System prompt tab.
- Skill settings page `/skills/{name}?tab=` (the name is the key; it cannot change): `settings_page_frame` with back link "Skills", the name as heading and one "Save skill" button; `tabbed_form(…, TabsStyle::Page)` with the tabs **General** (name read-only, description, size against the limit, the version list), **Body** (only the body, in a Markdown field with the side-by-side layout, as on the persona System prompt tab) and **Files** (supporting file rows with path, content as a plain textarea, executable checkbox; add and remove). The open tab is in the URL (`tab=body`, `tab=files`; General is left out), and saving and validation errors follow "Saving the persona settings page". A save above the size limit is rejected with a field error.
- The Markdown field's behavior (layouts, preview, highlighting, phones, keyboard access) is the merged `markdown-editor` spec; this change only places it. Supporting files are not Markdown and stay plain textareas (`markdown-editor`, Out of Scope).
- Built-ins open the same page read-only: no "Save skill" and no delete action; the Body tab shows the rendered body.
- Persona settings page: a **Skills** tab (`?tab=skills`, design D2b "Later tabs" of `markdown-editor`) with a checkbox list of instance skills; built-ins with scope `always` are shown checked and disabled, with the hint "every run". It is saved with "Save persona".
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

1. Migration adds the five tables (`skills`, `skill_versions`, `persona_skills`, `project_skills`, `run_skills`). No existing data changes.
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

Decided on 2026-11-22 (Peter Kurfer), from the review of DF-15:

- **Q8: Other global skill directories of a harness.** The runtime empties every global skill directory the profile lists (`other_home_skill_dirs`, for OpenCode `~/.claude/skills` and `~/.agents/skills`, to confirm in spike 1.2) and writes only into `skills_dir` (D5, D7). Alternative was to own only `skills_dir` and accept that planted or stale skills in the other directories survive between runs.
- **Q9: `dark-factory-basics` in chats.** The chat instructions take precedence and say which sections of the skill do not apply (D9). Alternative was to mark task-only sections inside the skill, which puts chat knowledge into every task run's context.

The `AcpHarness` field number clash with `chat-emptydir-session-restore` needed no decision: this change uses placeholder numbers, and task 4.2 assigns the next free numbers when it is implemented.

## Open Questions

None.
