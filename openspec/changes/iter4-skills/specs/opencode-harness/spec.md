# Spec Delta

## ADDED Requirements

### Requirement: OpenCode skill directories
An `opencode` run SHALL get OpenCode's own global skill directory as `skills_dir`, every other global skill directory OpenCode reads as `other_home_skill_dirs`, and every workspace directory it reads project skills from as `repository_skill_dirs`. Expected until spike 1.2 of `iter4-skills` records the observed paths: `.config/opencode/skills`; `.claude/skills`, `.agents/skills`; `.opencode/skills`, `.claude/skills`, `.agents/skills`.

#### Scenario: OpenCode lists the factory skills
- **WHEN** an `opencode` run starts with skills `dark-factory-basics` and `release-notes`
- **THEN** both are written below `/data/home/.config/opencode/skills` and OpenCode's skill tool lists both

#### Scenario: Claude-compatible repository skill
- **WHEN** an `opencode` run gets `release-notes` and the repository contains `.claude/skills/release-notes/SKILL.md`
- **THEN** a `skills.repository_skill_shadowed` warning names `.claude/skills/release-notes`
