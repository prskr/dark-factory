# Spec Delta

## MODIFIED Requirements

### Requirement: Reuse of a kept cluster
With `E2E_KEEP=1`, the runner SHALL keep the cluster and the state the next run needs (Forgejo users and repository) after the run. A later run SHALL reuse a kept cluster that is healthy: it SHALL skip cluster creation and infrastructure installation, SHALL create new tokens for the existing Forgejo users, and SHALL load the current images, upgrade the release, seed and run the tests. A run that finds no kept cluster, or one that is not healthy, SHALL create a new cluster as before. `E2E_FRESH=1` SHALL force a new cluster.

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

## ADDED Requirements

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
