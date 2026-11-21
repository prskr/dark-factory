# Spec Delta

## ADDED Requirements

### Requirement: OpenCode skill directories
An `opencode` run SHALL get the skill directory OpenCode reads global skills from (`.config/opencode/skills` below `HOME`, confirmed by the spike of `iter4-skills`) and, as repository skill directories, every workspace directory OpenCode reads project skills from (`.opencode/skills`, `.claude/skills`, `.agents/skills`, confirmed by the same spike).

#### Scenario: OpenCode lists the factory skills
- **WHEN** an `opencode` run starts with skills `dark-factory-basics` and `release-notes`
- **THEN** both are written below `/data/home/.config/opencode/skills` and OpenCode's skill tool lists both

#### Scenario: Claude-compatible repository skill
- **WHEN** an `opencode` run gets `release-notes` and the repository contains `.claude/skills/release-notes/SKILL.md`
- **THEN** a `skills.repository_skill_shadowed` warning names `.claude/skills/release-notes`
