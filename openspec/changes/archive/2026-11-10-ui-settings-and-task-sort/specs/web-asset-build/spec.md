# Spec Delta

## MODIFIED Requirements

### Requirement: Scripts are compiled from TypeScript
The web UI's own scripts SHALL be written in TypeScript. Each source file SHALL compile to one minified JavaScript file, served under the source's name with the `.js` extension. Sources SHALL NOT be bundled or merged. Third-party scripts SHALL be served byte for byte as published in their npm package.

#### Scenario: Component compiled
- **WHEN** the control plane is built with the asset tools available and a page uses the `tabs` component
- **THEN** the page loads `/assets/tabs.js`, the compiled form of the `tabs` TypeScript source, without type annotations

#### Scenario: Vendored script unchanged
- **WHEN** the control plane is built
- **THEN** `/assets/vendor/alpine.min.js` serves `dist/cdn.min.js` of the locked `alpinejs` byte for byte

### Requirement: Plain cargo build without asset tools
A plain `cargo` build SHALL compile the assets with the tools from `PATH` when they are present, and embed the third-party scripts from `VENDOR_SCRIPTS` or the crate's installed npm packages when present. When a tool or a third-party script is missing, the build SHALL succeed, print a warning naming it, and embed none of that kind. No compiled asset and no third-party script SHALL be checked into the repository.

#### Scenario: Compiler missing
- **WHEN** `cargo build -p df-web` runs without the TypeScript compiler on `PATH`
- **THEN** the build succeeds with a warning naming the compiler, and the component scripts are not embedded

#### Scenario: Tools present
- **WHEN** `cargo build -p df-web` runs with the pinned tools on `PATH`
- **THEN** the embedded scripts and stylesheet match those of the Bazel build

#### Scenario: Third-party scripts not installed
- **WHEN** `cargo build -p df-web` runs without `VENDOR_SCRIPTS` and without `crates/df-web/node_modules`
- **THEN** the build succeeds with a warning naming the missing scripts and the install command, and nothing under `/assets/vendor/` is served

### Requirement: Tests of compiled assets are not silently skipped
A test that needs compiled scripts, the compiled stylesheet or the third-party scripts SHALL run whenever the assets it reads were staged, regardless of the other kinds. When they were not, it SHALL be reported as ignored, with a reason naming the missing assets. Tests of a component's code SHALL read its TypeScript source and run in every build.

#### Scenario: Cargo test without tools
- **WHEN** `cargo test -p df-web` runs without the asset tools on `PATH`
- **THEN** the tests that need compiled assets are listed as ignored and all other tests pass

#### Scenario: Only the stylesheet tool missing
- **WHEN** `cargo test -p df-web` runs with the TypeScript compiler but without `tailwindcss` on `PATH`
- **THEN** the tests of compiled scripts run, and only the tests of the compiled stylesheet are listed as ignored

#### Scenario: Bazel test
- **WHEN** `bazel test //crates/df-web/...` runs
- **THEN** the tests that need compiled assets run

## ADDED Requirements

### Requirement: Third-party scripts from the npm lockfile
Mermaid, htmx with its SSE extension, and Alpine SHALL be pinned in `crates/df-web/package.json` with a committed pnpm lockfile, so that updates change only those two files. Bazel SHALL fetch them from the lockfile and embed their published dist files under `/assets/vendor/`; a Bazel build without one of them SHALL fail. No install script SHALL run.

#### Scenario: Bazel build embeds the locked versions
- **WHEN** `bazel build //crates/dark-factory` runs
- **THEN** `/assets/vendor/mermaid.min.js`, `/assets/vendor/htmx.min.js` and `/assets/vendor/alpine.min.js` serve the dist files of the versions in `crates/df-web/pnpm-lock.yaml`, and the SSE extension is served from the same `htmx.org` version

#### Scenario: Version update
- **WHEN** Renovate bumps `mermaid` in `crates/df-web/package.json`
- **THEN** the pull request changes `package.json` and `pnpm-lock.yaml`, and the next Bazel build embeds the new version without other edits

#### Scenario: No change in the browser at the switch
- **WHEN** the first lockfile is committed
- **THEN** every served third-party script equals the file that was checked in before, or the review lists the difference
