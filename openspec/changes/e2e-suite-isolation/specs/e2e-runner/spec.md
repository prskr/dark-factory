# Spec Delta

## ADDED Requirements

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
When a test fails, the suite SHALL save a screenshot of each of its pages, its browser trace and its browser console in a directory per test. When the run fails, the runner SHALL save the logs of the control plane, the gateway and the sandbox pods into the same artifacts directory, and CI SHALL keep that directory with the failed job.

#### Scenario: Failed UI assertion
- **WHEN** a test fails waiting for a text on the task page
- **THEN** the artifacts directory has the test's screenshot, trace and console log, and the error names the text and the element waited for

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
