## MODIFIED Requirements

### Requirement: Scripts are compiled from TypeScript
The web UI's own scripts SHALL be written in TypeScript. Each source file SHALL compile to one minified JavaScript file, served under the source's name with the `.js` extension. Sources SHALL NOT be bundled or merged, with one exception: the Markdown editor bundle ("Editor bundle"). Third-party scripts SHALL be served byte for byte as published in their npm package.

#### Scenario: Component compiled
- **WHEN** the control plane is built with the asset tools available and a page uses the `tabs` component
- **THEN** the page loads `/assets/tabs.js`, the compiled form of the `tabs` TypeScript source, without type annotations

#### Scenario: Vendored script unchanged
- **WHEN** the control plane is built
- **THEN** `/assets/vendor/alpine.min.js` serves `dist/cdn.min.js` of the locked `alpinejs` byte for byte

## ADDED Requirements

### Requirement: Editor bundle
The Markdown editor SHALL be built from one TypeScript entry module and the CodeMirror packages into one minified classic script, served as `/assets/editor.js`, by a pinned `esbuild` that is available both to Bazel and on the developer `PATH`. Bazel builds SHALL always embed the bundle and SHALL fail without it. The bundle SHALL stay below 300 KiB gzip-compressed. The entry module SHALL be type-checked in strict mode like the other scripts.

#### Scenario: Bazel build
- **WHEN** `bazel build //crates/dark-factory` runs on a machine without `esbuild` on `PATH`
- **THEN** the binary serves `/assets/editor.js`, built from the CodeMirror versions in `crates/df-web/pnpm-lock.yaml`

#### Scenario: Bundle grows too large
- **WHEN** a change makes the gzip-compressed bundle 300 KiB or larger
- **THEN** a test fails and names the size

### Requirement: CodeMirror packages from the npm lockfile
The CodeMirror packages of the editor bundle SHALL be pinned in `crates/df-web/package.json` and locked in its pnpm lockfile, so that an update changes only those two files. No install script SHALL run.

#### Scenario: CodeMirror update
- **WHEN** Renovate bumps `@codemirror/view` in `crates/df-web/package.json`
- **THEN** the pull request changes only `package.json` and `pnpm-lock.yaml`, and the next Bazel build embeds the new version

### Requirement: Plain cargo build without the editor bundle
A plain `cargo` build SHALL build the editor bundle with `esbuild` from `PATH` (or `ESBUILD_BIN`) when the CodeMirror packages are installed in `crates/df-web/node_modules`. When `esbuild` or the packages are missing, the build SHALL succeed, print a warning naming what is missing and how to get it, and embed no editor bundle; tests that read the bundle SHALL be reported as ignored. No bundle SHALL be checked into the repository.

#### Scenario: esbuild missing
- **WHEN** `cargo build -p df-web` runs with the CodeMirror packages installed but without `esbuild` on `PATH`
- **THEN** the build succeeds with a warning naming `esbuild`, and `/assets/editor.js` is not served

#### Scenario: Cargo test without esbuild
- **WHEN** `cargo test -p df-web` runs without `esbuild` on `PATH`
- **THEN** the tests of the editor bundle are listed as ignored and all other tests pass
