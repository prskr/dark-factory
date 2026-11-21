# Tasks

Step 1 (groups 1–3) and step 2 (groups 4–6) are separate pull requests (design D1). Step 2 starts only after the reviewers accept open questions Q1 and Q2 of `design.md`; if they do not, step 2 moves to a follow-up change. Container-backed tests (`df_test_postgres`) need a socket; state which ones could not run.

## 1. Preview endpoint

- [ ] 1.1 Write the failing tests first in a new `crates/df-web/src/markdown_preview.rs` (`#[cfg(test)] mod tests`, `TestApp`): a logged-in htmx `POST /markdown/preview` with `source` returns the output of `markdown::render` wrapped in `<div class="markdown">`; `<script>` stays escaped; links carry `rel="noopener noreferrer nofollow"`; a `mermaid` fence comes back as `x-data="mermaidDiagram"`; empty `source` gives "Nothing to preview"; a request without the htmx header gets 403; without a session no rendered HTML; a decoded `source` over 256 KiB gets 413; 200 KiB of `#` (about 600 KiB form-encoded) is rendered; a body over 1 MiB gets 413. Verify: `cargo nextest run -p df-web markdown_preview` fails.
- [ ] 1.2 Implement the route in the board-member router (`lib.rs`), with `DefaultBodyLimit::max(1 MiB)` and the 256 KiB check on the decoded `source` (design D3), no logging of the text. Verify: the tests of 1.1 pass.

## 2. Component `markdown_field`

- [ ] 2.1 Write the failing HTML tests in `components/forms.rs`: `markdown_field` renders label, monospace textarea (`font-mono`, 16 px below `lg`), `name`/`id`/value, `aria-describedby` and `aria-invalid` like `textarea_field`, a tab row with "Write" and "Preview" (`role=tab`, `aria-controls`, `x-cloak`), a preview panel with `hidden` and `aria-live="polite"`; with `preview: false` no tab row and no preview panel. Verify: `cargo nextest run -p df-web forms` fails.
- [ ] 2.2 Implement `MarkdownField` and `markdown_field` (design D2), the auto-grow CSS (`field-sizing: content`, `min-height` from `rows`, `max-height: 70vh`) in `styles/input.css`. Verify: tests of 2.1 pass.
- [ ] 2.3 Add the Alpine component `scripts/markdownField.ts` (tabs with the keyboard pattern of `tabs`, preview request with `htmx.ajax` when "Preview" opens and the text changed, "Preview not available" with retry, 413 message, height fallback when `field-sizing` is not supported (design D2)), register it in `COMPONENT_SCRIPTS`, extend `scripts/types/*.d.ts` for `htmx.ajax`. Add `script_source("markdownField.ts")` tests for "no request while the text is unchanged" and the error texts. Verify: `cargo nextest run -p df-web` and `npx --yes -p typescript@<version> tsc -p crates/df-web`.

## 3. Persona form and docs (step 1)

- [ ] 3.1 Use `markdown_field` for `system_prompt` in `config_pages/personas.rs` (`rows: 8`, `preview: true`), test first: the persona form contains the tabs and `name="system_prompt"`; a 422 re-render keeps the text. Verify: `cargo nextest run -p df-web personas` (container-backed).
- [ ] 3.2 Add an e2e test in `crates/df-e2e` (skill `e2e`): open the persona form, type a heading and a `rust` fence, open "Preview", see a heading and `hl-` spans; save and reopen: the text is unchanged. Verify: `just e2e-local` (or state that no container socket was available).
- [ ] 3.3 Docs: `docs/content/using/setup.md` (persona form: System prompt is a Markdown field with Write and Preview), `docs/design-system.md` (Forms: `markdown_field`, when to use `preview: false`), `crates/df-web/AGENTS.md` if the component helper for tabs changed. Verify: `just docs-build`.
- [ ] 3.4 Step 1 checks: `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length`, light and dark and 390 px check of the persona modal against the review checklist in `docs/design-system.md`. Verify: clippy clean; checklist items noted in the PR.

## 4. Editor bundle build (step 2)

- [ ] 4.1 Pin `esbuild` in `multitool.lock.json` (npm tarballs `@esbuild/linux-x64`, `linux-arm64`, `darwin-arm64` with sha256), add it to `bazel_env` in `BUILD.bazel` and to `cargo xtask update-tools` if that tool manages multitool entries. Verify: `bazel run --bazelrc=.bazelrc.agents //:bazel_env` puts `esbuild` on `.bazel_env/bin`; `esbuild --version` prints the pinned version.
- [ ] 4.2 Add the CodeMirror packages to `crates/df-web/package.json` (`@codemirror/view`, `state`, `commands`, `language`, `lang-markdown`, the fence languages of Q5, `@lezer/highlight`), update `pnpm-lock.yaml` with `pnpm install`, add a Renovate group so all `@codemirror/*` and `@lezer/*` move together. Verify: `pnpm install --frozen-lockfile` in `crates/df-web` succeeds without install scripts.
- [ ] 4.3 Write `crates/df-web/editor/markdown-editor.ts` (ES module, exposes `window.DfMarkdownEditor.attach(textarea, options)`, design D7) and `editor/tsconfig.json`; add `tsc -p crates/df-web/editor` to the `web-typecheck` hook in `lefthook.yml` and to CI. Verify: the type check passes and fails on a deliberate type error.
- [ ] 4.4 Extend `build_assets.rs` test first (its `#[path]` tests): with `EDITOR_BUNDLE` set the file is staged as `editor.js` and cfg `df_web_editor` is reported; without it, `esbuild` from `ESBUILD_BIN`/`PATH` runs when the CodeMirror packages are in `node_modules`; a missing tool or package gives a warning naming it and no cfg; with `DF_WEB_REQUIRE_ASSETS=1` it is an error; `rerun_inputs` covers the entry and the packages. Then implement, and register `df_web_editor` in `check-cfg`. Verify: `cargo nextest run -p df-web build_assets`.
- [ ] 4.5 Add the Bazel genrule `:editor_bundle` in `crates/df-web/BUILD.bazel` (esbuild, entry, linked CodeMirror packages, `--bundle --minify --format=iife --target=es2022`) and pass it as `EDITOR_BUNDLE` to the build script. Verify: `bazel --bazelrc=.bazelrc.agents build //crates/df-web/...`.
- [ ] 4.6 Tests of the compiled bundle with `#[cfg_attr(not(df_web_editor), ignore = "…")]`: `/assets/editor.js` is served with a content hash, defines `DfMarkdownEditor`, and is below 300 KiB gzip (the test names the size). Verify: `bazel --bazelrc=.bazelrc.agents test //crates/df-web/...`.

## 5. Editor in the component (step 2)

- [ ] 5.1 `layout.rs`: `<body data-editor-src>` only when the bundle is embedded; test first, like `body_names_the_diagram_renderer_for_the_mermaid_component` and its counterpart without the asset. Verify: `cargo nextest run -p df-web layout`.
- [ ] 5.2 `markdownField.ts`: load the bundle once per page from `data-editor-src`; attach only with a fine pointer, a viewport of at least 768 px, and without `localStorage['df.editor.plain']`; keep the textarea on load failure; "Plain text editor" switch (design D8). `script_source` tests for each condition. Verify: `cargo nextest run -p df-web` and the type check.
- [ ] 5.3 Editor theme and highlighting (design D7): `EditorView.theme` with CSS variables only, Markdown tags from tokens, fence code with syntect `hl-` classes; labelling (`aria-labelledby`, `aria-describedby`, `aria-invalid`) copied to the content element; Tab not bound; Escape not bound; textarea updated on every change. Verify: check in light and dark, with each default code theme, and with a screen reader (VoiceOver or NVDA) that the field is announced by its label; note results in the PR.
- [ ] 5.4 E2E with the bundle: in the persona form a `rust` fence shows `hl-` spans in the editor, Tab moves focus to the Harness select, saving stores the edited text; at 390 px the editor bundle is not requested. Verify: `just e2e-local` (or state that no container socket was available).

## 6. Docs and checks (step 2)

- [ ] 6.1 Docs: `docs/development.md` (asset tools: `esbuild`, `ESBUILD_BIN`, the editor bundle on the Bazel and plain `cargo` paths, cfg `df_web_editor`), `crates/df-web/AGENTS.md` (the bundled exception, `editor/`, its type check), `docs/content/using/setup.md` (highlighting, Tab behavior, "Plain text editor", phones), `docs/design-system.md` (editor theme rules). Verify: `just docs-build`.
- [ ] 6.2 Final checks: `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo deny check`, `openspec validate markdown-editor --strict`, a plain `cargo build -p df-web` without `esbuild` on `PATH` succeeds with the warning. Verify: all pass.

## Workflow follow-up

- Archive: add the backlog row "Markdown editor follow-ups" if it is not in `product/iterations/roadmap.md` yet; tell the owners of `iter4-skills` (skill body uses `markdown_field`) and `iter6-agent-memory` (decision Q6) about the component.
