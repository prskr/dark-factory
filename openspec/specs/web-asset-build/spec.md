# web-asset-build Specification

## Purpose

Defines how the web UI's own scripts and stylesheet are built from TypeScript and Tailwind sources, type-checked, and embedded into the control plane, with and without Bazel.

## Requirements

### Requirement: Scripts are compiled from TypeScript
The web UI's own scripts SHALL be written in TypeScript. Each source file SHALL compile to one minified JavaScript file, served under the source's name with the `.js` extension. Sources SHALL NOT be bundled or merged. Vendored third-party scripts SHALL be served as checked in.

#### Scenario: Component compiled
- **WHEN** the control plane is built with the asset tools available and a page uses the `tabs` component
- **THEN** the page loads `/assets/tabs.js`, the compiled form of the `tabs` TypeScript source, without type annotations

#### Scenario: Vendored script unchanged
- **WHEN** the control plane is built
- **THEN** `/assets/vendor/alpine.min.js` serves the checked-in file byte for byte

### Requirement: Assets are built by pinned tools
The TypeScript compiler and `tailwindcss` SHALL be pinned tools, available both to Bazel and on the developer `PATH`. Bazel builds SHALL always compile the assets with the pinned versions. Release images and binaries SHALL be built this way.

#### Scenario: Bazel build
- **WHEN** `bazel build //crates/dark-factory` runs on a machine without the tools on `PATH`
- **THEN** the binary embeds the compiled scripts and stylesheet

### Requirement: Plain cargo build without asset tools
A plain `cargo` build SHALL compile the assets with the tools from `PATH` when they are present. When a tool is missing, the build SHALL succeed, print a warning naming the tool, and embed none of that tool's outputs. No compiled asset SHALL be checked into the repository.

#### Scenario: Compiler missing
- **WHEN** `cargo build -p df-web` runs without the TypeScript compiler on `PATH`
- **THEN** the build succeeds with a warning naming the compiler, and the component scripts are not embedded

#### Scenario: Tools present
- **WHEN** `cargo build -p df-web` runs with the pinned tools on `PATH`
- **THEN** the embedded scripts and stylesheet match those of the Bazel build

### Requirement: Tests of compiled assets are not silently skipped
A test that needs compiled scripts or the compiled stylesheet SHALL run whenever the assets it reads were compiled, regardless of the other kind. When they were not, it SHALL be reported as ignored, with a reason naming the missing assets. Tests of a component's code SHALL read its TypeScript source and run in every build.

#### Scenario: Cargo test without tools
- **WHEN** `cargo test -p df-web` runs without the asset tools on `PATH`
- **THEN** the tests that need compiled assets are listed as ignored and all other tests pass

#### Scenario: Only the stylesheet tool missing
- **WHEN** `cargo test -p df-web` runs with the TypeScript compiler but without `tailwindcss` on `PATH`
- **THEN** the tests of compiled scripts run, and only the tests of the compiled stylesheet are listed as ignored

#### Scenario: Bazel test
- **WHEN** `bazel test //crates/df-web/...` runs
- **THEN** the tests that need compiled assets run

### Requirement: Scripts are type-checked
The TypeScript sources SHALL be type-checked in strict mode by the pre-commit hook and in CI, with a pinned compiler version. Alpine's API used by the components SHALL be typed by a declaration file in the repository. A type error SHALL fail the check.

#### Scenario: Type error
- **WHEN** a component calls a method that its Alpine component type does not declare
- **THEN** the pre-commit type check and the CI job fail and name the file and line
