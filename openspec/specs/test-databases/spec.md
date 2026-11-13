# test-databases Specification

## Purpose
Defines the lifecycle of the per-test PostgreSQL databases that the test utility crate creates in the shared test container, so repeated test runs don't fill the container storage.

## Requirements

### Requirement: Finished test databases are dropped during the run
While a test run creates databases, a test database SHALL be dropped at a later database creation once its test released its pool and it is older than a short grace period (default 1 minute), not only at the next server start. Databases released after the run's last creations MAY stay until the next server start. A database in use SHALL NOT be dropped, also when several test processes share the container.

#### Scenario: Long test run
- **WHEN** one test run creates 700 test databases one after another
- **THEN** at its end, only the databases of tests still running at the last sweep and those created within the grace period and sweep interval before it remain, not hundreds
- **AND** the next run's server start removes them

#### Scenario: Concurrent test processes
- **WHEN** two test binaries run in parallel against the shared container
- **THEN** neither drops a database the other still has connections to

### Requirement: A held pool keeps its database in use
A database SHALL count as in use while its test holds the pool, also when the test sends no query for a long time. A test pool SHALL keep a connection open for its whole lifetime, so that a database without connections has no pool.

#### Scenario: Idle test
- **WHEN** a test holds its pool without a query for longer than the grace period and the pool's idle timeout default (10 minutes)
- **THEN** its database still exists and the next query succeeds

### Requirement: Leftovers are removed at server start
At server start, test databases and temporary template databases left by earlier runs SHALL be removed when they have no connections, as today, so an aborted run doesn't leave them behind for long.

#### Scenario: Aborted run
- **WHEN** a test run was killed and left 300 test databases
- **THEN** the next run's server start removes those that have no connections
