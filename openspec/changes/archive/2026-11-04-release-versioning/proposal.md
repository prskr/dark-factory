# Proposal: release versioning

## Why

Dark Factory has no release that a user could install: the workspace and the chart are stuck at `0.1.0`, the chart is not published, and a release runs only on a hand-pushed `v*` tag that is created before anything is built. The project is not stable and may never promise stability, so a date-based version tells a user how far behind an installation is better than a SemVer number that suggests compatibility guarantees. The docs site (follow-up change `docs-site`) needs releases to version its docs against.

## What Changes

- **Version scheme `YYYY.M.N`**: year, month without leading zero, and a counter of releases within that month starting at `0` (`2026.11.0`, `2026.11.1`, `2026.12.0`). Tags are `v<version>`. The version is valid SemVer without a pre-release suffix, so Cargo, Helm and OCI tooling order it correctly and `helm install` picks it without `--devel`.
- **Build-time stamping**: the workspace `Cargo.toml` and `Chart.yaml` hold the placeholder `0.0.0`. A release injects the version as `DF_VERSION` into the Bazel build; one helper replaces the three uses of `CARGO_PKG_VERSION`. Unstamped builds (plain `cargo`, dev deploys) report `dev`. **BREAKING**: OTel `service.version` and the runtime version reported to the control plane become `dev` for unstamped builds instead of `0.1.0`.
- **Release workflow**: one manually started workflow without inputs computes the next version from the existing tags, builds and pushes all images and the Helm chart, and only then creates the tag and a GitHub Release with generated notes. A failed run leaves no tag. **BREAKING**: pushing a `v*` tag no longer starts a release.
- **Image tags**: first-party images (control plane, runtime, plugins, test project, reference project `claude-project`) are tagged with the release version only, without `v` prefix. The floating `latest` tag is no longer pushed. `claude-harness` keeps its adapter-based tag. **BREAKING**: `claude-project:latest` and `claude-harness:latest` stop moving.
- **Helm chart in OCI**: the chart is pushed to `oci://ghcr.io/prskr/dark-factory/charts/dark-factory` with `version` = `appVersion` = the release version. The default image tags in `values.yaml` become empty, so every first-party image follows `appVersion`.
- **Version in the UI**: the rail's bare log-out button is replaced by the profile menu that phones already have (avatar with initials, name, role, log out); the menu gets a footer line `Dark Factory <version>`. One menu serves phone and desktop. The rail design is updated in Penpot first.
- **Docs**: a contributor guide `docs/releasing.md` (cutting a release, workflow diagram, version computation, one-time setup of DNS, GitHub Pages and GHCR visibility) and a tech-debt entry on whether the chart defaults should reference the test project image.

## Capabilities

### New Capabilities
- `release-process`: the version scheme, how the next version is computed, what a release builds and publishes (images, Helm chart, tag, GitHub Release) and in which order.

### Modified Capabilities
- `build-pipeline`: the reference project image is tagged with the release version, and no `latest` tag is pushed for it or the harness image.
- `helm-deployment`: first-party image tags default to the chart `appVersion`.
- `web-ui-shell`: the rail ends with the profile menu instead of a log-out button; the profile menu shows the version.
- `telemetry`: `service.version` is the release version, or `dev` for unstamped builds.

## Out of scope

- Versioned docs site, the docs link in the profile menu and the publishing of docs during a release: change `docs-site` (follow-up, next change).
- A "Settings" entry in the profile menu: iteration 5 (configurable projects), where settings pages land.
- Changelog file or changelog page: rejected for now; GitHub's generated release notes are enough pre-MVP.
- Signing images and the chart (cosign) and SBOMs: backlog.
- Pruning old images and chart versions from GHCR: backlog.
- Version compatibility checks between control plane and runtime: rejected; both ship from one release and the runtime version is informational.

## Impact

- `.github/workflows/release.yaml` (rewritten), `.github/workflows/harness-images.yaml` (`claude-project` tags)
- `Cargo.toml` (workspace version), `deploy/helm/dark-factory/Chart.yaml`, `deploy/helm/dark-factory/values.yaml`, chart unit tests
- `deploy/BUILD.bazel` (`image_tag` flag replaced by a version flag), Bazel rust targets that read `DF_VERSION`
- `crates/df-telemetry` (version helper, `service.version`), `crates/factory-agent/src/connection.rs` (`runtime_version`), `crates/df-web/src/components/shell.rs` and the layout that renders the rail, `crates/dark-factory` (passes the version to the web UI)
- Penpot board of the navigation rail
- `docs/releasing.md` (new), `docs/tech_debt.md`, `product/iterations/roadmap.md` (backlog rows for the out-of-scope follow-ups)
