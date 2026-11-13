# e2e-runner Specification

## Purpose
Defines how `just e2e-local` runs the end-to-end suite against a local kind cluster: which tests run, which prerequisites are checked, how a kept cluster is reused and how many tests run at once.

## Requirements

### Requirement: Test discovery
The runner SHALL run every test binary under `crates/df-e2e/tests/`, except binaries listed in a documented exclusion list with a reason. A new test file SHALL run without changing the runner.

#### Scenario: New test file
- **WHEN** a developer adds a new file under `crates/df-e2e/tests/` and runs `just e2e-local`
- **THEN** its tests run without any change to the runner

#### Scenario: Excluded binary
- **WHEN** a test binary is on the exclusion list
- **THEN** it is built but not run, and the runner prints that it was skipped with the reason

#### Scenario: List mode
- **WHEN** a developer runs `E2E_LIST=1 just e2e-local`
- **THEN** the runner prints the selected and the skipped binaries with their reasons and exits without a cluster step

### Requirement: Free disk check
Before creating or reusing a cluster, the runner SHALL check the free space of the container runtime's storage. Below the threshold (default 10 GiB, overridable), it SHALL stop before any cluster change with a message that names the free space, the threshold and the cleanup of the shared test database container.

#### Scenario: Disk nearly full
- **WHEN** the container storage has 3 GiB free
- **THEN** the runner exits non-zero before creating a cluster, naming 3 GiB, the threshold and the cleanup command

#### Scenario: Storage not on this host
- **WHEN** the container runtime's storage is not on this host (Docker Desktop's VM, a remote daemon)
- **THEN** the runner prints a warning that the disk check was skipped and continues

#### Scenario: Disk nearly full with a kept cluster
- **WHEN** a kept cluster exists, the container storage is below the threshold and `E2E_KEEP` is not set
- **THEN** the runner exits non-zero and the kept cluster still exists

#### Scenario: Enough space
- **WHEN** the container storage has 25 GiB free
- **THEN** the runner continues without a message about disk space

### Requirement: Reuse of a kept cluster
With `E2E_KEEP=1`, the runner SHALL keep the cluster and the state the next run needs (Forgejo users, tokens and repository) after the run. A later run SHALL reuse a kept cluster that is healthy: it SHALL skip cluster creation, infrastructure installation and Forgejo setup, and SHALL load the current images, upgrade the release, seed and run the tests. A run that finds no kept cluster, or one that is not healthy, SHALL create a new cluster as before. `E2E_FRESH=1` SHALL force a new cluster.

#### Scenario: Second run on a kept cluster
- **WHEN** a run with `E2E_KEEP=1` finished and the developer runs `E2E_KEEP=1 just e2e-local` again after changing code
- **THEN** the cluster is not recreated, the changed images are loaded, the release is upgraded, the control plane pods run the new images and the tests run against the new code

#### Scenario: Clean state on a kept cluster
- **WHEN** a run starts on a kept cluster that earlier runs left with tasks, sandbox volumes and task branches
- **THEN** its tests start without tasks, sandbox pods, volumes or task branches from earlier runs, so repeated runs neither collide on branch names nor fill the sandbox quota

#### Scenario: Kept cluster broken
- **WHEN** a kept cluster exists but its Forgejo is not reachable
- **THEN** the runner deletes it and creates a new cluster

#### Scenario: Forced fresh cluster
- **WHEN** a kept cluster exists and the developer runs with `E2E_FRESH=1`
- **THEN** the runner deletes it and creates a new cluster

### Requirement: Short scheduling waits in the suite
The e2e cluster's release values SHALL set short `scheduling` waits, and a test that waits for a scripted retry time SHALL use a fake agent script whose retry time is a few seconds, so that tests waiting for an automatic resume or a forge retry don't wait for production delays. A test that must act before the retry time keeps a longer one.

#### Scenario: Resume after a rate limit
- **WHEN** the rate-limit test's run reports a retry time
- **THEN** the task resumes within seconds of that time, and the test doesn't wait a minute or more for it

### Requirement: Parallel tests
The runner SHALL run tests of a binary with 2 threads by default; `E2E_TEST_THREADS` SHALL override it. The documented memory budget of the cluster SHALL cover the default.

#### Scenario: Default
- **WHEN** a developer runs `just e2e-local` without `E2E_TEST_THREADS`
- **THEN** the tests run with 2 threads

#### Scenario: Override
- **WHEN** a developer runs with `E2E_TEST_THREADS=1`
- **THEN** the tests run one at a time
