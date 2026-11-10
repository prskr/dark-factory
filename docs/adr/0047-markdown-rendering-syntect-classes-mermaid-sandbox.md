# ADR-0047: Markdown rendering with class-based syntect themes and sandboxed Mermaid

- **Status:** Accepted (implemented with `iter3-markdown-rendering`; amended on 2026-11-05: own syntect adapter instead of comrak's, syntect features without `yaml-load`)
- **Date:** 2026-11-02
- **Related:** [ADR-0004](0004-server-rendered-ui-maud-htmx-alpine.md), [ADR-0020](0020-configuration-in-database.md), [ADR-0044](0044-task-artifacts-and-cross-task-reading.md), [ADR-0048](0048-image-artifacts.md)

## Context

ADR-0004 renders Markdown on the server with `comrak` + `syntect`; only comrak was built, so code blocks in agent messages are unhighlighted. Artifacts (ADR-0044) make Markdown the main way results travel, and agents describe architecture and flows in Mermaid. All rendered text is agent-written and untrusted. The UI has a light and a dark theme, switched by `prefers-color-scheme` without script.

## Decision

- **Code blocks:** highlighted on the server by a small comrak highlighter adapter over syntect in CSS-class mode (`hl-` prefix; comrak's own syntect adapter falls back to first-line detection), syntect with the pure-Rust regex engine (`regex-fancy` with bundled syntaxes and themes, no oniguruma and no `yaml-load`, builds as static musl). The HTML carries classes only, no colours.
- **Language:** taken from the fence's info string only (syntect name or extension). No language, an unknown one or a block above 64 KiB renders plain. No content-based guessing.
- **When:** only completed Markdown is highlighted (agent messages after their turn, artifacts). Streaming messages stay plain text.
- **Themes:** the instance config options `web.codeTheme.light` and `web.codeTheme.dark` (Helm values of the same name; defaults `InspiredGitHub` and `base16-ocean.dark`) name bundled syntect themes; an unknown name fails startup. The control plane generates one stylesheet at startup (light as default, dark under `prefers-color-scheme: dark`), served by content hash. Code blocks carry their theme's background, inside agent bubbles too.
- **Mermaid:** `mermaid` fences are rendered in the browser by `mermaid.js` (vendored: pinned in the npm lockfile of `crates/df-web`, no longer checked in), loaded only on pages that contain one, with `securityLevel: 'sandbox'` (each diagram in a sandboxed iframe). Theme `default` or `dark` from `prefers-color-scheme`. Invalid diagrams show their source as a plain code block with a one-line error; so does a build without Mermaid ("Diagram renderer not available"). A change of `prefers-color-scheme` re-renders the diagrams, as the UI switches themes without a reload.
- **No cache:** Markdown is rendered per request and per SSE replay.

## Consequences

- No colour in the server's HTML; a theme change is a restart, not a re-render of stored content.
- The binary grows by syntect's bundled syntaxes and themes.
- The page needs JavaScript for diagrams only; code highlighting works without it.
- Mermaid adds a large vendored script that pages without diagrams never load.
- Bubble-aware code themes and custom `.tmTheme` files can be added later without changing the HTML.

## Alternatives considered

- **Client-side highlighting (highlight.js) next to Mermaid:** one mechanism for both, but contradicts the server-rendered approach of ADR-0004 and moves themes into JavaScript.
- **syntect with inline styles:** one fixed theme, breaks either light/dark or the inverted agent bubble.
- **tree-sitter:** better highlighting, one C grammar dependency per language.
- **Mermaid with `securityLevel: 'strict'`:** better sizing and text selection, but isolation of untrusted diagrams then depends on its sanitizer; the UI has no CSP.
- **Theme names as an instance setting in the database (ADR-0020):** needs a form and runtime CSS regeneration for deployment presentation, which is not domain configuration.
