# Spec Delta

## Purpose

Defines the persona identity: who a persona appears as in commits and in the UI (handle, display name, commit email, avatar), how it is validated, and which commits carry it.

## ADDED Requirements

### Requirement: Persona identity fields
Every persona SHALL have a handle, a display name and a commit email, all required. The handle SHALL match `^[a-z0-9][a-z0-9-]{0,38}$`. The display name SHALL be 1 to 100 characters without control characters or `<`, `>` and `\n`. The commit email SHALL be a single address `local@domain` without whitespace, `<`, `>` or `,`, at most 254 characters, and is stored lowercased.

#### Scenario: Valid identity
- **WHEN** a board member saves a persona with handle `reviewer`, display name "Code Reviewer" and email `Reviewer@Example.org`
- **THEN** the persona is stored with email `reviewer@example.org`

#### Scenario: Invalid handle
- **WHEN** a board member saves a persona with handle `Code Reviewer`
- **THEN** the form rejects it at the handle field and nothing is stored

#### Scenario: Display name with angle brackets
- **WHEN** a board member saves a persona with display name "Bot <root@example.org>"
- **THEN** the form rejects it at the display name field

#### Scenario: Missing email
- **WHEN** a board member saves a persona without a commit email
- **THEN** the form rejects it at the email field

### Requirement: Unique handle and commit email
No two personas SHALL share a handle or a commit email. Saving a persona whose handle or commit email another persona already uses SHALL fail and change nothing. Two personas MAY share a display name.

#### Scenario: Email taken
- **WHEN** persona `linus` has email `linus@example.org` and a board member saves persona `ada` with email `linus@example.org`
- **THEN** the form rejects it at the email field naming `linus`, and `ada` is unchanged

#### Scenario: Shared display name
- **WHEN** personas `reviewer` and `reviewer-go` both have display name "Reviewer"
- **THEN** both are stored

### Requirement: Generated avatar
A persona's avatar SHALL be generated from its identity without stored images or requests to other hosts. It shows the first letters of up to two words of the display name on a background color chosen deterministically from the persona's id. It SHALL have an accessible label with the display name.

#### Scenario: Initials
- **WHEN** persona `reviewer` has display name "Code Reviewer"
- **THEN** its avatar shows "CR" and has the label "Code Reviewer"

#### Scenario: Stable color
- **WHEN** persona `reviewer` is renamed to "Strict Reviewer"
- **THEN** its avatar shows "SR" on the same background color as before

#### Scenario: No external request
- **WHEN** a page showing persona avatars is loaded
- **THEN** the browser makes no request for an avatar to any host other than the Dark Factory instance

### Requirement: Commits carry the persona identity
Every commit created in a run's sandbox, by the harness or by the runtime (WIP snapshots), SHALL have author and committer name and email equal to the run's persona identity at the run's start, the moment the run is created. A run SHALL NOT start without a persona identity.

#### Scenario: Harness commit
- **WHEN** the agent of a run with persona `linus` ("Linus", `linus@example.org`) runs `git commit`
- **THEN** the commit's author and committer are `Linus <linus@example.org>`

#### Scenario: WIP snapshot
- **WHEN** the runtime pushes a WIP snapshot for the same run
- **THEN** the snapshot commit's author and committer are `Linus <linus@example.org>`

#### Scenario: Identity edited during a run
- **WHEN** a board member changes `linus`'s email to `linus@corp.example` while a run of `linus` is active
- **THEN** that run keeps committing as `linus@example.org`, and the next run commits as `linus@corp.example`

#### Scenario: Assign without identity
- **WHEN** the runtime receives an `Assign` without a commit identity
- **THEN** it fails the run before preparing the workspace and reports the missing identity

### Requirement: Identity migration
The upgrade SHALL give every existing persona an identity: the handle is the slug of the name (lowercased, other characters than `[a-z0-9]` collapsed to `-`, trimmed, cut to 39 characters). In creation order, a handle an earlier persona holds gets the next free suffix `-2`, `-3`, …, with the slug cut so both fit in 39 characters. The display name is derived from the old name ("Migrated display name"). The email is `<handle>@dark-factory.localhost`.

#### Scenario: Name with spaces
- **WHEN** persona "Code Reviewer" exists before the upgrade
- **THEN** afterwards it has handle `code-reviewer`, display name "Code Reviewer" and email `code-reviewer@dark-factory.localhost`

#### Scenario: Colliding slugs
- **WHEN** personas "Ada" and then "ADA!" exist before the upgrade
- **THEN** afterwards they have handles `ada` and `ada-2`

#### Scenario: Suffixed handle collides with another slug
- **WHEN** personas "Ada", "ADA!" and then "Ada-2" exist before the upgrade
- **THEN** afterwards they have handles `ada`, `ada-2` and `ada-2-2`

#### Scenario: Long colliding names
- **WHEN** a persona named with 39 `a` characters and then one with the same name followed by `!` exist before the upgrade
- **THEN** afterwards they have handles of 39 `a` characters and of 37 `a` characters followed by `-2`

#### Scenario: Name without usable characters
- **WHEN** persona "🤖" exists before the upgrade
- **THEN** afterwards it has handle `persona`, or `persona-2` on collision

### Requirement: Migrated display name
The upgrade SHALL derive a valid display name from each old name: control characters, `<` and `>` removed, trimmed and cut to 100 characters, or the handle if nothing is left. Old names were only required to be non-blank, so names that break the display-name rule can exist.

#### Scenario: Name that is not a valid display name
- **WHEN** persona "<Bot>" exists before the upgrade
- **THEN** afterwards it has handle `bot` and display name "Bot"

#### Scenario: Name of only angle brackets
- **WHEN** persona "<<>>" exists before the upgrade
- **THEN** afterwards it has handle `persona` and display name "persona"
