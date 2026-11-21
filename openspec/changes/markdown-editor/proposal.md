# Proposal

## Why

Board members write long Markdown in the web UI (persona system prompts now, skill bodies with iteration 4) in a plain 8-row textarea. They cannot see how the text renders, and long texts with code fences are hard to read without highlighting. One shared editor component fixes this for every Markdown field.

## What Changes

The change has two steps on one component (design D1). Step 2 can be split into its own change if the reviewers decide so (design, open question Q1).

**Step 1: editor with preview**

- **New component `markdown_field`** in `components/forms.rs`, next to `textarea_field`: a monospace textarea that grows with its content, with "Write" and "Preview" tabs. Without JavaScript it is a plain textarea.
- **Server-rendered preview:** a new route `POST /markdown/preview` renders the text with the existing `markdown::render` (comrak, syntect, Mermaid in its sandbox, same escaping). The preview looks exactly like the Markdown that the UI shows elsewhere. The route needs a login and the htmx header (CSRF guard), accepts at most 256 KiB and returns only the rendered, escaped HTML.
- The preview renders when the user opens the Preview tab, not on every key.
- **First user:** the persona form's "System prompt" field.

**Step 2: syntax highlighting while typing (CodeMirror 6)**

- **CodeMirror 6 as a progressive layer** over the same textarea: Markdown highlighting with nested highlighting of fenced code (a fixed set of languages). The textarea stays the form field and the fallback.
- **New build step:** a pinned `esbuild` bundles one entry file with the CodeMirror packages (pinned in `package.json`, locked in `pnpm-lock.yaml`) into one classic script `/assets/editor.js`. Bazel always builds it. A plain `cargo` build without `esbuild` or without the npm packages warns and embeds no editor bundle; the field stays a textarea with preview.
- The bundle (measured: about 510 KB minified, 175 KB gzip) loads only when a page shows an editor, like Mermaid.
- Theme from the design tokens and the configured code themes, in light and dark. Tab moves focus out of the editor. Phones keep the plain textarea (recommended, open question Q4).
- **BREAKING (build, contributors only):** the `web-asset-build` rule "sources SHALL NOT be bundled" gets one exception, the editor entry. Contributors on the plain `cargo` path need `esbuild` on `PATH` (from `bazel run //:bazel_env`) to get highlighting.

## Capabilities

### New Capabilities

- `markdown-editor`: the Markdown field of the web UI: Write and Preview tabs, server-rendered preview route and its limits, behavior without JavaScript, highlighting while typing, keyboard, screen reader and phone behavior, and the fields that use it.

### Modified Capabilities

- `web-asset-build`: one bundled script (the editor) built by a pinned `esbuild`, its plain `cargo` fallback, and the CodeMirror packages in the npm lockfile.
- `configuration-management`: the persona form edits the system prompt in the Markdown editor.

## Impact

- `df-web`: `components/forms.rs` (`markdown_field`), new module `markdown_preview.rs` (route), `config_pages/personas.rs`, new `scripts/markdownField.ts`, `layout.rs` (`COMPONENT_SCRIPTS`, `data-editor-src`), `styles/input.css`; step 2: `editor/` (ES module entry, own `tsconfig.json`), `build_assets.rs` (editor bundle, cfg `df_web_editor`), `BUILD.bazel` (genrule `:editor_bundle`), `package.json`, `pnpm-lock.yaml`.
- Tooling (step 2): `multitool.lock.json` (`esbuild`), `BUILD.bazel` (`bazel_env`), `lefthook.yml` and CI type check of the entry.
- No database change, no migration, no new Rust dependency.
- Docs: `docs/content/using/setup.md` (persona form), `docs/development.md` (asset tools), `docs/design-system.md` (Forms), `crates/df-web/AGENTS.md`.

## Integration points (no change to those proposals here)

- **Skills (`iter4-skills`, DF-7):** the skill body in the skill form modal uses `markdown_field`. Supporting files are not Markdown and stay textareas (see Out of Scope).
- **Memories (`iter6-agent-memory`, DF-10, PR #302):** that proposal says memory content is plain text and "SHALL never be rendered as Markdown". A preview tab would contradict it. The component therefore has a mode without preview (highlighting only, or a plain textarea). Which one the Memory tab uses is open question Q6.

## Out of Scope

- Side-by-side Write and Preview on wide screens: backlog row "Markdown editor follow-ups" in `product/iterations/roadmap.md` (open question Q3).
- Highlighting for non-Markdown text (skill supporting files, scripts): same backlog row.
- A warning before a dialog with unsaved changes closes (Escape, backdrop): same backlog row.
- Live preview on every key: rejected (server load, flicker; tab switch is enough).
- A Markdown toolbar (bold, link buttons) and WYSIWYG editing: rejected (the users write Markdown for agents; source is the truth).
- Client-side Markdown rendering: rejected (a second renderer would drift from `markdown::render` and its sanitizing).
