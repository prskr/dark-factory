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
With `E2E_KEEP=1`, the runner SHALL keep the cluster, the Forgejo users and the repository after the run. A later run SHALL reuse a healthy kept cluster: it SHALL skip cluster creation and infrastructure installation, create new tokens for the existing Forgejo users, load the current images, upgrade the release, seed and run the tests. Without a healthy kept cluster, it SHALL create a new one. `E2E_FRESH=1` SHALL force a new cluster.

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

#### Scenario: Tokens on a kept cluster
- **WHEN** a run reuses a kept cluster
- **THEN** the bot and the reviewer get new tokens, the tokens of the previous run no longer authenticate, and the sandboxes' credential reference holds the new bot token

#### Scenario: Cluster kept by an older runner
- **WHEN** a run finds a kept cluster whose state still holds the tokens of an earlier runner version
- **THEN** the run reuses the cluster and ignores the stored tokens

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

### Requirement: Tests independent of each other
Every test SHALL pass alone, in any order and in parallel with any other test of the suite. A test SHALL NOT change data that another test reads or writes: shared fixtures (the seeded projects, persona and model providers, the base branch and the issues of a shared repository) stay as the seed and the runner wrote them.

#### Scenario: Parallel tests on one repository
- **WHEN** two tests run at the same time and both work with the repository of the seeded projects
- **THEN** neither commits to, merges into or rewrites the base branch of that repository, and neither test's result depends on the other's progress

#### Scenario: Scenario that has to move a base branch
- **WHEN** a test checks behavior that needs a commit or a merge on a project's base branch
- **THEN** it works on a project and repository that only the tests of that scenario use

#### Scenario: Settings test
- **WHEN** a test that saves a settings form of a seeded project or model provider passes
- **THEN** the saved record has the values it had before the test

### Requirement: Repeatable tests
A test SHALL pass when it runs a second time against the same instance without a reset. Names a test gives to records outside its task (branches, files, pull requests, issues, personas, models, subscriptions) SHALL be unique per test run, and a test SHALL find its records by what it created, never by a predicted key or position.

#### Scenario: Second run on a kept cluster
- **WHEN** a developer runs the same test twice in a row against one instance
- **THEN** both runs pass

#### Scenario: Child tasks of a coordinator
- **WHEN** a test checks the subtasks a coordinator task created
- **THEN** it finds them through the coordinator it created, whatever keys the project has handed out before

### Requirement: Attributable assertions
Every check of a test SHALL be limited to records of that test: its tasks, runs, requests, branches, pull requests and issues. A check SHALL NOT count or search records that other tests can write.

#### Scenario: Audit entry of an interrupt
- **WHEN** the interrupt test checks that its interrupt was audited
- **THEN** it looks for the entry of its own run, and an entry of another run does not make it pass

#### Scenario: Pull request of a task
- **WHEN** the repository holds more pull requests than one API page returns
- **THEN** a test still finds the pull request of its task by the task's branch

### Requirement: Cleanup of a test's tasks
After a test ends, passed or failed, the suite SHALL cancel, through the control plane, every task the test created that is not in a final status, before the test's result is reported. When the control plane does not accept the cancellation, the test SHALL fail and name the tasks it left behind.

#### Scenario: Failed test with a running task
- **WHEN** a test fails while its task's run is still waiting for a network decision
- **THEN** the task is cancelled before the test's result is reported, and its sandbox is released

#### Scenario: Cancellation not possible
- **WHEN** neither the UI nor a direct request to the control plane can cancel a task of the test
- **THEN** the test fails, and its error names the task that is still active

#### Scenario: Passed test
- **WHEN** a test passes and its task is waiting for a follow-up
- **THEN** the task is cancelled

### Requirement: Failure diagnostics
When a test fails, the suite SHALL save a screenshot of its main page and its browser console log in a directory per test. It saves no browser trace: the browser runs on a remote server, which writes a trace on its own disk, not on the test host. When the run fails, the runner SHALL save the logs of the control plane, the gateway and the sandbox pods into the same artifacts directory, and CI SHALL keep that directory with the failed job.

#### Scenario: Failed UI assertion
- **WHEN** a test fails waiting for a text on the task page
- **THEN** the artifacts directory has the test's screenshot and console log, and the error names the text and the element waited for

#### Scenario: Failed CI job
- **WHEN** the e2e job fails in CI
- **THEN** the job's artifacts hold the per-test files and the cluster logs

### Requirement: Fail-fast waits
A test waiting for a task status SHALL stop as soon as the task reaches a final status other than the awaited one, and SHALL name the status it found. Waits SHALL be bounded by a limit that fits what they wait for, and every call to a service in the cluster SHALL have a time limit.

#### Scenario: Run fails while a test waits for waiting
- **WHEN** a test waits for its task to be `waiting` and the task becomes `failed`
- **THEN** the test fails within one poll interval and its error names `failed`

### Requirement: Host access to cluster services
The runner SHALL make PostgreSQL, Forgejo, the Playwright server and the event receiver reachable from the test host on fixed ports bound to `127.0.0.1`, without processes that forward the connections. The connections SHALL survive a restart of the pods behind them. A kept cluster without this access SHALL be recreated.

#### Scenario: Pod restart during a run
- **WHEN** the Playwright server pod restarts between two tests
- **THEN** the next test connects through the same host port once the new pod is ready

#### Scenario: Not reachable from the network
- **WHEN** another machine on the network connects to the host on one of these ports
- **THEN** the connection is refused

#### Scenario: Kept cluster from before
- **WHEN** a run finds a kept cluster that was created without the port mappings
- **THEN** the runner deletes it and creates a new cluster

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

### Requirement: Forgejo setup through the API
The runner SHALL create the Forgejo users, their tokens, the repositories, the issue, the hidden file, the collaborator and the webhooks through the Forgejo HTTP API, with the user, repository and webhook secret names the suite's fixtures use. Running the setup again on the same Forgejo SHALL succeed and leave the same users and repositories.

#### Scenario: Fresh Forgejo
- **WHEN** the setup runs against a Forgejo that has only the admin user
- **THEN** the users `factory` and `reviewer` exist, each has one token, and the repositories `factory/demo` and `factory/existing-branch` exist with the reviewer as collaborator with write access and a webhook to the control plane signed with the suite's webhook secret

#### Scenario: Setup runs twice
- **WHEN** the user setup runs a second time against the same Forgejo
- **THEN** it succeeds, the users still exist, and each has exactly one token, the new one

#### Scenario: Repository reset with a repository missing
- **WHEN** the repository reset runs and one of the suite's repositories does not exist
- **THEN** the reset deletes the other one, creates both again and succeeds

#### Scenario: Forgejo refuses a request
- **WHEN** Forgejo answers a setup request with an error other than "already exists" or, on delete, "not found"
- **THEN** the setup fails, naming the request and the HTTP status, and prints no token

### Requirement: Playwright version check
Before it sets up Forgejo or runs a test, the runner SHALL check that the Playwright version of the browser server image, of its base image and of the test driver are the same, and SHALL stop when they differ.

#### Scenario: Versions differ
- **WHEN** the locked test driver uses a different Playwright version than the browser server image
- **THEN** the runner exits non-zero before the Forgejo setup and before any test, and names each of the three versions and where it comes from

#### Scenario: Version not found
- **WHEN** one of the three versions cannot be read
- **THEN** the runner exits non-zero and names the file it could not read the version from
