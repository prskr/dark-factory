# Design

## Context

- `crates/df-web/build.rs` copies `assets/` into `$OUT_DIR/assets/` and writes `app.css` there. `Assets` (`src/assets.rs`) embeds that folder with `rust-embed`.
- For `app.css`, Bazel runs the pinned `tailwindcss` (`@multitool`) in the `:app_css` genrule and passes the output through `TAILWIND_APP_CSS`. Plain `cargo` runs `tailwindcss` from `PATH` (or `TAILWIND_BIN`). If that fails, it falls back to the checked-in `assets/app.css`.
- Components are classic scripts that register on `alpine:init`. `COMPONENT_SCRIPTS` in `src/layout.rs` loads them before Alpine. They use `Alpine.data`, `this.$el`, `this.$refs`, `this.$watch` and `this.$nextTick`.
- Pinned tools come from GitHub releases through `cargo xtask update-tools` (`TOOLS` in `tools/xtask/src/main.rs`), `multitool.lock.json` and `//:bazel_env`. Developers get them on `PATH` with direnv.
- CI runs the Rust tests only through Bazel (`bazel test //...`). Plain `cargo` runs only in the xtask checks.
- The `openspec-validate` hook sets the precedent for an `npx` tool with a version pinned in two places and updated by Renovate.

## Goals / Non-Goals

**Goals:**

- One compiler and one invocation, shared by Bazel and plain `cargo`.
- Strict type checking that runs before review.
- No generated file in the repository.

**Non-Goals:**

- Type checking inside the Bazel build graph (`rules_ts`). Rejected: the lint is fast, and a pre-commit hook plus a CI job is enough.
- Source maps. Rejected for now: the compiled files are small and close to the source.
- Follow-ups in the proposal's Out of scope list (autocomplete, JS unit tests, native `tsc` binary) go to the backlog in `product/iterations/roadmap.md` when this change is archived.

## Decisions

### D1: swc as a standalone binary, run by a genrule and by `build.rs`

The swc CLI binary is pinned like `tailwindcss`: an entry in `TOOLS`, `multitool.lock.json` and `//:bazel_env`. A genrule in `crates/df-web/BUILD.bazel` compiles every `scripts/*.ts` with a checked-in `.swcrc` that strips types, targets ES2022 and minifies. Its outputs reach `build.rs` through `WEB_SCRIPTS`, a space-separated list of the compiled file paths (a genrule cannot declare a directory). Under plain `cargo`, `build.rs` runs the same command with `swc` from `PATH` (or `SWC_BIN`).

Alternatives:

- `rules_swc`: Bazel only. It brings the `rules_js` toolchain setup, and plain `cargo` would still need the binary, so the tool would be configured twice.
- esbuild: same model, but its binaries are published only on npm. `xtask update-tools` only reads GitHub releases. This is the fallback if D1's binary check fails (task 1.1).
- `tsc` emit: needs Node at build time and does not minify.
- `swc_core` in `build.rs`: no external tool, but it adds a large compile to every `cargo` build of `df-web`.

### D2: Layout and script shape

- Sources live in `crates/df-web/scripts/<name>.ts`, next to `styles/input.css`. The output is `$OUT_DIR/assets/<name>.js`, so the URLs and `COMPONENT_SCRIPTS` stay the same.
- `scripts/types/alpine.d.ts` declares only the Alpine API the components use: `Alpine.data`, and the magics `$el`, `$refs`, `$watch`, `$nextTick` through `ThisType`. It grows when a component needs more.
- `crates/df-web/tsconfig.json` sets `strict`, `noEmit`, `target`/`lib` ES2022 plus `DOM` and `DOM.Iterable`, and `moduleDetection: "force"`. The last one isolates each file's top-level names without turning it into an ES module.
- Files stay classic scripts with no `import`/`export`.

### D3: `build.rs` behaviour per asset

For each asset kind (scripts, stylesheet), `build.rs` decides in this order:

1. The Bazel env var is set (`WEB_SCRIPTS`, `TAILWIND_APP_CSS`): copy that output.
2. The tool runs (`SWC_BIN` / `TAILWIND_BIN`, otherwise `PATH`): compile.
3. The tool is not found: `cargo::warning`, nothing embedded for that kind.

A tool that is found but fails is a build error, as today. The fallback to a checked-in `app.css` is removed, and the file is deleted.

`DF_WEB_REQUIRE_ASSETS=1`, set in `build_script_env` in Bazel, turns case 3 into a build error. This keeps a Bazel misconfiguration from shipping an unstyled UI.

### D4: Gating tests on compiled assets with a cfg

`build.rs` emits one cfg per asset kind it compiled: `df_web_scripts` and `df_web_styles`, each with `rustc-check-cfg`. Each test that reads compiled output is gated only on the kind it reads, so a missing `tailwindcss` does not ignore a script test:

- Stylesheet tests (`app_css()`): `#[cfg_attr(not(df_web_styles), ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)")]`.
- Compiled script tests: `#[cfg_attr(not(df_web_scripts), ignore = "needs compiled scripts: swc on PATH (bazel run //:bazel_env)")]`.
- Tests that read both: `not(all(df_web_scripts, df_web_styles))`.

`cargo test` lists them as ignored, so they never pass silently.

`script_source` reads the TypeScript source from a test-only `RustEmbed` of `scripts/`. Component tests therefore always run.

Alternative: a cargo feature `compiled-assets`. Rejected: callers would have to know to enable it, and `--all-features` would fail without the tools.

### D5: Type checking with pinned `tsc` via `npx`

A lefthook command (glob `crates/df-web/scripts/**`, `crates/df-web/tsconfig.json`) and a CI step run `npx --yes -p typescript@<version> tsc -p crates/df-web`. Both copies of the version are updated by one Renovate regex manager, as for OpenSpec.

Alternatives:

- JSDoc with `checkJs`: no build step, but the syntax is clumsier. It was discussed and declined in favour of `.ts`.
- `rules_ts`: too much Bazel surface for a lint.

## Risks / Trade-offs

- [swc stops publishing the standalone CLI binaries, which are a side product of its npm release] → checked for v1.16.22 (2026-11-05). The release has `swc-linux-x64-musl`, `swc-linux-arm64-musl`, `swc-darwin-x64` and `swc-darwin-arm64` as bare executables with sha256 digests, which is what `xtask update-tools` needs. Use the static `musl` builds on Linux. `cargo xtask update-tools --check` fails if a later release drops them; then switch to esbuild (npm source in `xtask update-tools`). If it fails, switch to esbuild and teach `xtask update-tools` an npm tarball source before any other work.
- [The cfg from the build script may not reach the Bazel `rust_test` target, so CI would ignore the asset tests] → task 3.4 checks that `bazel test` runs them and doesn't list them as ignored. If it does, add `--cfg=df_web_scripts` and `--cfg=df_web_styles` to the test target's `rustc_flags`.
- [Contributors without `bazel_env` get an unstyled, script-less UI from `just serve-local`] → `serve-local` checks for `swc` and `tailwindcss` on `PATH` first and fails with the setup command. `build.rs` prints a warning.
- [Agents in sandboxes without Bazel tools can't check compiled output] → accepted. Those tests are reported as ignored and run in CI.
- [`npx` needs Node and network access on first use] → the same accepted trade-off as the OpenSpec hook.

## Migration Plan

Contributors re-run `bazel run //:bazel_env` once to get `swc` on `PATH`. There is no runtime or data migration. Rollback is a revert of the change.

## Open Questions

- The exact `typescript` version to pin: the latest release when the change is implemented.
