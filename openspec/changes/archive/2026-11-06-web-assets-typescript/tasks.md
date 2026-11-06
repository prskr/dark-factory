# Tasks

Published docs: no change. Users and operators see the same assets under the same URLs (images are built by Bazel). Only contributor docs change.

## 1. Pinned swc tool

- [x] 1.1 Check that the latest swc GitHub release has standalone CLI binaries for every entry in `PLATFORMS` (`tools/xtask/src/main.rs`) and note the asset names: v1.16.22 has `swc-linux-x64-musl`, `swc-linux-arm64-musl`, `swc-darwin-x64` and `swc-darwin-arm64`, each with a sha256 digest (design, Risks).
- [x] 1.2 Add `swc` to `TOOLS` with an asset-name test like `tailwindcss_asset_names`, run `cargo xtask update-tools`, and add it to `//:bazel_env`. Verify that `bazel run //:bazel_env` puts `swc` on `PATH` and `swc --version` prints the pinned version.
- [x] 1.3 Add `swc` to the pinned-tools list in `docs/development.md`.

## 2. TypeScript sources and type checking

- [x] 2.1 Add `crates/df-web/tsconfig.json` (D2) and `scripts/types/alpine.d.ts`. Verify `npx --yes -p typescript@<version> tsc -p crates/df-web` runs on an empty `scripts/`.
- [x] 2.2 Move `tabs`, `egress-rule`, `workspace`, `egress-log` and `markdown` from `assets/*.js` to `scripts/*.ts` with explicit types. Keep their header comments and behaviour. Verify `tsc` passes with `strict`.
- [x] 2.3 Add the `web-typecheck` lefthook command and a CI step in `.github/workflows/ci.yaml`. Add a Renovate regex manager that updates both `typescript@` versions. Verify that a deliberate type error fails the hook, then revert it.

## 3. Asset build in `build.rs` and Bazel

- [x] 3.1 Add `crates/df-web/.swcrc` (strip types, ES2022, minify) and a genrule `:web_scripts` in `crates/df-web/BUILD.bazel`. Pass its outputs to the build script as `WEB_SCRIPTS` (space-separated file paths) and set `DF_WEB_REQUIRE_ASSETS=1`. Verify `bazel build //crates/df-web:web_scripts` emits one `.js` per source.
- [x] 3.2 Rework `build.rs` per D3: Bazel output, then tool from `PATH`/`SWC_BIN`/`TAILWIND_BIN`, then warn and skip. Remove the `app.css` fallback, emit `df_web_scripts` and `df_web_styles` for the kinds that compiled (D4), and delete `crates/df-web/assets/app.css`. Verify `cargo build -p df-web` with an empty `PATH` for the tools succeeds and prints both warnings.
- [x] 3.3 Switch `script_source` to a test-only embed of `scripts/` that reads the `.ts` source, and update its callers (`layout.rs`, `markdown` and request tests). Gate each test that reads compiled output on the kind it reads (D4): for example `radios_and_checkboxes_use_the_accent_text_color` on `df_web_styles`, and `embeds_stylesheet_and_vendored_scripts` on the stylesheet check only, since the vendored scripts are always embedded. Find the others by running without tools. Verify `cargo test -p df-web` without tools passes and lists exactly those tests as ignored. Then put only `swc` on `PATH`, without `tailwindcss`, and verify the script tests run while only the stylesheet tests are ignored.
- [x] 3.4 Run `bazel --bazelrc=.bazelrc.agents test //crates/df-web/...`. Verify the gated tests run and are not ignored. If they are ignored, add `--cfg=df_web_scripts` and `--cfg=df_web_styles` to the test target (design, Risks).
- [x] 3.5 Add a test that a compiled component (`tabs.js`) is embedded and contains no TypeScript-only syntax, for example `: string`, gated only by `df_web_scripts`. Verify it passes under Bazel, and under `cargo` with `swc` but without `tailwindcss` on `PATH`.

## 4. Developer workflow and rules

- [x] 4.1 Make `just serve-local` fail early with the `bazel run //:bazel_env` hint when `swc` or `tailwindcss` is missing on `PATH`. Verify by running it with a `PATH` without them.
- [x] 4.2 Update the root `AGENTS.md` section "Plain `cargo` builds" to the narrowed rule (proposal, What Changes). Update `crates/df-web/AGENTS.md` "Web UI scripts": `.ts` sources in `scripts/`, compiled and minified, `tsc` check, `script_source` reads the source. Update the `rust` skill (`.agents/skills/rust/SKILL.md`) on what plain `cargo` needs and which tests are ignored without the tools.
- [x] 4.3 Document in `docs/development.md` how web assets are built (Bazel, `PATH`, warning without tools, the `df_web_scripts` and `df_web_styles` cfgs) and how to run the type check. Verify the commands there run as written.

## 5. Integration checks

- [x] 5.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-crates` and `cargo-machete`. Verify they are clean with and without the asset tools on `PATH`.
- [x] 5.2 With a container socket, run `just serve-local` and check in a browser that tabs, egress log, workspace and Mermaid diagrams behave as before. Without a socket, say so in the report and rely on the e2e CI job.

## Workflow follow-up

- On archive, add backlog rows to `product/iterations/roadmap.md` for: chat autocomplete of file and skill names, a JS unit test runner, and the native TypeScript compiler as a pinned binary.
