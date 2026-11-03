# df-web

Rules for the web UI crate, on top of the root `AGENTS.md`.

## Web UI scripts

JavaScript that is code lives in JS files, not in Rust strings.

- Inline `x-data="{ … }"` holds plain data only: flags, strings, numbers, `null`, including values the server fills in. Short expressions in `x-on:*`, `:class` and similar attributes stay inline.
- State with a method, getter or `init()` is an Alpine component in `crates/df-web/assets/<name>.js` that calls `Alpine.data('<name>', …)` on `alpine:init`. Add the file to `COMPONENT_SCRIPTS` in `crates/df-web/src/layout.rs`, which loads it before Alpine. Server values go in as a JSON argument (`x-data="egressLog({…})"`, escaped by maud).
- Tests read a component's source with `crate::assets::script_source`.
- Scripts are served as written: no bundler or minifier. Responses are compressed and cached by content hash.

## Checking the UI in a browser

With a container socket, `just serve-local` runs the control plane on `http://127.0.0.1:8080` with PostgreSQL and Dex from `compose.yml` (login `admin@example.com` / `password`, `docs/development.md`, section 6). Compare against the PenPot boards as described in `docs/design-workflow.md`. The `obscura` browser has a fixed 1280 px viewport; narrower or wider widths need a browser with viewport control (Playwright MCP).
