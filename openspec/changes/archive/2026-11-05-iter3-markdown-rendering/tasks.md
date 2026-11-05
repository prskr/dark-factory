# Tasks

## 1. Shared renderer

- [x] 1.1 Move `transcript::markdown` with its tests to `crates/df-web/src/markdown.rs` (`render`), switch the transcript to it, and add the missing tests for the spec's "Untrusted Markdown" scenarios (script, external image, `javascript:` link); verify `cargo test -p df-web markdown` and the existing transcript snapshot tests pass unchanged

## 2. Code highlighting

- [x] 2.1 Add `syntect` (latest, `default-features = false`, features `default-syntaxes`, `default-themes`, `html`, `regex-fancy`; no comrak `syntect` feature) to `[workspace.dependencies]`; ignore RUSTSEC-2025-0141 (bincode 1) in `deny.toml` with a reason; update `Cargo.lock` and the Bazel lock files (skill `rust`); verify `cargo tree -p df-web -i onig` finds nothing and `cargo xtask outdated` passes
- [x] 2.2 Write failing tests for the "Code highlighting by fence language" scenarios (`rust`, `rs`, no language, unknown language, > 64 KiB, `<script>` in an `html` fence), then implement the highlighting adapter with `hl-` classes and a shared `LazyLock` adapter (D2); verify the tests pass and the output contains no `style=` attribute
- [x] 2.3 Add a transcript test for "Highlighting only completed Markdown": a streaming agent message with a `rust` fence has no `hl-` classes, the completed one has; verify with `cargo test -p df-web transcript`

## 3. Code themes

- [x] 3.1 Write failing config tests, then add `web.codeTheme.light` / `.dark` to `WebConfig` with defaults (`InspiredGitHub`, `base16-ocean.dark`); verify `cargo test -p df-core config`
- [x] 3.2 Write failing tests, then implement `markdown::theme_css(light, dark)` (light rules, dark rules inside `prefers-color-scheme: dark`, `pre` background per theme) and theme-name validation returning an error that names the option and lists the valid themes; verify the tests pass, including an unknown name
- [x] 3.3 Validate the themes at control plane startup and pass them to the web state (`dark-factory/src/main.rs`); serve the stylesheet at `/assets/code-theme.css?v=<hash>` with immutable cache headers and link it in `layout.rs` after `app.css`; verify a router test for status, content type, cache header and the hashed link in the page
- [x] 3.4 Replace `bg-code-on-agent` for fenced blocks in `styles/input.css` (inline code keeps it); check contrast of default themes in both UI themes in the browser with `just serve-local` (state if no container socket is available)
- [x] 3.5 Document `web.codeTheme.light` / `.dark` (optional, defaults, valid names, read at startup, unknown name fails startup) in `crates/df-agent-channel/docs/configuration.md` and the renderer in `crates/df-web/README.md`

## 4. Mermaid

- [x] 4.1 Vendor the latest Mermaid 11 IIFE build as `crates/df-web/assets/vendor/mermaid.min.js`; note version and SHA-256 in `crates/df-web/README.md`; verify it is embedded (`asset_url` test) and no page references a third-party host
- [x] 4.2 Write failing tests, then render `mermaid` fences as plain escaped `<pre>` with `x-data="mermaidDiagram"` and put `data-mermaid-src` (hashed URL) on `<body>`; verify the HTML tests, including a fence whose source contains `"` and `<`
- [x] 4.3 Implement `assets/markdown.js` (`Alpine.data('mermaidDiagram')`, D5: lazy load once, `securityLevel: 'sandbox'` kept in `secure`, theme from `matchMedia`, redraw on change, error line via `textContent`, listener removed in `destroy`), add it to `COMPONENT_SCRIPTS`; verify with `script_source` tests that it sets `securityLevel: 'sandbox'` and never assigns `innerHTML` from the source
- [x] 4.4 Browser check (`just serve-local`, fake agent or seeded transcript): a flowchart renders in an iframe with a fitting height, an invalid diagram shows source and error, a `%%{init: {"securityLevel":"loose"}}%%` diagram with an `onerror` label runs no script, a page without diagrams does not request `mermaid.min.js`, switching the color scheme redraws; record results in the change's review log (state if no container socket is available)

## 5. Helm

- [x] 5.1 Write failing chart unit tests (defaults, quoted `Solarized (dark)`, `Monokai` rejected), then add `web.codeTheme.light|dark` to `values.yaml`, `values.schema.json` (enum), the configmap and the chart README; verify `helm unittest` passes
- [x] 5.2 Add a Rust test that every theme name in `values.schema.json`'s enum exists in syntect's bundled theme set; verify it passes

## 6. Integration

- [x] 6.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo deny check`, `cargo xtask check-file-length` and the Bazel build (`bazel --bazelrc=.bazelrc.agents build //crates/df-web/... //crates/dark-factory/...`); verify all pass

## Workflow follow-up

- Update ADR-0047's status once implemented and archive the change after review.
