# Design

## Context

Motivation and scope: see `proposal.md`. Current state that shapes the design:

- **Fields.** `textarea_field` (`crates/df-web/src/components/forms.rs`) renders a labelled `<textarea>` with `INPUT_CLASS` (16 px sans text, no auto-grow). The persona form calls it with 8 rows for `system_prompt` (`config_pages/personas.rs`, `persona_form`). The form lives in a `dialog_with(…, DialogStyle::Form, …)` modal: a centered card from 768 px, a full-height sheet below. It posts with htmx (`hx-post`, `#form`, 422 re-renders the form with values). Creating and editing a persona both use this modal ("Configuration forms as modals").
- **Settings pages.** Projects (`/projects/{key}`) and model providers (`/model-providers/{id}`) are created in a modal and then edited on a settings page: `settings_page_frame` (settings sidebar, back link, heading, the save button in the header), `tabbed_form(…, TabsStyle::Page)` (tab row on top, open tab in `?tab=`, posted as `tab`), `saved_response` (form swap, heading out of band, "… saved" status in a live region). The open changes `persona-identity` and `persona-commit-signing` already speak of a "persona page".
- **Renderer.** `markdown::render` (`src/markdown.rs`) is the single Markdown renderer: comrak with raw HTML escaped, unsafe links dropped, images as links, `rel="noopener noreferrer nofollow"`, syntect highlighting with `hl-` classes (64 KiB per fence), Mermaid fences as `<pre x-data="mermaidDiagram">` that `scripts/markdown.ts` draws in Mermaid's sandbox iframe. The code theme stylesheet `/assets/code-theme.css` switches with `prefers-color-scheme`.
- **Scripts.** `scripts/*.ts` are classic scripts compiled one by one by `swc`; no `import`/`export`, no bundler (`crates/df-web/AGENTS.md`). Third-party scripts are dist files of npm packages, pinned in `package.json`, locked in `pnpm-lock.yaml`, staged by `build_assets.rs` (`VENDOR`) and by Bazel (`:vendor_scripts` from `npm_link_all_packages`). Mermaid (5.5 MB minified) is loaded only when a page has a diagram: `<body data-mermaid-src>` names its URL, the component injects the `<script>` on first use.
- **Tools.** `swc` and `tailwindcss` are pinned in `multitool.lock.json`, used by Bazel genrules and put on `PATH` by `bazel run //:bazel_env`. Plain `cargo` runs them from `PATH`; a missing tool warns and sets no cfg (`df_web_scripts`, `df_web_styles`, `df_web_vendor`).
- **CSRF.** `auth::csrf_guard` rejects a non-GET request without the htmx header; with `SameSite=Lax` this is the CSRF protection of every form.
- **Tabs.** The Alpine component `tabs` (`scripts/tabs.ts`) implements the WAI-ARIA tab pattern (arrow keys, Home and End, `role=tabpanel`).
- **No JavaScript.** The settings pages post with htmx and switch tabs with Alpine, so they need JavaScript. "Without JavaScript" in this design means: the field's script did not load or failed. The field then works as a plain textarea.

## Goals / Non-Goals

**Goals:**

- One component for every Markdown field, so skills and later fields get the same behavior by one call.
- A preview that is byte-equal to what the UI shows elsewhere, with the same sanitizing.
- Highlighting while typing that degrades to a working textarea in every failure case: no tool at build time, script load error, phone, screen reader preference.
- No new trust in third-party build artifacts.

**Non-Goals:**

- Highlighting of non-Markdown files, an unsaved-changes guard: backlog row "Follow-ups of `markdown-editor`" in `product/iterations/roadmap.md`.
- Scroll sync between editor and preview: rejected (decided by Peter, PR #303).
- Collaborative editing, autosave, drafts: rejected (forms save on submit).
- Spell checking inside CodeMirror beyond what the browser offers: rejected.
- Markdown linting (heading levels, line length): rejected.

## Decisions

### D1. Two steps on one component; both in this change

A PenPot design sprint comes first (task group 1): the persona settings page, the System prompt tab with its views (Split, Editor, Preview) at 1440 px and 1024 px, the create modal, phone and dark. Its approved boards fix the labels and sizes the specs name.

Step 1 (component, preview route, persona settings page) needs no build change. Step 2 (CodeMirror) changes the asset build. Both steps share the markup contract: the `<textarea name=…>` is always the form field, and the component enhances it. Step 2 only adds a layer, no change to the server side or to callers.

Both steps are in this change and in one pull request (decided, Q1). The steps stay as the order of work: step 1 (task groups 2–5) works without the bundle, which is also the fallback when the bundle is missing; step 2 (groups 6–8) adds the layer.

- *Alternative: CodeMirror only in a later change.* Rejected (Q1): the editor that skills get in iteration 4 would be without highlighting, and highlighting in long skill texts is the main benefit.
- *Alternative: CodeMirror first, preview later.* Rejected: the build pipeline is the risky part; the preview is cheap and useful alone.

### D2. Component API and markup

```rust
pub struct MarkdownField<'a> {
    pub label: &'a str,
    pub name: &'a str,
    pub value: &'a str,
    pub hint: Option<&'a str>,
    pub rows: u8,              // minimum height
    pub preview: Preview,
    pub label_hidden: bool,    // label for screen readers only (the field fills a page tab of that name)
}
pub enum Preview {
    Switch,  // view switch Editor | Preview (modals, narrow places)
    Split,   // Editor | Split | Preview from 1280 px, opens on Split; Editor | Preview below (D2a)
}
pub fn markdown_field(field: &MarkdownField<'_>, errors: &FormErrors) -> Markup
```

Only the two layouts with a caller in this change exist. The source-only variant for memory content (decided, Q6) is specified in D7a and added by `iter6-agent-memory`, which has its caller; a field without preview and without highlighting is `textarea_field`.

Markup (step 1), simplified:

```html
<div class="space-y-1" x-data='markdownField({"split": true})'>
  <label for="system_prompt" class="sr-only">System prompt</label>   <!-- visible unless label_hidden -->
  <div class="relative flex justify-center" x-cloak x-show="enhanced">
    <div role="radiogroup" aria-label="View" @keydown="move($event)">
      <button type="button" role="radio" aria-checked="false">(icon) Editor</button>
      <button type="button" role="radio" aria-checked="true" class="max-xl:hidden">(icon) Split</button>
      <button type="button" role="radio" aria-checked="false">(icon) Preview</button>
    </div>
    <button type="button" role="switch" aria-checked="true" class="absolute right-0" x-show="editorPossible">Highlighting</button>  <!-- step 2 -->
  </div>
  <div class="… xl:grid-cols-2">                     <!-- Split: two columns; Editor or Preview: one -->
    <div id="system_prompt-write">
      <textarea id="system_prompt" name="system_prompt" rows="8" class="… font-mono text-sm"
                aria-describedby="…">…</textarea>
    </div>
    <div id="system_prompt-preview" class="markdown …" aria-live="polite">…</div>
  </div>
  <p id="system_prompt-hint">…</p> <p id="system_prompt-error" role="alert">…</p>
</div>
```

- **View switch** (decided by Peter, 2026-11-26, Q16): a segmented control centred above the field, each segment an icon (Lucide `pencil`, `columns-2`, `eye`) plus its name. It follows the WAI-ARIA radio group pattern (one tab stop; arrow keys, Home and End move and select). Tabs do not fit: Split shows both panels. The switch is hidden until the component runs (`x-cloak`), so the no-JS field is a plain labelled textarea. The selected view is component state; it is not written to the URL and not remembered.
- `markdownField` is a new Alpine component (`scripts/markdownField.ts`) with its own `move(event)`, so the switch's `@keydown="move($event)"` never falls through to the `tabs` scope of the surrounding `tabbed_form` (which would set `tab = ''` and hide every panel). The switch renders no `role="tab"`, so the settings page's `tabs` component, which queries `[role="tab"]` in the whole form, is unaffected; `tabs.ts` needs no change. A page test covers a settings page with the field.
- **Label:** visible by default. `label_hidden` keeps it for screen readers only (`sr-only`) where the field fills a page tab of the same name (the System prompt tab), so the name is not repeated above the editor (Q16).
- The textarea uses `font-mono text-sm` (13–14 px, like code) from 1024 px and keeps 16 px below, so phones do not zoom on focus.
- **Auto-grow:** CSS `field-sizing: content` with `min-height` from `rows` and `max-height: 70vh` (then it scrolls). In browsers without `field-sizing` (Firefox at the time of writing; `CSS.supports('field-sizing', 'content')` is false), `markdownField` sets `height = min(scrollHeight, 70vh)` on `input` and whenever the field becomes visible (an `IntersectionObserver` or the view change; a hidden panel measures `scrollHeight` as 0), so the field grows to the same maximum in every browser. Without scripts the field keeps its `rows` height and a resize handle. *Alternative:* fixed height without a script fallback: rejected, it breaks the growth requirement in Firefox. With CodeMirror (step 2) the editor grows by itself up to the same maximum.

### D2a. Split layout and live preview

`Preview::Split` is for a field that has a page or tab of its own (the System prompt tab, later a skill body). From 1280 px its switch offers Editor, Split and Preview and opens on Split: the editor on the left and the preview on the right, each half the width; Editor and Preview show one panel at full width. Below 1280 px the switch offers Editor and Preview, like `Preview::Switch`, and opens on Editor; when the window narrows below 1280 px while Split is selected, the view becomes Editor. `Preview::Switch` (modals, narrow places) offers Editor and Preview and opens on Editor. Nothing is remembered between visits (Q16). At 1440 px the settings area leaves about 1,100 px for the page, so each column is about 530 px: enough for prose and for code lines of about 60 characters. Long code lines wrap in the editor and scroll inside their block in the preview.

- **Height.** In Split both columns share one height: from `rows` up to `calc(100vh - <page header>)`, then each column scrolls on its own. The editor and the preview never scroll in sync (decided, Peter, PR #303).
- **First preview from the server.** In this layout the server renders the preview of the current value into the page (`markdown::render`, the same call as the route), so the preview panel is filled when the tab opens, after a save and after a 422 (both swap `#form` with a new server render), and also without scripts. The live preview takes over from there.
- **Live preview.** While Split is shown the preview follows the text (in Editor no request; opening Preview requests like `Preview::Switch`, D3): the component posts the text 500 ms after the last change (debounce), with the request of D3. Rules that keep it cheap and calm:
  - one request at a time: a newer text aborts the request in flight (`AbortController`); a response for an older text is dropped;
  - no request if the text equals the last rendered text (the server-rendered first preview counts as rendered);
  - the swap morphs the preview (`htmx.morph` / the morph htmx uses for `outerMorph` live regions), so unchanged blocks and the scroll position stay;
  - **Mermaid:** a fence whose source did not change keeps its drawn diagram, so the morph must not touch it. Today `mermaidDiagram` reads its source once in `init` and re-inserts its sibling diagram when a morph removes it (`markdown.ts`, `apply()`), which reloads the iframe. Change: the component keeps the `<pre>` and its sibling `<div class="diagram">` as one unit (the morph skips the diagram `<div>`, for example with `hx-preserve` / `data-morph-ignore`), and redraws only when the `<pre>` text changes. Testable: after an update that does not touch the fence, the diagram's `iframe` is the same DOM node;
  - a failed request keeps the last preview and shows "Preview not up to date" in `faint` above it; the next change retries.
- **Cost.** Rendering is fast (comrak and syntect; a 20 KiB prompt takes a few milliseconds), a typing pause of 500 ms gives at most two requests per second per editor, and every request carries at most 256 KiB. No rate limit beyond the login and the size limit.
- **Effort.** Small: the debounce, abort and morph are a few lines in `markdownField.ts` plus `script_source` tests; the only fiddly part is keeping Mermaid diagrams from flickering, which the morph and the source check cover.
- *Alternative: live preview in the Editor view too.* Rejected: the preview is hidden while writing, so a request per pause would be wasted.
- *Alternative: tabs "Write" and "Preview" next to the label, no Split choice.* Rejected after the PenPot review (Q16): the tabs beside the label and a separate plain-text control on the right read oddly, and a wide screen had no way to give the editor or the preview the full width.
- *Alternative: render in the browser for zero latency.* Rejected (D3): a second renderer.

### D2b. Persona settings page

Built from the existing parts of the project and model provider pages; no new layout pattern.

- **Route:** `GET /personas/{id}?tab=` (unknown id → 404, unknown tab → General), `POST /personas/{id}` saves (posted `tab` keeps the open tab), `/personas/{id}/edit` is removed (404, like `/model-providers/{id}/edit`).
- **Frame:** `settings_page_frame("persona-settings", back "Personas", heading, "Save persona", form)`; the heading shows avatar, display name and "harness · model".
- **Tabs** (`tabbed_form(…, TabsStyle::Page)`, decided by Peter, 2026-11-25, after `persona-identity` and `persona-commit-signing` landed): **General** with the groups "Identity" (avatar preview, display name, handle, description) and "Agent" (harness, model provider, model; the selects keep `hx-post="/personas/refresh"`, which then swaps only the General panel's Agent group instead of the whole form); **System prompt** with `markdown_field(Preview::Split, rows: 20, label_hidden: true)` and no other field, so the editor uses the full width; **Commits** with the group "Commits" of `signing_key.rs` (commit email, public key, fingerprint, "Rotate key", "Verify key on Forgejo"). `?tab=system-prompt` and `?tab=commits` name them; General is left out of the URL. *Alternative:* the group "Commits" on General. Rejected: General gets long, and the key has room on its own tab like a project's Security tab.
- **Save:** one form for all three tabs, like the project page. A 422 opens the tab of the first error (`tabbed_form` already does this), for example Commits for a taken commit email, and keeps all entered values, including the system prompt. Success: `saved_response` with "Persona saved". The persona list's card links to the page; its edit button becomes a link.
- **Commits tab:** reuses the markup of `signing_key.rs`. "Rotate key" and "Verify key on Forgejo" keep their routes (`POST /personas/{id}/rotate-key`, `/personas/{id}/verify-key`) and dialogs; they are not part of "Save persona", as in the modal today. Their responses swap the Commits group on the page instead of the modal's.
- **Create:** the "New persona" modal keeps the groups "Identity", "Agent" (harness, model provider, model; no system prompt) and "Commits" (commit email and "No signing key yet. Save the persona to create it."). Saving answers `HX-Redirect: /personas/{id}?tab=system-prompt`, so the board member writes the prompt with the full editor next. Decided (Q12).
- **System prompt becomes optional.** Today `persona_values` requires it ("System prompt is required.", `personas.rs`, test in `config_pages/tests/personas.rs`), so the create modal without the field would always fail with 422. The check is removed: an empty system prompt is valid on create and on save. The column stays `TEXT NOT NULL` (empty string). A run of such a persona gets no system prompt file (`.claude/CLAUDE.md`, `.config/opencode/AGENTS.md`; `assign.rs` writes the file only when it has content, the sandbox section included), so the harness runs with its defaults. The persona card shows "No system prompt yet" in `faint` where it would show the start of the prompt (when the description is empty too), so an unfinished persona is visible in the list. Decided (Q15). *Alternative:* keep a required prompt in the create modal. Rejected by Q12.
- **Phones:** below 768 px the page is a full page with the scrolling tab row ("Form tabs on phones"); the System prompt tab shows the switch Editor | Preview with the plain textarea (D8).
- **Later tabs:** skills (`iter4-skills` assignment) fit as one more tab; this change does not add it.

### D3. Preview route

`POST /markdown/preview`, inside the board-member router (login) and behind `csrf_guard` (htmx header), form body `source=<text>`.

- **Rendering:** `markdown::render(source)`, the variant for agent text and artifacts. Single newlines are soft breaks, as an agent and the Markdown artifact view see them. *Alternative:* `render_with_hard_breaks` (user messages): rejected, skills and prompts are documents, not chat messages.
- **Response:** `200` with the rendered fragment wrapped in `<div class="md-body">` (the class of the existing Markdown styles); empty source gives "Nothing to preview" in `faint`. Mermaid fences come back as the usual `<pre x-data="mermaidDiagram">`; Alpine initializes them when htmx swaps the fragment in, so diagrams draw in their sandbox like everywhere else.
- **Limit:** the limit is on the decoded text: 256 KiB of `source` in UTF-8 bytes (the skill size limit of `iter4-skills`). URL form encoding can make the body up to three times larger (`#` becomes `%23`), so the route sets `DefaultBodyLimit::max(1 MiB)` to accept every valid text, then checks `source.len()` after decoding. A text over 256 KiB, or a body over 1 MiB, gets `413` and the preview shows "Text too long for a preview (max 256 KiB)". The form itself still saves; the limit is only for preview. Syntect's 64 KiB per fence cap applies as usual.
- **No storage, no audit:** the route is a pure function of its input; it writes nothing and logs no content.
- **Request from the client:** a plain `fetch`, not `htmx.ajax`. htmx 4's `htmx.ajax` would follow the `HX-Redirect: /login` of an expired session (a background request would leave the page and lose the text), swaps error bodies (it swaps every status but 204 and 304), serializes the whole form when the source is an element inside it, and queues instead of replacing. The component therefore sends:
  - `fetch('/markdown/preview', { method: 'POST', body: new URLSearchParams({ source }), headers: { 'HX-Request': 'true' }, redirect: 'manual', signal })`: only the field's text, the header the CSRF guard needs, no redirect followed, and an `AbortController` signal so a newer request cancels the older one;
  - swap only on `200`; `413` shows "Text too long for a preview (max 256 KiB)", `401` shows "Preview not available: your session has ended. Copy your text, then reload." (the text stays in the field), any other status or a network error shows "Preview not available" with a "Retry" button (Preview view) or "Preview not up to date" (Split, D2a). The last good preview is never replaced by an error body.
  The request runs when the Preview view opens; in Split 500 ms after the last change (D2a). If the text did not change since the last preview, the old preview is shown without a request. The editor is unaffected by any error.
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
   - **Tool updates:** `cargo xtask update-tools` rebuilds `multitool.lock.json` from GitHub release assets only (`tools/xtask/src/main.rs`, `TOOLS`), and esbuild publishes its binaries only on npm. A hand-added entry would be dropped by the weekly update and fail `--check`. So xtask gets a second source kind, "npm tarball" (registry metadata `https://registry.npmjs.org/@esbuild/<platform>/<version>`, `dist.tarball`, sha256 of the downloaded tarball, file `package/bin/esbuild`), with unit tests like the GitHub source.
   - Entry: `crates/df-web/editor/markdown-editor.ts`, an ES module that imports the CodeMirror packages and exposes one global, `window.DfMarkdownEditor = { attach(textarea, options) }`. It lives outside `scripts/`, so the classic-script rule for `scripts/*.ts` stays unchanged.
   - Output: `esbuild --bundle --minify --format=iife --target=es2022` to one classic script, served as `/assets/editor.js` (hashed URL like all assets).
   - Packages: added to `crates/df-web/package.json` (`@codemirror/*`, `@lezer/highlight`), locked in `pnpm-lock.yaml`, bumped by Renovate. Bazel gets them from `npm_link_all_packages` (already used for `VENDOR`); plain `cargo` from `crates/df-web/node_modules` (already needed for the vendor scripts).
   - Bazel: genrule `:editor_bundle` (esbuild + the linked packages), passed to the build script as `EDITOR_BUNDLE`; with `DF_WEB_REQUIRE_ASSETS=1` a missing bundle fails the build. esbuild resolves imports from the `node_modules` tree that `npm_link_all_packages` creates under `bazel-bin/crates/df-web/node_modules` (`NODE_PATH` pointing there, or running esbuild from that directory), not from a source-tree `node_modules`.
   - Plain `cargo`: `build_assets.rs` runs `esbuild` from `PATH` (or `ESBUILD_BIN`) when the CodeMirror packages are installed. Missing tool or packages → `cargo:warning` naming them, no bundle, cfg `df_web_editor` not set. The page then has no `data-editor-src` and the field stays a textarea with preview.
   - Type check: the entry gets its own `editor/tsconfig.json` (`module: esnext`, `moduleResolution: bundler`) with the types of the CodeMirror packages from `crates/df-web/node_modules`. The current `web-typecheck` hook and the CI step run `tsc` without installing packages, so:
     - CI: a step `pnpm install --frozen-lockfile` in `crates/df-web` before `tsc -p crates/df-web/editor` (the existing `tsc -p crates/df-web` needs no packages and stays as it is);
     - hook: `web-typecheck` also runs on `crates/df-web/editor/**`; when `crates/df-web/node_modules` is missing it skips the editor check with a message naming `pnpm install --frozen-lockfile` and exits 0, because CI enforces it.
2. **`swc` as the bundler (no new tool).** swc's bundler (`spack` / `swcpack`) is reachable only from Node.js: the `bundle()` function of `@swc/core` and the `spack` command of `@swc/cli` (checked in `@swc/core` 1.16.22 and `@swc/cli` 0.8.2). The static `swc` binary that we pin has a `swc bundle` subcommand, but it only panics: `unimplemented!("Bundle command is not yet implemented")` (`swc_cli_impl` 85.0.0, `src/commands/bundle.rs`). The swc docs (swc.rs, "Bundling") say spack "will be dropped in v2, in favor of SWC-based bundlers like Parcel 2, Turbopack, Rspack", tell users to "use one of the bundlers instead", and state that it currently requires CommonJS, while CodeMirror 6 ships only ES modules. Using spack would also add Node.js to the build. Rejected; open question Q2.
3. **`rules_esbuild` (aspect) in Bazel only.** Fits Bazel well, but plain `cargo` still needs an `esbuild` on `PATH`, so it adds a second way to the same output. Rejected for one entry file.
4. **Community prebuilt bundle** (a third-party single-file build of CodeMirror 6, or a bundling CDN such as esm.sh). No build tool, but we run code built by an unknown party's toolchain from sources we cannot check against the published packages; updates depend on that party. Rejected on supply-chain grounds (`AGENTS.md`, Security: this project runs untrusted agent code; the UI must not add more trust).
5. **Check in a bundle we build.** Rejected by the repo rule "no compiled asset is checked in".

The "no bundler" rule remains true for our own component scripts; the editor bundle is the one documented exception (spec delta `web-asset-build`).

### D5a. `esbuild` for the other assets (follow-up, Q14)

Once `esbuild` is pinned, it could do more. What the other assets need today:

| Asset | Today | Already minified? | What `esbuild` could change |
|---|---|---|---|
| Component scripts (`scripts/*.ts`, 10 files) | `swc compile`, one file each, `.swcrc` with `minify`, `compress`, `mangle` | yes | **Replace `swc`**: `esbuild <file>.ts --minify --target=es2022`, one file each, no bundling. Same job (strip types, minify), one pinned tool fewer. |
| Component scripts, bundled | – | – | One `app.js` instead of 10 files: fewer requests, but any change to one component invalidates the cached bundle, and the load order before Alpine must be kept by hand. Little gain: the files are small, compressed, cached by content hash and loaded over one HTTP/2 connection. |
| Stylesheet (`styles/input.css`) | `tailwindcss --minify` | yes | Nothing: `esbuild` cannot run Tailwind, and Tailwind already minifies. |
| Vendor scripts (Alpine, htmx, Mermaid) | served byte for byte from npm | yes (published `.min.js`) | Nothing worth it: re-bundling them breaks "byte for byte as published", which keeps Renovate bumps reviewable and the supply chain traceable. |

**Decided (Q14):** replace `swc` with `esbuild` for the component scripts (per file, no bundling) in a follow-up change, so this change stays about the editor. Effort: `build_assets.rs` (tool and arguments), the Bazel genrule `:web_scripts`, `.swcrc` removed, `multitool.lock.json` and `bazel_env` without `swc`, docs and `crates/df-web/AGENTS.md`; type checking stays with `tsc` (neither tool checks types). Risk is low: the scripts are classic scripts without `import`, and `esbuild` without `--format` keeps their top-level names. Do not bundle the component scripts, the stylesheet or the vendor scripts.

### D6. Bundle size and loading

Measured on 2026-11-21 with esbuild 0.28.2, `--bundle --minify --format=iife`, latest packages (`@codemirror/view` 6.43.15, `state` 6.7.6, `language` 6.13.1, `commands` 6.11.1, `lang-markdown` 6.5.2):

| Bundle | Minified | gzip -9 |
|---|---|---|
| Markdown only (view, state, commands with history, language, lang-markdown) | 511 KB | 175 KB |
| + fences: Rust, JSON, YAML, Python, shell (legacy mode) | 664 KB | 232 KB |
| For comparison: Mermaid 12.3.0 `mermaid.min.js` | 5,538 KB | 1,582 KB |
| Alpine 3.17.5 / htmx 4.0.2 | 56 KB / 37 KB | 20 KB / 13 KB |

`lang-markdown` imports `lang-html` (with its JavaScript and CSS parsers) for inline HTML; that is about a third of the base bundle and cannot be tree-shaken. Accepted: the bundle loads once per browser (content-hashed URL, long cache) and only where an editor is.

**Loading:** like Mermaid. `<body data-editor-src="/assets/editor.js?v=…">` when the bundle is embedded. `markdownField` injects the script on first init (a page or modal with the field), once per page; a failed load leaves the textarea. Pages without a Markdown field never load it.

**Fence languages (decided, Q5):** the set: Rust, TypeScript/JavaScript, JSON, YAML, TOML, shell, Python, SQL, Markdown, diff. Estimate ~250 KB gzip in total. Languages are bundled eagerly; lazy loading per language (`@codemirror/language-data`) needs code splitting, which an IIFE bundle cannot do. Unknown fence languages show as plain code.

### D7. Editor behavior

- **Attach:** `attach(textarea)` creates the editor after the textarea, hides the textarea (`hidden`; it stays in the form and is submitted), copies `id`-based labelling (`aria-labelledby` the `<label>`, `aria-describedby` the hint and error ids, `aria-invalid`) to the editor's content element, and writes the document back to `textarea.value` on every change. On 422 the form is replaced, the component runs again and attaches a new editor.
- **Extensions:** `history`, default keymap, `drawSelection`, `EditorView.lineWrapping`, `markdown({ codeLanguages })`, `syntaxHighlighting(…)`. No autocomplete, no line numbers, no search panel (browser find works on the visible text), no bracket matching. Small surface, fewer keyboard conflicts.
- **Tab key:** `indentWithTab` is **not** used. Tab and Shift+Tab move focus like in a textarea, so keyboard users never get trapped. Indentation: two spaces typed or `Ctrl+]` / `Ctrl+[` (CodeMirror's `indentMore` / `indentLess`; `Cmd` on macOS). Decided (Q7), with a hint in the UI: while the highlighting editor is attached, a one-line hint in `faint` below the field reads "Tab moves to the next field. Indent with Ctrl+] and Ctrl+[." (`Cmd` on macOS), linked by `aria-describedby`. The plain textarea shows no hint, because there Tab works as everywhere else.
- **Escape:** the editor does not bind Escape. In a modal (later users), Escape closes the dialog as today (native `<dialog>`). On the persona page there is no dialog; leaving the page loses unsaved text as on the project page today (backlog: unsaved-changes guard).
- **Theme:** one `EditorView.theme` with CSS variables only (`var(--color-panel)`, `--color-ink`, `--color-line-strong`, focus ring `--color-accent-text`), so light and dark follow `prefers-color-scheme` without code. Markdown structure from tokens, except headings: headings semibold in the code theme's function-name color (the `HighlightStyle` gives heading text the classes `hl-entity hl-name hl-function`, so `/assets/code-theme.css` colors it and an operator's `web.codeTheme.*` applies; defaults `#795DA3` light, 5.37:1 on `panel`, and `#8FA1B3` dark, 6.59:1; a theme without that rule leaves headings `ink`). The themes' own Markdown rules (`markup.heading`, …) are not used: `InspiredGitHub` gives headings no color and scopes its rules under `.hl-markdown`, and `base16-ocean.dark` would add five colors (bold, italic, lists, quotes). Markers (`#`, `*`, `>`, list bullets, fence lines) `faint`, links `accent-text`, inline code on `sunk`, emphasis italic, strong semibold.
- **Code inside fences uses the configured code theme:** the `HighlightStyle` assigns syntect's class names instead of colors (`tags.keyword → "hl-keyword"`, `tags.string → "hl-string"`, `tags.comment → "hl-comment"`, numbers `hl-constant hl-numeric`, types `hl-storage hl-type`, …). The existing `/assets/code-theme.css` colors them, so a fence looks the same in the editor and in the preview, and an operator's `web.codeTheme.*` applies to both. The mapping is approximate (syntect scopes are finer than Lezer tags); a tag without a match stays `ink`.
- **Size:** min height from `rows`, grows with content up to `70vh` (Split: the shared column height of D2a), then scrolls inside.

### D7a. Source highlighting without preview (memory content, for `iter6-agent-memory`)

Decided here (Q6), built by `iter6-agent-memory`: that change has the Memory tab, the only caller, so it adds the variant (`Preview::SourceOnly`) and its tests. This change adds nothing for it; the rules below are its input. The rules it must follow:

`Preview::SourceOnly` is for agent-written text that must be shown exactly as the model reads it (decided, Q6). The reason for the memory rule (`iter6-agent-memory` design D1: "the UI shows exactly that text, so no link or image in a memory can trick a reviewer") holds: every character stays visible as typed and in order; nothing is rendered as HTML, no link is followed, no image is loaded, no diagram is drawn. CodeMirror writes the document as text nodes and spans with classes, never as HTML. The remaining risk is styling that draws the eye away from text, so this variant has a stricter theme:

- No token fainter than `muted` (light: 7.6:1 on `panel`, 6.99:1 on `sunk`; dark: 11.7:1 and 10.2:1): markers and HTML comments use `muted`, not `faint`, so an instruction cannot hide in a dim comment or link target. No token is smaller, hidden or collapsed.
- Code inside fences does not use the code theme (some bundled themes color comments below 4.5:1); it is `ink` on `sunk`, without token colors.
- Headings keep the body font size (semibold only), so a content line cannot look like UI structure.
- No clickable links, no link or image extensions, no Mermaid, no view switch, no preview request.
- Without the bundle, on phones or with "Highlighting" off, the field is the plain textarea (monospace, growing height).

Agent input is bounded (4096 bytes; no `Cc`/`Cf` except `\n` and `\t`), so parsing in the viewer's browser costs nothing measurable. `iter6-agent-memory` rewords its rule to "never rendered as Markdown; the editor MAY highlight Markdown syntax"; that proposal makes the edit, not this change.

### D8. Phones and screen readers

- **Phones and tablets:** below 768 px the component does not attach CodeMirror and keeps the textarea with preview (decided, Q4: phones). From 768 px it attaches on every device, tablets with only a touch screen included (decided by Peter, PR #303: a 13" iPad gets highlighting). The pointer type decides neither the editor nor the layout. Text is 16 px below 1024 px and with `(pointer: coarse)` and no fine pointer (`any-pointer: fine`), so iOS and Android do not zoom on focus; that is the only use of the pointer type. If CodeMirror behaves badly on a touch device (IME, autocorrect, dictation, selection handles), the "Plain text editor" switch turns it off for that browser. Reasons for the phone limit: those quirks hurt most on small screens, long texts are not written on phones, and phones do not load the bundle.
- **Screen readers:** CodeMirror 6's content is a `contenteditable` with `role="textbox"` and `aria-multiline="true"`; NVDA, JAWS and VoiceOver read it, but less fluently than a textarea (line-by-line reading works, some announce highlighting spans). The component offers a per-browser switch "Highlighting" (`role="switch"`, on by default) at the right end of the view switch row; off keeps the textarea and is stored in `localStorage['df.editor.plain']`. It is shown only where the highlighting editor can attach (bundle embedded, viewport from 768 px). Decided (Q8; label and place Q16).
- **Reduced motion:** the editor has no animations.

### D9. Tests

- Rust (every build): component HTML for each `Preview` (label, `label_hidden` as `sr-only`, view switch hidden until Alpine with Split only from 1280 px, split classes from 1280 px, server-rendered first preview with `Preview::Split`, textarea name and value, `aria-describedby`), preview route (login required, missing htmx header → 403, escaped script, link `rel`, Mermaid fence markup, 413 above the limit, empty text, and a log-capture test (`df-telemetry` `test-util`) that the text does not appear in the logs), persona settings page (404, `?tab=`, save stays on the tab, 422 opens the tab of the first error, old `/edit` route 404, create without a system prompt redirects to the System prompt tab, card text for an empty prompt), `data-editor-src` only with the bundle.
- TypeScript sources: tests that read `markdownField.ts` (`script_source`) for the debounce, abort and "unchanged text" logic, `redirect: 'manual'`, swap only on 200, the load-once logic and the phone check, as the existing component tests do. `tabs.ts` stays unchanged (D2); a page test covers a settings page with the field.
- Compiled bundle: tests with `#[cfg_attr(not(df_web_editor), ignore = "…")]` check that `/assets/editor.js` is served with the hash and defines `DfMarkdownEditor`.
- E2E (`crates/df-e2e`): persona settings page at 1440 px: the preview is filled when the tab opens; type Markdown with a Rust fence, the Split preview shows highlighted code after the pause; a drawn Mermaid diagram keeps its `iframe` node while a paragraph below changes; Editor and Preview show one panel at full width; at 1024 px: no Split, open Preview, see the same; save and reload keeps the text; with the editor bundle: highlighting spans present, Tab moves focus to the next field, save stores the edited text.

## Risks / Trade-offs

- [Bundle grows with each fence language and CodeMirror release] → fixed language list; a test asserts the bundle stays below 300 KiB gzip, so growth is a visible decision.
- [`esbuild` is a new pinned tool for contributors] → same path as `swc` and `tailwindcss` (`bazel run //:bazel_env`); without it, plain `cargo` warns and the UI still works (textarea with preview).
- [CodeMirror packages must resolve to one copy of `@codemirror/state`, otherwise the editor throws at start] → pnpm lockfile, all `@codemirror/*` bumped together (Renovate group); the bundle test catches an error at load.
- [Editor and hidden textarea out of sync at submit] → the textarea is updated on every change (not only on submit), so htmx serializes the current text; e2e test saves after editing.
- [Highlighting colors of the editor differ slightly from the preview (Lezer tags vs. syntect scopes)] → accepted; the preview is the reference.
- [Preview route as a CPU sink] → login required, 256 KiB limit, syntect's 64 KiB per-fence cap; same cost as opening a large artifact.
- [Screen reader users get a weaker field] → "Highlighting" switch (D8), Tab never trapped, label and errors wired to the editor.
- [Firefox without `field-sizing` (step 1)] → script fallback for the height (D2); step 2 grows on all browsers.
- [Live preview flickers or reorders responses] → debounce, one request in flight, stale responses dropped, morph swap; Mermaid redraws only changed fences (D2a).
- [`persona-identity` and `persona-commit-signing` landed with the modal form] → this change moves their groups to the page (General and Commits tabs) and their e2e tests with them; the PenPot sprint (group 1) draws them on the page boards.
- [Breaking the persona edit flow (modal → page)] → pre-MVP; the list and the e2e tests move in the same change.
- [Personas without a system prompt run with the harness defaults] → the card shows "No system prompt yet"; the create flow opens the System prompt tab right away.
- [A background preview request after the session expired] → `redirect: 'manual'` and the 401 message; the page never navigates away on its own (D3).

## Migration Plan

No data migration. The change deploys as a normal release. If the editor bundle causes trouble, removing it from the build is enough: the component falls back to the textarea with preview without code changes.

## Open Questions

None left. All questions were decided in the review of PR #303.

### Decided (Peter, PR #303, 2026-11-22)

- **Q1.** CodeMirror is in this change, in the same pull request (D1).
- **Q2.** `esbuild` is accepted as a pinned asset tool, with one bundled script (D5).
- **Q3.** Side by side from 1280 px, one panel below; the PenPot sprint tries it on the boards (D2a; the switch is Q16).
- **Q4.** No CodeMirror on phones (below 768 px); they keep the plain field with preview. Tablets from 768 px get the editor, with or without a keyboard (D8).
- **Q5.** Fence languages: Rust, TypeScript/JavaScript, JSON, YAML, TOML, shell, Python, SQL, Markdown, diff (D6).
- **Q7.** Tab moves focus out of the editor; indent with `Ctrl+]` / `Ctrl+[`; a hint below the editor says so (D7).
- **Q8.** A switch to turn the highlighting editor off is offered (D8; named "Highlighting" by Q16).
- **Q9.** The preview uses `render`: a single newline is a soft break (D3).
- **Q10.** Skills use the same shape: a skill settings page with a Body tab that holds the editor with `Preview::Split`. `iter4-skills` takes this over; this change does not edit that proposal.
- **Q11.** No scroll sync between editor and preview, not in this change and not as a follow-up.
- **Q12.** No system prompt in the create modal; saving opens the System prompt tab (D2b).
- **Q13.** The live preview waits 500 ms after the last change (D2a).
- **Q6.** The Memory tab gets source highlighting without preview (`Preview::SourceOnly`, D7a). `iter6-agent-memory` adds the variant, which has its caller there, and rewords its "never rendered as Markdown" rule to allow highlighting (moved there after Grace's review, DF-18: no caller in this change).
- **Q15.** The system prompt is optional, so the create modal can leave it out (D2b; raised by Grace's review, DF-18).
- **Q14.** Replacing `swc` with `esbuild` for the component scripts is a follow-up change (backlog row "Follow-ups of `markdown-editor`"); no bundling of the component scripts, the stylesheet or the vendor scripts (D5a).

### Decided (Peter, PenPot review of the design sprint, 2026-11-26)

- **Q16.** The Markdown field gets a view switch centred above it instead of "Write"/"Preview" tabs beside the label: Editor | Split | Preview from 1280 px (opens on Split) where the field has a page of its own, Editor | Preview below 1280 px and in narrow places (opens on Editor); icon and name per segment; not remembered (D2, D2a). The label is hidden visually where the page tab already names the field. The plain-text control becomes the switch "Highlighting" at the right end of the switch row (D8).
