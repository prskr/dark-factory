# Design

## Context

- `df-web/src/transcript/mod.rs` has `pub fn markdown(source) -> Markup`: comrak with `render.escape = true`, `render.unsafe = false`, images rewritten to links in the AST, `rel` added by a string replace. It is called for agent messages whose turn ended (`ItemKind::Agent { complete: true }`); the SSE stream morphs the message element when the turn ends.
- comrak is a workspace dependency with `default-features = false`; syntect is not in the graph. `fancy-regex` already is (transitively).
- `df-web` assets are embedded at build time (`rust-embed`, `$OUT_DIR/assets/`) and served under `/assets/{*path}` with a content hash and a one-year immutable cache (`asset_url`). Alpine components live in `assets/<name>.js` and are listed in `COMPONENT_SCRIPTS` (`layout.rs`); Alpine initialises components in nodes added later (SSE swaps) by itself.
- Web settings come from the instance config file (`df_core::config::WebConfig`, `web.showHiddenBlocks`), rendered by the Helm configmap and passed to `df-web` state in `dark-factory/src/main.rs`.
- Light/dark is pure CSS (`@media (prefers-color-scheme: dark)` redefines tokens). Agent bubbles are inverted; `.md-body pre` uses `bg-code-on-agent`.
- Requirements: `specs/markdown-rendering/spec.md`, `specs/helm-deployment/spec.md`. Decisions behind them: ADR-0047.

## Goals / Non-Goals

**Goals:**
- One renderer module that the transcript uses now and `iter3-artifacts` reuses as is.
- Theme CSS generated once at startup; HTML independent of the theme.

**Non-Goals:**
- Rendering artifacts, the image endpoint, inline images of the same task: `iter3-artifacts` (iteration 3, ADR-0048).
- Highlighting tool-call content, diff colouring: backlog (roadmap).
- Custom `.tmTheme` files, bubble-aware code themes: backlog (roadmap).
- A Content-Security-Policy for the UI: backlog (roadmap); Mermaid's sandbox does not depend on it.
- Copy buttons, language guessing: rejected.

## Decisions

### D1: Module `df_web::markdown`

`transcript::markdown` moves to `crates/df-web/src/markdown.rs` (`pub fn render(source: &str) -> Markup`), keeping the existing safety steps and their tests. The transcript calls it. Artifacts will call the same function. No trait, one renderer.

### D2: Highlighting through our own comrak `SyntaxHighlighterAdapter` on syntect

- `syntect` is declared in `[workspace.dependencies]` with `default-features = false` and the features `default-syntaxes`, `default-themes`, `html` and `regex-fancy` (pure-Rust regex, bundled syntaxes and themes, no oniguruma; `onig` is absent from the lock). Not `default-fancy`: its `yaml-load` pulls in the unmaintained `yaml-rust`, and nothing here loads `.sublime-syntax` or `.tmTheme` files. `comrak` keeps its feature set: its `syntect` feature is not used.
- `markdown.rs` implements comrak's `SyntaxHighlighterAdapter` directly on syntect (`ClassedHTMLGenerator`, class prefix `hl-`) instead of wrapping comrak's `SyntectAdapter`, which falls back to first-line detection and cannot be told not to:
  - content above 64 KiB (`const MAX_HIGHLIGHT_BYTES`), no language, a language syntect does not know, or a syntect parse error → escaped plain text;
  - otherwise → highlighted with classes.
  - every fenced block is written as `<pre class="hl-code">`, so the theme stylesheet gives plain and highlighted blocks the same background.
- Language lookup: syntect's `find_syntax_by_token`, which covers names and extensions (`rust`, `rs`). It never falls back to first-line detection.
- The `SyntaxSet` (loading takes tens of milliseconds) is built once in a `LazyLock` and shared.
- Comrak passes untrusted text through the adapter; the adapter's output must be escaped. Tests cover `<script>` inside a highlighted `html` fence.

### D3: Theme names and stylesheet

- `WebConfig` gets `code_theme: CodeThemes { light: String, dark: String }` with the defaults from the spec, `#[serde(default)]`.
- Validation at startup, in the binary before the server starts: each name must be a key of syntect's `ThemeSet::load_defaults()`. The error names the option (`web.codeTheme.dark`) and the valid names (sorted).
- `df_web::markdown::theme_css(light, dark) -> String` uses `syntect::html::css_for_theme_with_class_style` with the `hl-` prefix: the light CSS as is, the dark CSS wrapped in `@media (prefers-color-scheme: dark) { … }`. It also sets the `pre` background from each theme's `background` setting, so a block shows its theme's background inside agent bubbles too; this replaces `bg-code-on-agent` for fenced blocks (inline code keeps it).
- The CSS is generated once when the router is built and served at `/assets/code-theme.css?v=<hash of the content>` by a dedicated route with the same cache headers as embedded assets. `layout.rs` links it after `app.css`.

Alternative: CSS custom properties per highlight class in `input.css` with fixed palettes: no operator choice.

### D4: Streaming stays plain

No change: `ItemKind::Agent { complete: false }` already renders plain text. The turn-end morph swaps in the rendered HTML, which carries the highlight classes and, for diagrams, the Alpine component.

### D5: Mermaid component

- `assets/vendor/mermaid.min.js`: the IIFE build of the latest Mermaid 11 release, vendored like htmx and Alpine (version and checksum noted in `crates/df-web/README.md`).
- `assets/markdown.js` registers `Alpine.data('mermaidDiagram', …)` and is added to `COMPONENT_SCRIPTS`. On `init()` it:
  1. loads Mermaid once per page by injecting a `<script>` with the hashed URL (a module-level promise; the URL is read from a `data-mermaid-src` attribute that `layout.rs` puts on `<body>` of every page; the script itself loads only on the first diagram),
  2. calls `mermaid.initialize({ startOnLoad: false, securityLevel: 'sandbox', theme })` with `theme` from `matchMedia('(prefers-color-scheme: dark)')` (`dark` / `default`), keeping `securityLevel` in Mermaid's `secure` list so `%%{init}%%` directives cannot override it,
  3. `mermaid.render(id, source)` and puts the returned markup (a sandboxed iframe) into a sibling `<div class="diagram">` after the `<pre>`, which gets `hidden`; the component keeps the source text for a redraw. The iframe height is computed from the SVG's `viewBox` and the iframe width (Mermaid sizes it for the unscaled SVG, which leaves empty space when the SVG shrinks), and recomputed when the diagram resizes (`ResizeObserver`),
  4. on a parse error leaves the source visible and adds a one-line error `<p>` below it (`textContent`, never HTML),
  5. listens to the `matchMedia` `change` event and redraws; removes the listener in `destroy()`, which also removes the siblings,
  6. keeps the current view (diagram or error) in component state and a `MutationObserver` (the `<pre>`'s `hidden` attribute and the parent's children) puts it back when an htmx morph of the surrounding element (request cards are morphed in place on state changes) resets `hidden` and drops the siblings. The restore only creates what is missing, so a diagram is never drawn twice.
- The `<pre>` comes from comrak's `codefence_renderers` for `mermaid` (`<pre class="hl-code" x-data="mermaidDiagram"><code>…escaped…</code></pre>`), not from the highlighter; there is no `data-mermaid` marker.
- `.diagram` is as wide as the bubble allows (`w-screen max-w-full`) and has the `panel` color: the diagram theme follows `prefers-color-scheme`, while agent bubbles are inverted, so the diagram needs its own background. The iframe is `color-scheme: light` so it stays transparent. Request cards render their Markdown in `.md-body` as well.
- Without JavaScript the `<pre>` stays a plain code block (spec "Diagrams without JavaScript").

Alternative: `securityLevel: 'strict'` (inline SVG, DOMPurify). Rejected in ADR-0047: isolation would depend on the sanitizer.

### D6: Helm

`values.yaml` `web.codeTheme.light|dark`, `values.schema.json` enum, configmap renders both quoted (names contain spaces and parentheses), README table rows, unit tests for defaults, quoting and schema rejection.

## Risks / Trade-offs

- [Mermaid's `sandbox` level is marked beta; iframe height may not fit the diagram] → After rendering, set the iframe height from the SVG's `viewBox` if Mermaid leaves it fixed; covered by a browser check, not a unit test.
- [Binary size grows by syntect's bundled syntaxes and themes (a few MiB)] → Accepted (ADR-0047).
- [fancy-regex highlighting of large or pathological input costs CPU per page view (no cache)] → 64 KiB cap per block; profile before adding a cache.
- [`syntect` loads its bundled syntax and theme dumps with `bincode` 1, which `cargo deny` reports as unmaintained (RUSTSEC-2025-0141)] → Ignored in `deny.toml` with a reason: bincode 1.3.3 is declared complete by its authors, only bundled data is decoded. Revisit when syntect moves on.
- [~3 MB Mermaid script] → Loaded only on pages with a diagram, cached immutably by hash.
- [syntect theme names change between releases] → Names are validated at startup and in the Helm schema; a test asserts every schema enum value exists in `ThemeSet::load_defaults()`.

## Migration Plan

Additive. Existing instance configs without `web.codeTheme` get the defaults. Rollback: previous image; the extra config keys are rejected by the old binary (`deny_unknown_fields`), so roll back the chart together with the image.
