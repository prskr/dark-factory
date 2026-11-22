# Proposal

## Why

Board members write long Markdown in the web UI (persona system prompts now, skill bodies with iteration 4) in a plain 8-row textarea inside a modal. They cannot see how the text renders, the modal leaves no room for a long text, and long texts with code fences are hard to read without highlighting. One shared editor component fixes this for every Markdown field. The persona gets a settings page like projects and model providers, with the system prompt in its own tab where the editor and the preview fit side by side.

## What Changes

The change starts with a PenPot design sprint (tasks group 1) and then has two implementation steps on one component (design D1), delivered together in one pull request.

**Step 1: editor with preview and the persona settings page**

- **New component `markdown_field`** in `components/forms.rs`, next to `textarea_field`: a monospace textarea that grows with its content. Two layouts in this change (a third, source highlighting without preview for memories, comes with `iter6-agent-memory`):
  - **Tabs** ("Write" and "Preview"): for narrow places (modals, phones, screens below 1280 px). The preview renders when "Preview" opens.
  - **Side by side** (editor left, preview right): from 1280 px where the field has a page of its own (the System prompt tab). The preview follows the text while typing, 500 ms after the last key.
  - Without JavaScript it is a plain textarea.
- **Server-rendered preview:** a new route `POST /markdown/preview` renders the text with the existing `markdown::render` (comrak, syntect, Mermaid in its sandbox, same escaping). The preview looks exactly like the Markdown that the UI shows elsewhere. The route needs a login and the htmx header (CSRF guard), accepts a text of at most 256 KiB and returns only the rendered, escaped HTML.
- **Persona settings page** `/personas/{id}`, built like the project and model provider settings pages: settings area, back link, heading, tabs **General** (name, description, harness, model provider, model) and **System prompt** (the editor, full width), the open tab in the URL (`?tab=system-prompt`), one "Save persona" button, "Persona saved" status.
- **Creating a persona** stays a modal with the General fields; saving it opens the new persona's settings page on the System prompt tab, like a new project opens its settings page. The system prompt becomes optional (today it is required): an empty prompt is valid, and the persona card shows "No system prompt yet".
- **BREAKING (UI):** the persona edit modal and `/personas/{id}/edit` are removed; the persona card's edit action links to the settings page. Pre-MVP, no compatibility needed.

**Step 2: syntax highlighting while typing (CodeMirror 6)**

- **CodeMirror 6 as a progressive layer** over the same textarea: Markdown highlighting with nested highlighting of fenced code (a fixed set of languages). The textarea stays the form field and the fallback.
- **New build step:** a pinned `esbuild` bundles one entry file with the CodeMirror packages (pinned in `package.json`, locked in `pnpm-lock.yaml`) into one classic script `/assets/editor.js`. Bazel always builds it. A plain `cargo` build without `esbuild` or without the npm packages warns and embeds no editor bundle; the field stays a textarea with preview.
- The bundle (measured: about 510 KB minified, 175 KB gzip) loads only when a page shows an editor, like Mermaid.
- Theme from the design tokens and the configured code themes, in light and dark. Tab moves focus out of the editor. A hint below the editor names the indent keys. Phones keep the plain textarea with preview.
- **BREAKING (build, contributors only):** the `web-asset-build` rule "sources SHALL NOT be bundled" gets one exception, the editor entry. Contributors on the plain `cargo` path need `esbuild` on `PATH` (from `bazel run //:bazel_env`) to get highlighting.

## Capabilities

### New Capabilities

- `markdown-editor`: the Markdown field of the web UI: tabs and side-by-side layouts, server-rendered preview, live preview with debounce, the preview route and its limits, behavior without JavaScript, highlighting while typing, keyboard, screen reader and phone behavior.

### Modified Capabilities

- `configuration-management`: persona settings page with General and System prompt tabs, tab in the URL, saving on the page; persona editing leaves the modal ("Configuration forms as modals"); the system prompt is edited in the Markdown editor.
- `web-asset-build`: one bundled script (the editor) built by a pinned `esbuild`, its plain `cargo` fallback, and the CodeMirror packages in the npm lockfile.

## Impact

- `df-web`: `components/forms.rs` (`markdown_field`), new module `markdown_preview.rs` (route), `config_pages/personas.rs` (system prompt no longer required) and a new `config_pages/persona_page.rs` (settings page, reusing `settings_page_frame`, `tabbed_form(TabsStyle::Page)`, `saved_response`), new `scripts/markdownField.ts`, `scripts/tabs.ts` (queries only its own tab list), `scripts/markdown.ts` (diagrams kept across a morph), `layout.rs` (`COMPONENT_SCRIPTS`, `data-editor-src`), `styles/input.css`; step 2: `editor/` (ES module entry, own `tsconfig.json`), `build_assets.rs` (editor bundle, cfg `df_web_editor`), `BUILD.bazel` (genrule `:editor_bundle`), `package.json`, `pnpm-lock.yaml`.
- `df-e2e`: persona tests move from the edit modal to the settings page, and `a_persona_is_created_with_a_model_of_its_provider` (`tests/demo.rs`) no longer fills the system prompt in the create modal.
- Tooling (step 2): `multitool.lock.json` (`esbuild`), `tools/xtask` (npm tarball source for `update-tools`), `BUILD.bazel` (`bazel_env`), `lefthook.yml` and CI (type check of the entry, with `pnpm install` in CI).
- PenPot file "App Design", page "Config - Personas": new boards for the persona settings page (design sprint, tasks group 1).
- No migration and no schema change (the `system_prompt` column stays `NOT NULL`; an empty prompt is an empty string), no new Rust dependency.
- Docs: `docs/content/using/setup.md` (personas), `docs/development.md` (asset tools), `docs/design-system.md` (Forms, Configuration views), `docs/design-workflow.md` (board mapping), `crates/df-web/AGENTS.md`, root `AGENTS.md` ("Plain cargo builds": `esbuild`, `ESBUILD_BIN`).

## Integration points (no change to those proposals here)

- **Persona identity and commit signing (`persona-identity`, `persona-commit-signing`):** both add persona fields or a "persona page" section. With this change they land on the persona settings page: identity fields on General, the signing key as its own tab or a General group. Whichever change lands second adapts its persona UI tasks.
- **Skills (`iter4-skills`, DF-7):** the skill body uses `markdown_field`; decided (design Q10): skills get a settings page with a Body tab in the same shape; `iter4-skills` takes this over. Skill assignment per persona fits a later Skills tab on the persona page. Supporting files are not Markdown and stay textareas (see Out of Scope).
- **Memories (`iter6-agent-memory`, DF-10, PR #302):** that proposal says memory content is plain text and "SHALL never be rendered as Markdown". Decided (design Q6, D7a): the Memory tab gets source highlighting without preview and with a strict theme. `iter6-agent-memory` adds that variant (`Preview::SourceOnly`), because the Memory tab is its only caller, and rewords its rule to allow highlighting.

## Out of Scope

- Highlighting for non-Markdown text (skill supporting files, scripts): backlog row "Follow-ups of `markdown-editor`" in `product/iterations/roadmap.md`.
- A warning before leaving a page or closing a dialog with unsaved changes: same backlog row.
- Replacing `swc` with `esbuild` for the component scripts: same backlog row (design D5a).
- Scroll sync between editor and preview in the side-by-side layout: rejected (decided by Peter, PR #303).
- Live preview on every key without a pause: rejected (server load, flicker; 500 ms after the last key is enough).
- A Markdown toolbar (bold, link buttons) and WYSIWYG editing: rejected (the users write Markdown for agents; source is the truth).
- Client-side Markdown rendering: rejected (a second renderer would drift from `markdown::render` and its sanitizing).
