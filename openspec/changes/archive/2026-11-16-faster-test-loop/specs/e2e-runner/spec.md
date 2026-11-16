# Spec Delta

## ADDED Requirements

### Requirement: Shared login per test binary
The suite SHALL log in at the identity provider at most once per test binary while the session stays valid. Other tests of that binary SHALL start logged in with the stored session. A test whose stored session is no longer accepted SHALL log in again. The test of the login flow SHALL always log in through the identity provider.

#### Scenario: Several tests in one binary
- **WHEN** a test binary with several tests runs against one instance
- **THEN** the identity provider's login form is submitted once, and every test starts on the board as the seeded user

#### Scenario: Stored session no longer valid
- **WHEN** a test starts with a stored session that the control plane no longer accepts
- **THEN** the test logs in through the identity provider, passes as it would with a fresh login, and later tests of the binary use the new session

#### Scenario: Login test
- **WHEN** the login test runs after other tests of its binary have logged in
- **THEN** it still submits the identity provider's login form and lands on the board
