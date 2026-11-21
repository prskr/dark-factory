# Spec Delta

## ADDED Requirements

### Requirement: Skills in Assign
`Assign.harness.acp` SHALL carry the skills of the run (see `skills`, "Skill assignment"), each with name, version and its files (the rendered `SKILL.md` and the supporting files), and the harness profile's skill directory and repository skill directories. For a harness profile without a skill directory, `skills_dir` SHALL be empty and the skills SHALL carry name and version but no files. Chat runs SHALL carry skills the same way.

#### Scenario: Claude Code run
- **WHEN** run 1 of `DF-3` by `claude-code` persona `linus` with skill `release-notes` version 4 connects
- **THEN** `Assign.harness.acp.skills_dir` is `.claude/skills`, `repository_skill_dirs` is `[".claude/skills"]`, and `skills` holds `dark-factory-basics` at the Dark Factory version and `release-notes` at version `4`

#### Scenario: Rendered SKILL.md
- **WHEN** `Assign` carries `release-notes`
- **THEN** the skill's file `SKILL.md` starts with frontmatter naming `release-notes` and its description

#### Scenario: Harness without a skill directory
- **WHEN** a run of a harness without a skill directory and a persona with skill `release-notes` version 4 connects
- **THEN** `Assign.harness.acp.skills_dir` is empty and `skills` lists `dark-factory-basics` and `release-notes` at version `4`, each without files

#### Scenario: Chat run
- **WHEN** a run of `DF-12-CHAT` by persona `linus` connects
- **THEN** `Assign.harness.acp.skills` holds the same skills as a task run of `linus` in `DF`
