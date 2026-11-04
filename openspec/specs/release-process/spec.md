# release-process Specification

## Purpose
Defines how Dark Factory versions its releases, how the next version is chosen, and what one release builds and publishes, so that every installation can be traced to one dated release of matching images and chart.

## Requirements

### Requirement: Calendar version scheme
A release version SHALL have the form `YYYY.M.N`: the four-digit year and the month without leading zero of the release date (UTC), and `N`, one more than the highest `N` already used in that month, starting at `0`. A version counts as used when a tag `v<version>` or a GitHub Release `v<version>` (draft or published) exists, so a deleted tag does not free its version. Versions SHALL carry no pre-release suffix. The Git tag of a release SHALL be `v<version>`.

#### Scenario: First release of a month
- **WHEN** a release runs on 2026-11-03 and no tag `v2026.11.*` exists
- **THEN** the release version is `2026.11.0` and its tag is `v2026.11.0`

#### Scenario: Further release in the same month
- **WHEN** a release runs in November 2026 and the tags `v2026.11.0` and `v2026.11.1` exist
- **THEN** the release version is `2026.11.2`

#### Scenario: New month
- **WHEN** a release runs on 2026-12-01 and the newest tag is `v2026.11.4`
- **THEN** the release version is `2026.12.0`

#### Scenario: Tag of a published release deleted
- **WHEN** the tag `v2026.11.2` was deleted but the GitHub Release `v2026.11.2` still exists, and `v2026.11.1` is the newest remaining tag
- **THEN** the next release in November 2026 is `2026.11.3`

#### Scenario: Ordering across the tenth release
- **WHEN** the tags `v2026.11.9` and `v2026.11.10` exist
- **THEN** the next release in November 2026 is `2026.11.11`

### Requirement: Manually started release
A release SHALL be started manually without inputs and SHALL compute its version itself. It SHALL build only the head of `main` and SHALL fail before building when started on another branch. Pushing a tag SHALL NOT start a release. Two releases SHALL NOT run at the same time.

#### Scenario: Started on a feature branch
- **WHEN** a release is started on branch `docs/initial-setup`
- **THEN** it fails without pushing anything

#### Scenario: Tag pushed by hand
- **WHEN** someone pushes a tag `v2026.11.5`
- **THEN** no release workflow runs

### Requirement: Release artifacts
A release SHALL push the control plane, runtime, plugin, test project and reference project images for `linux/amd64` and `linux/arm64`, each tagged with the release version (no `v` prefix), and the Helm chart to `oci://ghcr.io/prskr/dark-factory/charts/dark-factory` with chart `version` and `appVersion` equal to the release version. A release SHALL NOT push a `latest` tag.

#### Scenario: Install a release
- **WHEN** release `2026.11.0` has finished and the chart and image packages are public (a one-time step after the first release)
- **THEN** `helm install df oci://ghcr.io/prskr/dark-factory/charts/dark-factory --version 2026.11.0` renders the control plane image `ghcr.io/prskr/dark-factory/control-plane:2026.11.0` and the runtime image `ghcr.io/prskr/dark-factory/runtime:2026.11.0`

#### Scenario: No floating tag
- **WHEN** release `2026.11.1` has finished
- **THEN** no first-party image tag `latest` was written by it

### Requirement: Harness pinned per release
The chart of a release SHALL reference the harness image by the digest the release pushed, so that a later push of the same adapter-version tag does not change what an earlier release installs.

#### Scenario: Harness pinned per release
- **WHEN** release `2026.11.0` pushed `claude-harness:0.95.0` and a later build pushes a changed image under the same tag
- **THEN** chart `2026.11.0` still renders the harness image by the digest pushed by release `2026.11.0`

### Requirement: Tag after publishing
A release SHALL create its Git tag and a GitHub Release with generated notes only after every image and the chart were pushed. A release that fails before that point SHALL leave no tag and no GitHub Release, and the next release SHALL reuse its version.

#### Scenario: Failed image push
- **WHEN** a release computing `2026.11.2` fails while pushing an image
- **THEN** no tag `v2026.11.2` exists and the next release again computes `2026.11.2`

#### Scenario: Successful release
- **WHEN** a release computing `2026.11.2` succeeds
- **THEN** the tag `v2026.11.2` points to the built commit and a GitHub Release `v2026.11.2` lists the pull requests merged since the previous tag

### Requirement: Stamped version
Binaries built by a release SHALL report the release version. Binaries built without a version (plain `cargo` builds, dev deploys) or with an empty one SHALL report `dev`.

#### Scenario: Release binary
- **WHEN** `dark-factory` from release `2026.11.0` starts
- **THEN** it reports the version `2026.11.0`

#### Scenario: Plain cargo build
- **WHEN** `dark-factory` is built with `cargo build` without `DF_VERSION`
- **THEN** it reports the version `dev`

#### Scenario: Empty version
- **WHEN** `dark-factory` is built with `DF_VERSION` set to an empty string
- **THEN** it reports the version `dev`
