## ADDED Requirements

### Requirement: Unambiguous locators
Every browser locator of the e2e suite SHALL match one element, so that Playwright's strict mode fails when a second match appears. Picking an element by position (first, last, n-th) SHALL be possible only in a helper whose name says what the position means. `cargo clippy` SHALL fail for a positional pick anywhere else in `crates/df-e2e`, except in files that opt out of the database-access ban, and CI SHALL run that check. Those files SHALL use no locator; review enforces this.

#### Scenario: New positional pick in a test
- **WHEN** a test file in `crates/df-e2e/tests/` without a database-access opt-out calls `.first()` on a Playwright locator
- **THEN** `cargo clippy -p df-e2e --all-targets --all-features -- -D warnings` fails and names the call

#### Scenario: Positional pick in a helper of `src/`
- **WHEN** a function in `crates/df-e2e/src/browser.rs` calls `.last()` on a Playwright locator without an opt-out
- **THEN** clippy fails, even though the crate opts out of the database-access ban in other modules

#### Scenario: Helper that picks by position on purpose
- **WHEN** a helper named for the meaning of the position (for example the newest model row) calls `.last()` and carries the opt-out with a reason
- **THEN** clippy passes

#### Scenario: Control rendered for two layouts
- **WHEN** a page renders a button for the desktop layout and again for the phone layout, and a test addresses it by role and accessible name
- **THEN** the locator matches only the visible button, and the test passes without a positional pick

#### Scenario: Slice method with the same name
- **WHEN** the seed code calls `first()` on a `Vec` or a slice
- **THEN** clippy does not report it
