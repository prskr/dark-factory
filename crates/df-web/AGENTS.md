# df-web

Rules for the web UI crate, on top of the root `AGENTS.md`.

## Web UI scripts

JavaScript that is code lives in TypeScript files, not in Rust strings.

- Inline `x-data="{ … }"` holds plain data only: flags, strings, numbers, `null`, including values the server fills in. Short expressions in `x-on:*`, `:class` and similar attributes stay inline.
- State with a method, getter or `init()` is an Alpine component in `crates/df-web/scripts/<name>.ts` that calls `Alpine.data('<name>', …)` on `alpine:init`. Add `<name>.js` to `COMPONENT_SCRIPTS` in `crates/df-web/src/layout.rs`, which loads it before Alpine. Server values go in as a JSON argument (`x-data="egressLog({…})"`, escaped by maud).
- Files are classic scripts: no `import`/`export`, no bundler. `swc` (pinned, `.swcrc`) compiles each `.ts` to one minified `<name>.js` served under `/assets/`. Responses are compressed and cached by content hash. Bazel builds them in the genrule `:web_scripts`, `build.rs` runs `swc` from `PATH` (or `SWC_BIN`) under plain `cargo`.
- Types: `tsconfig.json` is `strict`. The Alpine API and other globals the components use are declared in `scripts/types/*.d.ts`; extend them when a component needs more. Check with `npx --yes -p typescript@<version> tsc -p crates/df-web` (version as in `lefthook.yml`; the `web-typecheck` hook and CI run it).
- Tests read a component's TypeScript source with `crate::assets::script_source("<name>.ts")`; they run in every build. A test that reads compiled output gets `#[cfg_attr(not(df_web_scripts), ignore = "…")]` (compiled scripts), `not(df_web_styles)` (`app_css()`) or `not(all(df_web_scripts, df_web_styles))`, whichever kinds it reads, so that a missing tool never lets it pass silently.
- Stylesheet: `styles/input.css` is compiled by `tailwindcss` (Bazel genrule or `build.rs`). `assets/app.css` is not checked in.
- Third-party scripts: Mermaid, htmx with its SSE extension and Alpine are not checked in. `package.json` pins them, `pnpm-lock.yaml` locks them, and `build.rs` embeds their published dist files under `/assets/vendor/` (table `VENDOR` in `build_assets.rs`). Under plain `cargo`, run `pnpm install --frozen-lockfile` in `crates/df-web` (pnpm as in `packageManager`); without it the build warns and the UI does not work. Bazel (`:vendor_scripts`, `VENDOR_SCRIPTS`) fails without them. A test that reads them gets `#[cfg_attr(not(df_web_vendor), ignore = "…")]`, or `not(all(…))` together with the other kinds it reads. A bump changes only `package.json` and `pnpm-lock.yaml` (Renovate).

## Checking the UI in a browser

With a container socket, `just serve-local` runs the control plane on `http://127.0.0.1:8080` with PostgreSQL and Dex from `compose.yml` (login `admin@example.com` / `password`, `docs/development.md`, section 6). Compare against the PenPot boards as described in `docs/design-workflow.md`. The `obscura` browser has a fixed 1280 px viewport; narrower or wider widths need a browser with viewport control (Playwright MCP).
