# Spec Delta

## ADDED Requirements

### Requirement: Skill directory delivery
With a non-empty `Assign.harness.acp.skills_dir`, the runtime SHALL, after workspace prepare and before the harness starts, remove the directory `<HOME>/<skills_dir>` with everything in it and write every skill of `Assign.harness.acp.skills` to `<HOME>/<skills_dir>/<name>/`, executable files with mode `0755` and others with `0644`. It SHALL NOT follow symlinks: a symlink at the directory or at one of its components below `HOME` SHALL be removed as a link, its target unchanged. A `skills_dir`, skill name or file path that is absolute or contains `..` SHALL fail the run before the harness starts. The runtime SHALL emit a `skills.delivered` notice listing each skill as `<name>@<version>`.

#### Scenario: Skills written
- **WHEN** `Assign` has `skills_dir` `.claude/skills` and skills `dark-factory-basics` and `release-notes` with `scripts/collect.sh` executable
- **THEN** before the adapter starts, `/data/home/.claude/skills/dark-factory-basics/SKILL.md` and `/data/home/.claude/skills/release-notes/scripts/collect.sh` exist, the script is executable, and a `skills.delivered` notice lists both skills with their versions

#### Scenario: Removed skill on a reused home
- **WHEN** run 1 got `release-notes`, the board removed it from the persona, and run 2 starts on the same task volume
- **THEN** `/data/home/.claude/skills/release-notes` does not exist when run 2's adapter starts

#### Scenario: Skill planted by the agent
- **WHEN** the agent wrote `/data/home/.claude/skills/deploy/SKILL.md` in run 1
- **THEN** the directory is absent when run 2's adapter starts

#### Scenario: Fresh home
- **WHEN** a run starts with an empty `/data/home`
- **THEN** the skill directory is created with all skills of `Assign`

#### Scenario: Symlinked skill directory
- **WHEN** the agent replaced `/data/home/.claude/skills` with a symlink to `/data/repo/.claude/skills`
- **THEN** the runtime removes the link, writes the skills into a real directory, and `/data/repo/.claude/skills` is unchanged

#### Scenario: Path escapes the skill
- **WHEN** `Assign` carries a skill file with path `../../.bashrc`
- **THEN** the run fails before the adapter starts and no file is written

### Requirement: Shadowed repository skills reported
After delivering the skills, the runtime SHALL check, for each delivered skill name and each entry of `Assign.harness.acp.repository_skill_dirs`, whether `<workspace>/<dir>/<name>/SKILL.md` exists, and SHALL emit one `skills.repository_skill_shadowed` notice (level `WARNING`) per hit naming the skill and the repository path. Repository skills with other names SHALL stay untouched.

#### Scenario: Repository skill with a factory name
- **WHEN** the repository contains `.claude/skills/release-notes/SKILL.md` and the run gets the instance skill `release-notes`
- **THEN** the transcript shows a warning that `.claude/skills/release-notes` is shadowed by the factory skill `release-notes`, and the harness uses the factory skill

#### Scenario: Other repository skill
- **WHEN** the repository contains `.claude/skills/bazel/SKILL.md` and no factory skill is named `bazel`
- **THEN** no warning is emitted and the harness can use the repository skill

### Requirement: Basics pointer in the system prompt
When a run's skills include `dark-factory-basics` and the harness has a skill directory, the system prompt SHALL contain a fixed line telling the agent to load the skill `dark-factory-basics` before its first action. Without `dark-factory-basics` the line SHALL be absent.

#### Scenario: Pointer present
- **WHEN** a run of a `claude-code` persona starts
- **THEN** the system prompt names `dark-factory-basics` and asks the agent to load it before its first action

### Requirement: Harness without a skill directory
When the harness has no skill directory and the run has skills, the control plane SHALL append the body of every built-in skill that applies to every run to the system prompt as a section and SHALL NOT deliver other skills; the runtime SHALL emit a `skills.unsupported` notice (level `WARNING`) listing the skills not delivered.

#### Scenario: Generic harness without skill support
- **WHEN** a run of a harness without a skill directory starts for a persona with skill `release-notes`
- **THEN** the system prompt contains the text of `dark-factory-basics`, no skill directory is written, and a `skills.unsupported` warning names `release-notes`
