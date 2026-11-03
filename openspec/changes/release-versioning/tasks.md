# Tasks

No container-backed tests are needed. The release workflow itself can only be verified on GitHub (task 6.2).

## 1. Version computation

- [ ] 1.1 Write failing unit tests in `tools/xtask` for `next_version(used, year, month)`, then implement it (design D1). Verify with `cargo test` in `tools/xtask`. The tests cover the `Calendar version scheme` scenarios:
  - no tag of the month → `.0`;
  - `v2026.11.0`, `v2026.11.1` → `2026.11.2`;
  - `v2026.11.9`, `v2026.11.10` → `2026.11.11` (numeric, not lexical);
  - only `v2026.11.4` in December → `2026.12.0`;
  - a version used only by a GitHub Release (tag deleted) still counts: release `v2026.11.2` and tag `v2026.11.1` → `2026.11.3`;
  - unparsable names (`v1.2.3-rc1`, `foo`) and versions of other months are ignored.
- [ ] 1.2 Add `cargo xtask next-version`: reads `git tag --list 'v*'`, the GitHub Release tag names (`gh release list --json tagName`, drafts included) and today's UTC date, and prints the version. Verify by running it in the repository (prints `<year>.<month>.0`).

## 2. Version stamping

- [ ] 2.1 Write a failing test in `df-telemetry` that `VERSION` is `dev` without `DF_VERSION` (and a test of the mapping function that an empty value also gives `dev`), then add `df_telemetry::VERSION` (design D3). Verify with `cargo test -p df-telemetry`.
- [ ] 2.2 Replace the three `env!("CARGO_PKG_VERSION")` uses with `df_telemetry::VERSION` (`service.version` in `otel.rs`, `runtime_version` in `factory-agent`). Verify with `grep -rn CARGO_PKG_VERSION crates/` (no matches) and an `otel` test that the resource carries `service.version = "dev"`.
- [ ] 2.3 Add the `//build:version` string flag and the env-file rule, wire it into `df-telemetry`'s `rust_library` via `rustc_env_files`, and switch the `image_push` targets from `//deploy:image_tag` to it; remove `image_tag` (design D2). Verify:
  - `bazel --bazelrc=.bazelrc.agents build //crates/dark-factory --//build:version=2026.11.0` produces a binary whose OTel resource reports `2026.11.0`, checked by a Bazel test that sets the flag through a transition or `env`;
  - the build without the flag reports `dev`.
- [ ] 2.4 Set the workspace `version` in `Cargo.toml` to `0.0.0`. Verify that `cargo build --workspace` and `cargo xtask check-crates` pass.
- [ ] 2.5 Document `DF_VERSION` (build-time variable, optional, default `dev`) in the `telemetry` reference (`docs/telemetry.md`, `service.version` row). Verify that the doc names the flag and the cargo variant.

## 3. Helm chart

- [ ] 3.1 Write failing `helm unittest` cases for the `First-party image tags follow the chart version` scenarios: default values render `control-plane:<appVersion>`, `runtime:<appVersion>` and `test-project:<appVersion>`; a set tag wins. Use a fixed `chart.appVersion` in the suite.
- [ ] 3.2 Set `version` and `appVersion` in `Chart.yaml` to `0.0.0` and the three first-party image tags in `values.yaml` to `''`. Fix the remaining tests that assert `0.1.0` (`tests/plugins_test.yaml`) and the chart `README.md`. Verify with the Helm lint and unit-test hooks (`lefthook run pre-commit`, or the chart test scripts in `deploy/`).

## 4. Profile menu with version

- [ ] 4.1 Update the Penpot rail board: the avatar at the bottom of the rail replaces the log-out button, the menu opens to its right, and the menu shows the footer `Dark Factory <version>` (phone and rail). Verify by exporting the board and attaching the export to the PR.
- [ ] 4.2 Write failing `df-web` tests:
  - the rail has no `hx-post="/logout"` button outside the menu, and renders the avatar trigger with an accessible name;
  - `profile_menu` renders `Dark Factory 2026.11.0` when given that version, and `Dark Factory dev` for `dev`;
  - the phone menu and the rail menu render the same entries.
- [ ] 4.3 Implement design D6: `profile_menu(display_name, version)`, rail bottom, and the version passed from `dark-factory` into the web UI state. Verify with `cargo test -p df-web` and a browser check at 1440 px and 390 px (`just serve-local` if a container socket exists, otherwise state that it was not checked).
- [ ] 4.4 Update `docs/design-system.md` (rail and profile menu section). Verify that the doc matches the Penpot board.

## 5. Release workflow

- [ ] 5.1 Rewrite `.github/workflows/release.yaml` as in design D4:
  - `workflow_dispatch` only, `concurrency: release`;
  - a guard on `refs/heads/main`;
  - jobs `version`, `images`, `reference-images` (outputs the harness digest), `chart` (writes the digest into the packaged `values.yaml`), and `publish` with `gh release create --generate-notes`.

  Verify with `actionlint` if available, otherwise by review. Then run it once from a branch to confirm that the guard fails before any push.
- [ ] 5.2 Remove release-time tagging of `claude-project` and the `latest` tags from `harness-images.yaml`, leaving its PR checks. Verify that the PR job still runs on a PR that touches `deploy/images/claude-project`.

## 6. Docs and integration

- [ ] 6.1 Write `docs/releasing.md`, linked from `docs/development.md`. Verify that every command in it ran during 6.2. Contents:
  - how to start a release;
  - the workflow as a Mermaid diagram;
  - how the version is computed;
  - one-time setup: DNS apex A/AAAA records and `www` CNAME to `prskr.github.io` for `www.dark-factory.eu`, Pages "Deploy from a branch" `gh-pages` with Enforce HTTPS, making the chart and image packages public, deleting the stale `latest` tags.
- [ ] 6.2 Run the first release from `main`. Verify:
  - tag `v<year>.<month>.0` and its GitHub Release exist;
  - the control plane, runtime, plugin, test project and `claude-project` images exist with the release version as tag, and `claude-harness` with its adapter version;
  - `helm template oci://ghcr.io/prskr/dark-factory/charts/dark-factory --version <version>` renders the release version for the first-party images and the harness by the digest the release pushed;
  - the profile menu of a deployment shows the version.
- [ ] 6.3 Add the tech-debt entry "chart default values reference the test project image" to `docs/tech_debt.md`. Add the backlog rows from the proposal's Out of scope to `product/iterations/roadmap.md` (signing and SBOMs, GHCR pruning) and the Settings entry to iteration 5. Verify with `openspec validate release-versioning --strict` and `typos`.

## Workflow follow-up

- Implement change `docs-site` after this one; it adds the docs build to the release workflow and the "Docs" entry to the profile menu.
- Archive this change after review.
