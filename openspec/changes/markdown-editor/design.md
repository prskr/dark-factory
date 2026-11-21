# Design

## Context

Motivation and scope: see `proposal.md`. Current state that shapes the design:

- **Fields.** `textarea_field` (`crates/df-web/src/components/forms.rs`) renders a labelled `<textarea>` with `INPUT_CLASS` (16 px sans text, no auto-grow). The persona form calls it with 8 rows for `system_prompt` (`config_pages/personas.rs`, `persona_form`). The form lives in a `dialog_with(…, DialogStyle::Form, …)` modal: a centered card from 768 px, a full-height sheet below. It posts with htmx (`hx-post`, `#form`, 422 re-renders the form with values).
- **Renderer.** `markdown::render` (`src/markdown.rs`) is the single Markdown renderer: comrak with raw HTML escaped, unsafe links dropped, images as links, `rel="noopener noreferrer nofollow"`, syntect highlighting with `hl-` classes (64 KiB per fence), Mermaid fences as `<pre x-data="mermaidDiagram">` that `scripts/markdown.ts` draws in Mermaid's sandbox iframe. The code theme stylesheet `/assets/code-theme.css` switches with `prefers-color-scheme`.
- **Scripts.** `scripts/*.ts` are classic scripts compiled one by one by `swc`; no `import`/`export`, no bundler (`crates/df-web/AGENTS.md`). Third-party scripts are dist files of npm packages, pinned in `package.json`, locked in `pnpm-lock.yaml`, staged by `build_assets.rs` (`VENDOR`) and by Bazel (`:vendor_scripts` from `npm_link_all_packages`). Mermaid (5.5 MB minified) is loaded only when a page has a diagram: `<body data-mermaid-src>` names its URL, the component injects the `<script>` on first use.
- **Tools.** `swc` and `tailwindcss` are pinned in `multitool.lock.json`, used by Bazel genrules and put on `PATH` by `bazel run //:bazel_env`. Plain `cargo` runs them from `PATH`; a missing tool warns and sets no cfg (`df_web_scripts`, `df_web_styles`, `df_web_vendor`).
- **CSRF.** `auth::csrf_guard` rejects a non-GET request without the htmx header; with `SameSite=Lax` this is the CSRF protection of every form.
- **Tabs.** The Alpine component `tabs` (`scripts/tabs.ts`) implements the WAI-ARIA tab pattern (arrow keys, Home and End, `role=tabpanel`).
- **No JavaScript.** The persona form is a modal that htmx loads, so the form itself needs JavaScript. "Without JavaScript" in this design means: the editor script did not load or failed. The field then works as a plain textarea. A page that renders the field without a modal (later users) works without any script.

## Goals / Non-Goals

**Goals:**

- One component for every Markdown field, so skills and later fields get the same behavior by one call.
- A preview that is byte-equal to what the UI shows elsewhere, with the same sanitizing.
- Highlighting while typing that degrades to a working textarea in every failure case: no tool at build time, script load error, phone, screen reader preference.
- No new trust in third-party build artifacts.

**Non-Goals:**

- Side-by-side layout, highlighting of non-Markdown files, an unsaved-changes guard: backlog row "Markdown editor follow-ups" in `product/iterations/roadmap.md`.
- Collaborative editing, autosave, drafts: rejected (forms save on submit).
- Spell checking inside CodeMirror beyond what the browser offers: rejected.
- Markdown linting (heading levels, line length): rejected.

## Decisions

### D1. Two steps on one component; both in this change

Step 1 (component, preview route, persona form) needs no build change. Step 2 (CodeMirror) changes the asset build. Both steps share the markup contract: the `<textarea name=…>` is always the form field, and the component enhances it. Step 2 only adds a layer, no change to the server side or to callers.

Both steps are in this change so that the design is reviewed once, and implemented as two pull requests (task groups 1–3 and 4–6). Step 1 is complete and shippable on its own.

- *Alternative: CodeMirror only in a later change.* Lower risk now, but the editor that skills get in iteration 4 would be without highlighting, and Peter named highlighting in long skill texts as the main benefit. Open question Q1.
- *Alternative: CodeMirror first, preview later.* Rejected: the build pipeline is the risky part; the preview is cheap and useful alone.

### D2. Component API and markup

```rust
pub struct MarkdownField<'a> {
    pub label: &'a str,
    pub name: &'a str,
    pub value: &'a str,
    pub hint: Option<&'a str>,
    pub rows: u8,              // minimum height
    pub preview: bool,         // false: no tabs (memory content, Q6)
}
pub fn markdown_field(field: &MarkdownField<'_>, errors: &FormErrors) -> Markup
```

Markup (step 1), simplified:

```html
<div class="space-y-1" x-data='markdownField({"preview": true})'>
  <div class="flex items-end justify-between">
    <label for="system_prompt">System prompt</label>
    <div role="tablist" x-cloak x-show="enhanced" @keydown="move($event)">
      <button role="tab" id="system_prompt-tab-write" aria-controls="system_prompt-write">Write</button>
      <button role="tab" id="system_prompt-tab-preview" aria-controls="system_prompt-preview">Preview</button>
    </div>
  </div>
  <div id="system_prompt-write">
    <textarea id="system_prompt" name="system_prompt" rows="8" class="… font-mono text-sm"
              aria-describedby="…">…</textarea>
  </div>
  <div id="system_prompt-preview" hidden class="markdown …" aria-live="polite"></div>
  <p id="system_prompt-hint">…</p> <p id="system_prompt-error" role="alert">…</p>
</div>
```

- The tab row sits right of the label (GitHub-like, compact) and is hidden until the component runs (`x-cloak`), so the no-JS field is a plain labelled textarea.
- `markdownField` is a new Alpine component (`scripts/markdownField.ts`). It reuses the keyboard logic of `tabs` by calling the same functions; to share code between classic scripts, the tab keyboard logic moves to a small global helper declared in `scripts/types/` (or `markdownField` nests an `x-data="tabs(…)"`; the implementer picks the smaller diff). Tab state is local; it is not written to the URL.
- The textarea uses `font-mono text-sm` (13–14 px, like code) from 1024 px and keeps 16 px below, so phones do not zoom on focus.
- **Auto-grow:** CSS `field-sizing: content` with `min-height` from `rows` and `max-height: 70vh` (then it scrolls). Browsers without `field-sizing` (Firefox at the time of writing) keep the fixed `rows` height and a resize handle; no script fallback. *Alternative:* a script that sets `height = scrollHeight` on input: works everywhere but adds layout work per key; rejected for step 1, can be added if Firefox users complain. With CodeMirror (step 2) the editor grows by itself up to the same maximum.

### D3. Preview route

`POST /markdown/preview`, inside the board-member router (login) and behind `csrf_guard` (htmx header), form body `source=<text>`.

- **Rendering:** `markdown::render(source)`, the variant for agent text and artifacts. Single newlines are soft breaks, as an agent and the Markdown artifact view see them. *Alternative:* `render_with_hard_breaks` (user messages): rejected, skills and prompts are documents, not chat messages.
- **Response:** `200` with the rendered fragment wrapped in `<div class="markdown">`; empty source gives "Nothing to preview" in `faint`. Mermaid fences come back as the usual `<pre x-data="mermaidDiagram">`; Alpine initializes them when htmx swaps the fragment in, so diagrams draw in their sandbox like everywhere else.
- **Limit:** `DefaultBodyLimit::max(256 KiB + form overhead)` on this route (256 KiB is the skill size limit of `iter4-skills`). A larger body gets `413` and the preview shows "Text too long for a preview (max 256 KiB)". The form itself still saves; the limit is only for preview. Syntect's 64 KiB per fence cap applies as usual.
- **No storage, no audit:** the route is a pure function of its input; it writes nothing and logs no content.
- **Request from the client:** the component posts the textarea value with `htmx.ajax('POST', '/markdown/preview', { values: { source }, target })`. htmx adds `HX-Request`, so the CSRF guard passes. The request runs when the Preview tab opens; if the text did not change since the last preview, the old preview is shown without a request. A failed request shows "Preview not available" with a retry link; the Write tab is unaffected.
- *Alternative: render in the browser* (markdown-it or similar). Rejected: a second renderer with its own sanitizing, a second set of GFM rules and a second highlighter. The server already has the exact renderer.
- *Alternative: a `GET` with the text in the query.* Rejected: URL length, texts in access logs.

### D4. Highlighting while typing: options compared

| | 1. Plain textarea | 2. CodeMirror 6 | 3. CodeMirror 5 | 4. Overlay (`<pre>` under a transparent textarea) |
|---|---|---|---|---|
| Highlighting | none | Markdown + nested fences (Lezer parsers, incremental) | Markdown mode + `gfm`, fences via modes | own tokenizer |
| Build | none | bundler needed (ES modules only) | prebuilt `lib/codemirror.js` + mode files, fits `VENDOR` | none (own code) |
| Size (min / gzip) | 0 | ~510 KB / ~175 KB (measured, below) | ~170 KB / ~60 KB (core + markdown, gfm, a few modes; estimate) | small, but grows with each language |
| Maintenance | none | active, frequent releases (Renovate) | maintenance mode since 2022, security fixes only | all ours: tokenizer, scroll sync, caret, IME, wrapping, fonts |
| Accessibility | native | `contenteditable` with `role=textbox`, good keyboard support, screen readers usable but less native than a textarea | `textarea`-backed input, older a11y model | native textarea (the text is real) |
| Phones | native | works, known IME/autocorrect quirks on Android | weak | native, but sync breaks with zoom and keyboards |
| Fallback | – | textarea stays the field | textarea stays the field | textarea is the field |

Notes:

- **CodeMirror 5** is the only option that fits the current "dist files from npm" rule without a bundler. But it is legacy: no new features, its Markdown highlighting is regex based, and an editor we add now would have to be migrated later. Rejected.
- **Overlay** looks cheap but is not: a tokenizer for Markdown plus fenced languages, pixel-exact sync of scroll, wrapping, tab width and fonts, and selection colors. Every browser quirk becomes ours. Rejected.
- **CodeMirror 6** is the only option with real Markdown parsing, nested fences and an active upstream. Its cost is the build (D5) and the bundle size (D6).

**Recommendation:** CodeMirror 6 as step 2 (D1). If the build step is not accepted, keep option 1 (plain textarea with preview); not option 3 or 4.

### D5. Bundling CodeMirror 6

CodeMirror 6 ships only ES modules (`@codemirror/view`, `state`, `commands`, `language`, `lang-markdown`, `@lezer/*`); there is no official single-file build.

Options:

1. **Pinned `esbuild` for one entry file (recommended).** `esbuild` is one static binary, like `swc` and `tailwindcss`. Pin it in `multitool.lock.json` (the npm tarballs `@esbuild/linux-x64`, `linux-arm64`, `darwin-arm64` contain the binary; multitool supports archives with sha256), expose it through `bazel_env`.
   - Entry: `crates/df-web/editor/markdown-editor.ts`, an ES module that imports the CodeMirror packages and exposes one global, `window.DfMarkdownEditor = { attach(textarea, options) }`. It lives outside `scripts/`, so the classic-script rule for `scripts/*.ts` stays unchanged.
   - Output: `esbuild --bundle --minify --format=iife --target=es2022` to one classic script, served as `/assets/editor.js` (hashed URL like all assets).
   - Packages: added to `crates/df-web/package.json` (`@codemirror/*`, `@lezer/highlight`), locked in `pnpm-lock.yaml`, bumped by Renovate. Bazel gets them from `npm_link_all_packages` (already used for `VENDOR`); plain `cargo` from `crates/df-web/node_modules` (already needed for the vendor scripts).
   - Bazel: genrule `:editor_bundle` (esbuild + the linked packages), passed to the build script as `EDITOR_BUNDLE`; with `DF_WEB_REQUIRE_ASSETS=1` a missing bundle fails the build.
   - Plain `cargo`: `build_assets.rs` runs `esbuild` from `PATH` (or `ESBUILD_BIN`) when the CodeMirror packages are installed. Missing tool or packages → `cargo:warning` naming them, no bundle, cfg `df_web_editor` not set. The page then has no `data-editor-src` and the field stays a textarea with preview.
   - Type check: the entry gets its own `editor/tsconfig.json` (`module: esnext`, `moduleResolution: bundler`) with `node_modules` types; `lefthook.yml` and CI run `tsc -p crates/df-web/editor` next to the existing check.
2. **`swc` as the bundler (no new tool).** swc's bundler (`spack` / `swcpack`) is reachable only from Node.js: the `bundle()` function of `@swc/core` and the `spack` command of `@swc/cli` (checked in `@swc/core` 1.16.22 and `@swc/cli` 0.8.2). The static `swc` binary that we pin only compiles files one by one (`swc compile`). Using spack would add Node.js to the build, the opposite of a static tool. Upstream does not recommend spack for production and does not develop it actively. Rejected; open question Q2.
3. **`rules_esbuild` (aspect) in Bazel only.** Fits Bazel well, but plain `cargo` still needs an `esbuild` on `PATH`, so it adds a second way to the same output. Rejected for one entry file.
4. **Community prebuilt bundle** (a third-party single-file build of CodeMirror 6, or a bundling CDN such as esm.sh). No build tool, but we run code built by an unknown party's toolchain from sources we cannot check against the published packages; updates depend on that party. Rejected on supply-chain grounds (`AGENTS.md`, Security: this project runs untrusted agent code; the UI must not add more trust).
5. **Check in a bundle we build.** Rejected by the repo rule "no compiled asset is checked in".

The "no bundler" rule remains true for our own component scripts; the editor bundle is the one documented exception (spec delta `web-asset-build`).

### D6. Bundle size and loading

Measured on 2026-11-21 with esbuild 0.28.2, `--bundle --minify --format=iife`, latest packages (`@codemirror/view` 6.43.15, `state` 6.7.6, `language` 6.13.1, `commands` 6.11.1, `lang-markdown` 6.5.2):

| Bundle | Minified | gzip -9 |
|---|---|---|
| Markdown only (view, state, commands with history, language, lang-markdown) | 511 KB | 175 KB |
| + fences: Rust, JSON, YAML, Python, shell (legacy mode) | 664 KB | 232 KB |
| For comparison: Mermaid 12.3.0 `mermaid.min.js` | 5,538 KB | 1,582 KB |
| Alpine 3.17.5 / htmx 4.0.2 | 56 KB / 37 KB | 20 KB / 13 KB |

`lang-markdown` imports `lang-html` (with its JavaScript and CSS parsers) for inline HTML; that is about a third of the base bundle and cannot be tree-shaken. Accepted: the bundle loads once per browser (content-hashed URL, long cache) and only where an editor is.

**Loading:** like Mermaid. `<body data-editor-src="/assets/editor.js?v=…">` when the bundle is embedded. `markdownField` injects the script on first init (a modal opening), once per page; a failed load leaves the textarea. Pages without a Markdown field never load it.

**Fence languages (open question Q5):** recommended set: Rust, TypeScript/JavaScript, JSON, YAML, TOML, shell, Python, SQL, Markdown, diff. Estimate ~250 KB gzip in total. Languages are bundled eagerly; lazy loading per language (`@codemirror/language-data`) needs code splitting, which an IIFE bundle cannot do. Unknown fence languages show as plain code.

### D7. Editor behavior

- **Attach:** `attach(textarea)` creates the editor after the textarea, hides the textarea (`hidden`; it stays in the form and is submitted), copies `id`-based labelling (`aria-labelledby` the `<label>`, `aria-describedby` the hint and error ids, `aria-invalid`) to the editor's content element, and writes the document back to `textarea.value` on every change. On 422 the form is replaced, the component runs again and attaches a new editor.
- **Extensions:** `history`, default keymap, `drawSelection`, `EditorView.lineWrapping`, `markdown({ codeLanguages })`, `syntaxHighlighting(…)`. No autocomplete, no line numbers, no search panel (browser find works on the visible text), no bracket matching. Small surface, fewer keyboard conflicts.
- **Tab key:** `indentWithTab` is **not** used. Tab and Shift+Tab move focus like in a textarea, so keyboard users never get trapped. Indentation: two spaces typed or `Ctrl+]` / `Ctrl+[` (CodeMirror's `indentMore` / `indentLess`), listed in the field's hint. Open question Q7.
- **Escape:** the editor does not bind Escape. In the modal, Escape closes the dialog as today (native `<dialog>`). That loses unsaved text as with the textarea today (backlog: unsaved-changes guard).
- **Theme:** one `EditorView.theme` with CSS variables only (`var(--color-panel)`, `--color-ink`, `--color-line-strong`, focus ring `--color-accent-text`), so light and dark follow `prefers-color-scheme` without code. Markdown structure from tokens: headings `ink` semibold, markers (`#`, `*`, `>`, list bullets, fence lines) `faint`, links `accent-text`, inline code on `sunk`, emphasis italic, strong semibold.
- **Code inside fences uses the configured code theme:** the `HighlightStyle` assigns syntect's class names instead of colors (`tags.keyword → "hl-keyword"`, `tags.string → "hl-string"`, `tags.comment → "hl-comment"`, numbers `hl-constant hl-numeric`, types `hl-storage hl-type`, …). The existing `/assets/code-theme.css` colors them, so a fence looks the same in the editor and in the preview, and an operator's `web.codeTheme.*` applies to both. The mapping is approximate (syntect scopes are finer than Lezer tags); a tag without a match stays `ink`.
- **Size:** min height from `rows`, grows with content up to `70vh`, then scrolls inside.

### D8. Phones and screen readers

- **Phones:** below 768 px, or with `pointer: coarse` and no fine pointer, the component does not attach CodeMirror and keeps the textarea with preview (recommended, open question Q4). Reasons: `contenteditable` editors have IME, autocorrect and selection-handle quirks on Android and iOS; long skill texts are written on desktops; the bundle does not load on phones.
- **Screen readers:** CodeMirror 6's content is a `contenteditable` with `role="textbox"` and `aria-multiline="true"`; NVDA, JAWS and VoiceOver read it, but less fluently than a textarea (line-by-line reading works, some announce highlighting spans). The component honours a per-browser switch "Plain text editor" (a small text button next to the tabs, stored in `localStorage['df.editor.plain']`) that keeps the textarea. Open question Q8.
- **Reduced motion:** the editor has no animations.

### D9. Tests

- Rust (every build): component HTML (label, tabs hidden until Alpine, textarea name and value, `aria-describedby`, no tabs with `preview: false`), preview route (login required, missing htmx header → 403, escaped script, link `rel`, Mermaid fence markup, 413 above the limit, empty text), persona form uses the field, `data-editor-src` only with the bundle.
- TypeScript sources: tests that read `markdownField.ts` (`script_source`) for the load-once logic and the phone check, as the existing component tests do.
- Compiled bundle: tests with `#[cfg_attr(not(df_web_editor), ignore = "…")]` check that `/assets/editor.js` is served with the hash and defines `DfMarkdownEditor`.
- E2E (`crates/df-e2e`): persona form: type Markdown with a Rust fence, open Preview, see highlighted code; with the editor bundle: highlighting spans present, Tab moves focus to the next field, save stores the edited text.

## Risks / Trade-offs

- [Bundle grows with each fence language and CodeMirror release] → fixed language list; a test asserts the bundle stays below 300 KiB gzip, so growth is a visible decision.
- [`esbuild` is a new pinned tool for contributors] → same path as `swc` and `tailwindcss` (`bazel run //:bazel_env`); without it, plain `cargo` warns and the UI still works (textarea with preview).
- [CodeMirror packages must resolve to one copy of `@codemirror/state`, otherwise the editor throws at start] → pnpm lockfile, all `@codemirror/*` bumped together (Renovate group); the bundle test catches an error at load.
- [Editor and hidden textarea out of sync at submit] → the textarea is updated on every change (not only on submit), so htmx serializes the current text; e2e test saves after editing.
- [Highlighting colors of the editor differ slightly from the preview (Lezer tags vs. syntect scopes)] → accepted; the preview is the reference.
- [Preview route as a CPU sink] → login required, 256 KiB limit, syntect's 64 KiB per-fence cap; same cost as opening a large artifact.
- [Screen reader users get a weaker field] → plain-text switch (D8), Tab never trapped, label and errors wired to the editor.
- [Firefox without `field-sizing` (step 1)] → fixed height with resize handle; step 2 grows on all browsers.

## Migration Plan

No data migration. Step 1 and step 2 deploy as normal releases. Rollback of step 2: remove the bundle from the build; the component falls back to the textarea without code changes.

## Open Questions

Each with a recommendation. None changes the step 1 specs.

- **Q1. CodeMirror in this change, or a follow-up change?** Recommendation: in this change as step 2, a separate pull request, so skills (iteration 4) get highlighting from the start.
- **Q2. Accept `esbuild` as a third pinned asset tool and one bundled script?** Recommendation: yes (D5 option 1). Without it, stop at step 1.
- **Q3. Side-by-side Write and Preview on wide screens?** Recommendation: not now. The persona and skill modals are at most ~720 px wide; side by side needs a wide modal or a full page. Revisit when the skill form exists (backlog row).
- **Q4. CodeMirror on phones?** Recommendation: no; phones keep the textarea with preview (D8).
- **Q5. Which fence languages?** Recommendation: Rust, TypeScript/JavaScript, JSON, YAML, TOML, shell, Python, SQL, Markdown, diff (~250 KB gzip).
- **Q6. Memory content in the editor?** `iter6-agent-memory` says memory content is plain text and never rendered as Markdown. Recommendation: the Memory tab uses `markdown_field` with `preview: false` and no Markdown highlighting (plain textarea, monospace, auto-grow), which keeps that rule. If memories should become Markdown, that proposal changes first.
- **Q7. Tab key:** focus out (recommended, accessible, like a textarea) or indent (like a code editor, with Escape-then-Tab to leave)? Recommendation: focus out; indent with `Ctrl+]`.
- **Q8. Plain-text switch for screen reader and keyboard users:** offer it (recommended) or rely on CodeMirror's accessibility alone?
- **Q9. Preview rendering variant:** `render` (single newline is a soft break, recommended) or hard breaks like user messages?
