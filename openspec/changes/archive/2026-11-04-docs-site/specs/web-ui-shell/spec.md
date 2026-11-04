# Spec Delta

## ADDED Requirements

### Requirement: Docs link in the profile menu
The profile menu SHALL contain a Docs entry above Log out that opens `https://www.dark-factory.eu/docs/<version>/` in a new tab, with `<version>` the version the control plane reports, or `https://www.dark-factory.eu/docs/dev/` for an unstamped build. The entry SHALL show an external-link icon and its accessible name SHALL say that it opens in a new tab.

#### Scenario: Released instance
- **WHEN** a board member on an instance running `2026.11.0` chooses Docs in the profile menu
- **THEN** `https://www.dark-factory.eu/docs/2026.11.0/` opens in a new tab and the UI tab stays open

#### Scenario: Dev build
- **WHEN** the instance was built without a version
- **THEN** the Docs entry links to `https://www.dark-factory.eu/docs/dev/`
