# Proposal

## Why

Agent messages render as Markdown, but code blocks are unhighlighted: ADR-0004 named `syntect`, only `comrak` was built. With artifacts (`iter3-artifacts`, ADR-0044) Markdown becomes the main way results travel between tasks and to humans, and agents describe flows and architecture in Mermaid. This change delivers the shared renderer that artifacts build on (ADR-0047).

## What Changes

- The Markdown renderer moves from `df-web`'s transcript module into a shared module used by the transcript now and by artifacts later. Its safety rules stay: no raw HTML, images become links, links get `rel="noopener noreferrer nofollow"`.
- Fenced code blocks with a known language in the info string are highlighted on the server by syntect in CSS-class mode. Blocks without a language, with an unknown one or above 64 KiB stay plain. Streaming agent messages stay plain text, as today.
- Two new instance config options, `web.codeTheme.light` and `web.codeTheme.dark`, name bundled syntect themes (defaults `InspiredGitHub`, `base16-ocean.dark`). An unknown name fails startup. The control plane generates a theme stylesheet once at startup and serves it with a content hash.
- `mermaid` fences render as diagrams in the browser: vendored `mermaid.js`, loaded only on pages with a diagram, `securityLevel: 'sandbox'`, theme from `prefers-color-scheme` (re-rendered when it changes). Invalid diagrams show their source and a one-line error.
- Helm values `web.codeTheme.light` / `web.codeTheme.dark` with a schema enum of the bundled theme names.

Out of scope:

- Highlighting or diff colouring of tool-call content (input, output, diffs): backlog in `product/iterations/roadmap.md`.
- Operator-supplied `.tmTheme` files: backlog.
- Code themes that follow the inverted agent bubble: backlog.
- Copy button on code blocks: rejected (code lives in the repository).
- Content-based language detection: rejected.
- Rendering artifacts: `iter3-artifacts` (iteration 3).

## Capabilities

### New Capabilities

- `markdown-rendering`: how agent-written Markdown becomes HTML in the UI: safety rules, code highlighting, code themes, Mermaid diagrams.

### Modified Capabilities

- `helm-deployment`: new values `web.codeTheme.light` and `web.codeTheme.dark` rendered into the instance config.

## Impact

- `crates/df-web`: new `markdown` module (renderer, theme stylesheet route), `transcript` uses it, new `assets/markdown.js` Alpine component (Mermaid loader), vendored `assets/vendor/mermaid.min.js`, CSS for code blocks and diagrams in `styles/input.css`.
- `crates/df-core`: `WebConfig` gets `code_theme` (`light`, `dark`), validated at startup.
- `crates/dark-factory`: passes the themes into the web state.
- Dependencies: `syntect` (features `default-syntaxes`, `default-themes`, `html`, `regex-fancy`: pure-Rust regex, no oniguruma) enters `[workspace.dependencies]`; `comrak` keeps its features. `deny.toml` ignores RUSTSEC-2025-0141 (bincode 1, used by syntect for its bundled data). `Cargo.lock` and Bazel lock files change. The binary grows by syntect's bundled syntaxes and themes.
- `deploy/helm/dark-factory`: values, schema, configmap, README, unit tests.
- Docs: instance config reference (`crates/df-agent-channel/docs/configuration.md`), `crates/df-web/README.md`.
