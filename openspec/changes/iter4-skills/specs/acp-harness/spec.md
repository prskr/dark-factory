# Spec Delta

## ADDED Requirements

### Requirement: Skill directory replaced on every run
With a non-empty `Assign.harness.acp.skills_dir`, the runtime SHALL, after workspace prepare and before the harness starts, remove `<HOME>/<skills_dir>` with everything in it and write every skill of `Assign.harness.acp.skills` to `<HOME>/<skills_dir>/<name>/`. It SHALL then emit a `skills.delivered` notice listing each skill as `<name>@<version>`.

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

### Requirement: Skill paths validated
The runtime SHALL accept a `skills_dir`, skill name or skill file path only if it is relative and consists of one or more normal components (no empty, `.` or `..` component), so the skill directory is always strictly below `HOME`. Any other value SHALL fail the run before anything is removed or written and before the harness starts.

#### Scenario: Skill directory is HOME
- **WHEN** `Assign` has `skills_dir` `.`
- **THEN** the run fails before the adapter starts and nothing below `/data/home` is removed

#### Scenario: Path escapes the skill
- **WHEN** `Assign` carries a skill file with path `../../.bashrc`
- **THEN** the run fails before the adapter starts and no file is written

### Requirement: Skill directory without symlink following
The runtime SHALL NOT follow symlinks when it removes or writes the skill directory. A symlink at the skill directory or at one of its components below `HOME` SHALL be removed as a link, its target unchanged.

#### Scenario: Symlinked skill directory
- **WHEN** the agent replaced `/data/home/.claude/skills` with a symlink to `/data/repo/.claude/skills`
- **THEN** the runtime removes the link, writes the skills into a real directory, and `/data/repo/.claude/skills` is unchanged

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
- **THEN** the transcript shows a warning that `.claude/skills/release-notes` clashes with the factory skill `release-notes`, and Claude Code uses the factory skill because it prefers personal skills

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

### Requirement: Harness without a skill directory
When `Assign.harness.acp.skills_dir` is empty, the runtime SHALL write no skill files and SHALL emit a `skills.unsupported` notice (level `WARNING`) listing every skill of `Assign` by name and version. The control plane SHALL append the body of every skill that applies to every run to the system prompt as a section.

#### Scenario: Generic harness without skill support
- **WHEN** a run of a harness without a skill directory starts for a persona with skill `release-notes`
- **THEN** the system prompt contains the text of `dark-factory-basics`, no skill directory is written, and a `skills.unsupported` warning names `dark-factory-basics` and `release-notes`
