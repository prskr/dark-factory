# Proposal

## Why

The web UI's own scripts (five Alpine components, ~460 lines in `crates/df-web/assets/*.js`) have no type checking. The only checks are string-contains tests in Rust and the Playwright suite. Planned features add more client code, for example autocomplete of repository file and skill names in the chat. Type errors should fail before review, not in the browser.

The rule that everything builds with plain `cargo` prevents a TypeScript build step today. That rule exists so that rust-analyzer never depends on Bazel-generated code. Compiled JS and CSS are bytes embedded by `rust-embed`, not Rust source, so they don't need that protection. The same rule is also why the generated `assets/app.css` is checked in as a fallback for machines without `tailwindcss`.

## What Changes

- **Plain-cargo rule narrowed**:
  - Rust code, including generated Rust, still builds with plain `cargo` and rust-analyzer.
  - `cargo build`, `cargo test --workspace` and `cargo clippy --all-targets` still pass without non-Rust tools, apart from generators of Rust code (`protoc`, as today).
  - Compiled web assets (JS from TypeScript, Tailwind CSS) may need a pinned tool. A `cargo` build without that tool prints a warning and embeds no compiled asset.
- **TypeScript for the UI components**:
  - The Alpine components move from `assets/*.js` to TypeScript sources.
  - A pinned standalone compiler turns each source into one minified JS file with the same served name (`<name>.js`). There is no bundler; files are not merged.
  - Bazel builds them in a target. `build.rs` takes that output through an environment variable, or runs the tool from `PATH` under plain `cargo`.
- **Type checking**: `tsc --noEmit` over the TypeScript sources runs in the pre-commit hook and in CI. Alpine's API is typed by a small checked-in declaration file.
- **BREAKING (contributors)**: the checked-in `crates/df-web/assets/app.css` is removed. Tailwind CSS follows the same rule as the scripts: Bazel output, or `tailwindcss` from `PATH`, otherwise a warning and no stylesheet.
- **Tests that read compiled assets**: these run only when the assets were compiled. Otherwise they are reported as ignored, never as passed. Bazel always compiles the assets, so CI always runs them. Tests of component logic read the TypeScript source.
- **Contributor rules and docs**: the root `AGENTS.md`, `crates/df-web/AGENTS.md`, the `rust` skill and `docs/development.md` describe the narrowed rule, the TypeScript workflow and the new pinned tool.

Out of scope:

- Autocomplete of file and skill names in the chat. Backlog; row added when this change is archived.
- A JS unit test runner for component logic. Backlog idea; the Playwright suite covers behavior for now.
- Type checking with the native TypeScript compiler as a pinned standalone binary instead of `npx`. Backlog idea.
- Bundling, npm runtime dependencies, ES-module imports between components: rejected for now. Components stay self-contained classic scripts.
- Rebuilding or trimming the vendored `mermaid.min.js` (3.5 MB): rejected. It is already minified and only loaded on pages that show a diagram.

## Capabilities

### New Capabilities

- `web-asset-build`: how the web UI's scripts and stylesheet are produced from sources (TypeScript, Tailwind), type-checked, and embedded, and how a plain `cargo` build behaves without the asset tools.

### Modified Capabilities

- `build-pipeline`: "Cargo workspace stays the source of truth" is narrowed to Rust code. Compiled web assets may need pinned tools.

## Impact

- **Code**: `crates/df-web/build.rs`, `crates/df-web/BUILD.bazel`, `crates/df-web/src/assets.rs` (test helper, test gating), the component tests in `src/layout.rs`, `src/markdown*.rs` and others that use `script_source`, the five components (rewritten as `.ts`), new `tsconfig.json` and Alpine declarations.
- **Tooling**: one new pinned tool in `multitool.lock.json`, `tools/xtask` (`TOOLS`) and `//:bazel_env`. A pinned `typescript` version for `npx` in `lefthook.yml` and `.github/workflows/ci.yaml`, updated by a Renovate regex manager.
- **Repository**: `crates/df-web/assets/app.css` is deleted.
- **Users and operators**: none. Images and release binaries are built by Bazel and serve the same assets under the same URLs.
