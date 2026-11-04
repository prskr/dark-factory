# Spec Delta

## Purpose

Defines how the docs are published per Dark Factory version, so that an operator reads the docs of the release they installed and can move between versions without losing their place.

## ADDED Requirements

### Requirement: Version URL layout
The docs of `main` SHALL be served under `/docs/dev/`, the docs of each release under `/docs/<version>/` (without `v`), and a copy of the newest release under `/docs/latest/`. `/docs/` SHALL forward to `/docs/latest/`, or to `/docs/dev/` while no release exists.

#### Scenario: Before the first release
- **WHEN** no release exists and a visitor opens `/docs/`
- **THEN** the browser is forwarded to `/docs/dev/`

#### Scenario: After a release
- **WHEN** release `2026.11.0` is the newest release and a visitor opens `/docs/`
- **THEN** the browser is forwarded to `/docs/latest/`, which shows the docs of `2026.11.0`

### Requirement: Publishing per source
A push to `main` SHALL replace only `/`, `/docs/dev/` and the site-wide files it owns. A release SHALL add `/docs/<version>/`, replace `/docs/latest/` and rewrite `/versions.json`. Earlier release directories SHALL NOT change, and publishing SHALL run one at a time. Publishing a release older than the newest published release SHALL fail without changing the site.

#### Scenario: Release keeps older versions
- **WHEN** release `2026.11.1` is published after `2026.11.0`
- **THEN** `/docs/2026.11.0/` is unchanged and `/docs/latest/` shows `2026.11.1`

#### Scenario: Older release published again
- **WHEN** `2026.11.1` is the newest published release and the docs of `2026.11.0` are published again
- **THEN** publishing fails, and `/docs/latest/` and `/versions.json` still show `2026.11.1`

#### Scenario: Push to main
- **WHEN** a change to the landing page is merged to `main`
- **THEN** `/` and `/docs/dev/` are rebuilt and no `/docs/<version>/` directory changes

### Requirement: Versions index
`/versions.json` SHALL list `dev` and every published release, newest first, and mark the newest release as latest.

#### Scenario: Index after two releases
- **WHEN** `2026.11.0` and `2026.11.1` are published
- **THEN** `/versions.json` lists `dev`, `2026.11.1` (latest) and `2026.11.0`

### Requirement: Version switcher
Every docs page SHALL offer a version switcher built from `/versions.json` at page load, so that the docs of an older version list releases published after it. Switching SHALL open the same page path in the chosen version. A path that does not exist there SHALL lead to a not-found page that links to the start page of that version.

#### Scenario: Old docs see new releases
- **WHEN** a visitor opens `/docs/2026.11.0/` after `2026.12.0` was published
- **THEN** the switcher lists `2026.12.0`

#### Scenario: Same page in another version
- **WHEN** a visitor on `/docs/2026.11.0/operations/telemetry/` switches to `dev`
- **THEN** the browser opens `/docs/dev/operations/telemetry/`

#### Scenario: Page missing in the target version
- **WHEN** the visitor switches to a version that has no such page
- **THEN** the not-found page links to that version's docs start page

### Requirement: Outdated version banner
Docs of a release that is not the newest SHALL show a banner naming the newest release with a link to the same page under `/docs/latest/`. Docs under `/docs/dev/` SHALL show a banner that they describe unreleased changes.

#### Scenario: Old release
- **WHEN** a visitor opens `/docs/2026.11.0/` while `2026.11.1` is the newest release
- **THEN** a banner states that `2026.11.1` is the latest release and links to `/docs/latest/`

#### Scenario: Dev docs
- **WHEN** a visitor opens `/docs/dev/`
- **THEN** a banner states that these docs describe unreleased changes

### Requirement: Release publishes its docs
A release SHALL build the docs from the released commit and publish them after it has created its tag, so that no docs exist for a version without a tag. When publishing the docs fails, re-running that step SHALL publish them for the same version.

#### Scenario: Docs of a release
- **WHEN** release `2026.11.0` has finished
- **THEN** `/docs/2026.11.0/` exists and was built from the commit the tag `v2026.11.0` points to

#### Scenario: Tag creation fails
- **WHEN** a release fails while creating the tag `v2026.11.2`
- **THEN** no `/docs/2026.11.2/` exists and `/versions.json` does not list `2026.11.2`

#### Scenario: Docs publishing fails
- **WHEN** the tag `v2026.11.2` exists and publishing its docs failed
- **THEN** re-running the docs step publishes `/docs/2026.11.2/` and makes it latest
