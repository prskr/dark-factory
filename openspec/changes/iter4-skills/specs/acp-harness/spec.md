# Spec Delta

## MODIFIED Requirements

### Requirement: Harness files
The runtime SHALL write every file of `Assign.harness.acp.files` below `/data/home` before the harness starts, after the skill directories, overwriting what is there, creating missing directories, without following symlinks. A path that is absolute or contains `..` SHALL fail the run before the harness starts. Apart from the skill directories ("Skill directory replaced on every run"), the runtime SHALL write no other harness file of its own.

#### Scenario: Files written
- **WHEN** `Assign` carries files `.claude/settings.json` and `.claude/CLAUDE.md`
- **THEN** both exist under `/data/home` with the given content before the adapter starts

#### Scenario: Path escapes HOME
- **WHEN** `Assign` carries a file `../repo/.git/config`
- **THEN** the run fails before the adapter starts and no file is written outside `/data/home`

#### Scenario: Symlink planted by the agent
- **WHEN** `/data/home/.config` is a symlink to `/data/repo` and `Assign` carries `.config/opencode/AGENTS.md`
- **THEN** the runtime does not write through the symlink

## ADDED Requirements

### Requirement: Skill directory replaced on every run
With a non-empty `Assign.harness.acp.skills_dir`, the runtime SHALL, after workspace prepare and before the harness starts, remove `<HOME>/<skills_dir>` and every `<HOME>/<other_home_skill_dirs>` with everything in them, and write every skill of `Assign` to `<HOME>/<skills_dir>/<name>/`. It SHALL then emit a `skills.delivered` notice listing each skill as `<name>@<version>`.

#### Scenario: Skills written
- **WHEN** `Assign` has `skills_dir` `.claude/skills` and skills `dark-factory-basics` and `release-notes` with `scripts/collect.sh`
- **THEN** before the adapter starts, `/data/home/.claude/skills/dark-factory-basics/SKILL.md` and `/data/home/.claude/skills/release-notes/scripts/collect.sh` exist, and a `skills.delivered` notice lists both skills with their versions

#### Scenario: Removed skill on a reused home
- **WHEN** run 1 got `release-notes`, the board removed it from the persona, and run 2 starts on the same task volume
- **THEN** `/data/home/.claude/skills/release-notes` does not exist when run 2's adapter starts

#### Scenario: Skill planted by the agent
- **WHEN** the agent wrote `/data/home/.claude/skills/deploy/SKILL.md` in run 1
- **THEN** the directory is absent when run 2's adapter starts

#### Scenario: Fresh home
- **WHEN** a run starts with an empty `/data/home`
- **THEN** the skill directory is created with all skills of `Assign`

#### Scenario: Other global skill directory emptied
- **WHEN** an `opencode` run lists `.agents/skills` in `other_home_skill_dirs` and the agent wrote `/data/home/.agents/skills/deploy/SKILL.md` in run 1
- **THEN** `/data/home/.agents/skills` is absent or empty when run 2's adapter starts

### Requirement: Skill paths validated
An empty `skills_dir` SHALL mean "no skill directory". The runtime SHALL accept a non-empty `skills_dir`, an entry of `other_home_skill_dirs` or `repository_skill_dirs`, a skill name or a skill file path only if it is relative and consists of one or more normal components (no empty, `.` or `..` component), so every skill directory is strictly below `HOME` or the workspace. Any other value SHALL fail the run before anything is removed or written and before the harness starts.

#### Scenario: No skill directory
- **WHEN** `Assign` has an empty `skills_dir`
- **THEN** the path check passes, nothing below `/data/home` is removed for skills, and the adapter starts

#### Scenario: Skill directory is HOME
- **WHEN** `Assign` has `skills_dir` `.`
- **THEN** the run fails before the adapter starts and nothing below `/data/home` is removed

#### Scenario: Path escapes the skill
- **WHEN** `Assign` carries a skill file with path `../../.bashrc`
- **THEN** the run fails before the adapter starts and no file is written

#### Scenario: Skill directory overlaps a harness file
- **WHEN** `Assign` has `skills_dir` `.claude` and a harness file `.claude/CLAUDE.md`
- **THEN** the run fails before the adapter starts and `/data/home/.claude/CLAUDE.md` is unchanged

### Requirement: Skill directories apart from harness files
The runtime SHALL fail the run before anything is removed or written if `skills_dir` or an entry of `other_home_skill_dirs` is equal to, an ancestor of or a descendant of another of them, of a path of `Assign.harness.acp.files`, or of a session state directory of `Assign` (if the harness has any). The control plane SHALL check the same rule for every harness profile, and that a profile without `skills_dir` has no `other_home_skill_dirs`.

#### Scenario: Profiles are consistent
- **WHEN** the harness profiles are loaded
- **THEN** no profile has a skill directory that overlaps its system prompt path, settings path or another skill directory

### Requirement: HOME checked before skill delivery
Before it removes anything below `HOME`, the runtime SHALL check that `HOME` is a real directory with no symlink among its path components, also with Landlock disabled; otherwise the run SHALL fail before the harness starts and nothing SHALL be removed.

#### Scenario: Symlinked home
- **WHEN** an earlier unrestricted run replaced `/data/home` with a symlink to `/data/runtime`
- **THEN** the run fails before the adapter starts and nothing below `/data/runtime` is removed

### Requirement: Skill directory without symlink following
The runtime SHALL NOT follow symlinks when it removes or writes skill directories: every operation SHALL be relative to a descriptor of `HOME`, opening each component without following links. A symlink at a skill directory or at one of its components SHALL be removed as a link, its target unchanged. The runtime SHALL change modes only of real directories it opened without following links, never of files or links.

#### Scenario: Symlinked skill directory
- **WHEN** the agent replaced `/data/home/.claude/skills` with a symlink to `/data/repo/.claude/skills`
- **THEN** the runtime removes the link, writes the skills into a real directory, and `/data/repo/.claude/skills` is unchanged

#### Scenario: Link to a runtime file inside the skill directory
- **WHEN** the agent placed `/data/home/.claude/skills/x/f` as a symlink or a hard link to a file below `/data/runtime` or `/run/factory`
- **THEN** after the next delivery the link is gone, and the target's content and mode are unchanged

### Requirement: Skill directory marked read-only
The runtime SHALL write skill files with mode `0444` (`0555` if executable) and skill directories with mode `0555`, so writes by the harness fail by default. The runtime SHALL still be able to replace the directory on the next run.

#### Scenario: Agent edits a skill
- **WHEN** the harness runs `echo x >> /data/home/.claude/skills/release-notes/SKILL.md`
- **THEN** the write fails with `Permission denied`

#### Scenario: Executable script
- **WHEN** a skill file `scripts/collect.sh` is marked executable
- **THEN** the harness can run `/data/home/.claude/skills/release-notes/scripts/collect.sh`

#### Scenario: Replaced after an agent's chmod
- **WHEN** the agent made a skill file writable and changed it in run 1
- **THEN** run 2 starts with the delivered content

### Requirement: Shadowed repository skills reported
After delivering the skills, the runtime SHALL emit one `skills.repository_skill_shadowed` notice (level `WARNING`) for each delivered skill name and each entry of `Assign.harness.acp.repository_skill_dirs` where `<workspace>/<dir>/<name>/SKILL.md` exists, naming the skill and the repository path. The runtime SHALL NOT change or hide repository skills; on a clash the harness's own lookup order decides.

#### Scenario: Clash in Claude Code
- **WHEN** the repository contains `.claude/skills/release-notes/SKILL.md` and a `claude-code` run gets the instance skill `release-notes`
- **THEN** the transcript shows a warning that `.claude/skills/release-notes` clashes with the factory skill `release-notes`, and Claude Code uses the factory skill because it prefers personal skills (harness behavior, verified by spike 1.1, not by a unit test)

#### Scenario: Repository skill kept
- **WHEN** the repository contains `.claude/skills/release-notes/SKILL.md` and the run gets `release-notes`
- **THEN** the repository file is unchanged and present in the workspace

#### Scenario: Other repository skill
- **WHEN** the repository contains `.claude/skills/bazel/SKILL.md` and no factory skill is named `bazel`
- **THEN** no warning is emitted and the harness can use the repository skill

### Requirement: Skills pointer in the system prompt
When the harness has a skill directory and a run's skills include `dark-factory-basics`, the system prompt SHALL contain a fixed section that tells the agent to load `dark-factory-basics` before its first action and states that the skill directory is managed by Dark Factory and changes to it are discarded. Otherwise the section SHALL be absent.

#### Scenario: Pointer present
- **WHEN** a run of a `claude-code` persona starts
- **THEN** the system prompt names `dark-factory-basics`, asks the agent to load it before its first action, and says that `~/.claude/skills` is managed by Dark Factory

### Requirement: Chat instructions take precedence over the basics
In a chat run, the Skills section SHALL come before the project context and the chat instructions, and the chat instructions SHALL state that they take precedence over `dark-factory-basics` and that its sections about branches, commits, pushes, pull requests and `complete_task` do not apply.

#### Scenario: Chat prompt order
- **WHEN** a chat run of project `DF` with project context starts
- **THEN** the system prompt has the persona prompt, the Skills section, the project context and the chat instructions in this order, and the chat instructions say they take precedence over `dark-factory-basics`

### Requirement: Harness without a skill directory
When `Assign.harness.acp.skills_dir` is empty, the runtime SHALL write no skill files and SHALL emit a `skills.unsupported` notice (level `WARNING`) listing every skill of `Assign` by name and version. The control plane SHALL append the body of every skill that applies to every run to the system prompt as a section.

#### Scenario: Generic harness without skill support
- **WHEN** a run of a harness without a skill directory starts for a persona with skill `release-notes`
- **THEN** the system prompt contains the text of `dark-factory-basics`, no skill directory is written, and a `skills.unsupported` warning names `dark-factory-basics` and `release-notes`
