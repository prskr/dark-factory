# Spec Delta

## MODIFIED Requirements

### Requirement: Commit identity
Each checked commit's author and committer email SHALL equal the commit email of the pushing run's persona identity as carried in the run's Git route; otherwise the push SHALL be rejected with `df:identity`. This check SHALL NOT be configurable.

#### Scenario: Commit as a human
- **WHEN** a pushed commit has author `peter@example.org`
- **THEN** the push is rejected with `df:identity`

#### Scenario: Commit as another persona
- **WHEN** a run of persona `linus` (`linus@example.org`) pushes a commit authored and committed by `ada@example.org`
- **THEN** the push is rejected with `df:identity`

#### Scenario: Commit as the run's persona
- **WHEN** a run of persona `linus` pushes a commit with author and committer `linus@example.org`
- **THEN** the commit passes the identity check
