// The highlighting Markdown editor (CodeMirror 6), bundled by `esbuild` into `/assets/editor.js`
// (`build_assets.rs`, `:editor_bundle` in `BUILD.bazel`; design D5 and D7 of `markdown-editor`).
//
// This is the one ES module of the web UI; the component scripts in `scripts/` stay classic
// scripts. The bundle is an IIFE that exposes `window.DfMarkdownEditor.attach(textarea, options)`.
// The Alpine component `markdownField` (`scripts/markdownField.ts`) loads it on demand.
//
// `attach` creates the editor after the textarea and hides the textarea, which stays in the form:
// the editor writes its text into `textarea.value` on every change (and fires `input` on the
// textarea, so listeners of the field see the editor like the textarea) and copies the labelling
// to its content element. Tab is not bound, so Tab and Shift+Tab move focus out of the editor;
// indenting is `Mod-]` and `Mod-[`. Escape is not bound either (a dialog around the field closes).
// No autocomplete, line numbers, search panel or bracket matching.
//
// Colors come from the design tokens (CSS variables of `styles/input.css`), so light and dark
// follow `prefers-color-scheme`. Code inside fences gets the class names of the syntect themes
// (`hl-keyword`, ...), so the configured code theme (`/assets/code-theme.css`) colors it like the
// preview. The mapping from Lezer tags is approximate; a tag without a match stays `ink`.
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { javascript } from '@codemirror/lang-javascript';
import { json } from '@codemirror/lang-json';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { python } from '@codemirror/lang-python';
import { rust } from '@codemirror/lang-rust';
import { sql } from '@codemirror/lang-sql';
import { yaml } from '@codemirror/lang-yaml';
import { HighlightStyle, LanguageDescription, LanguageSupport, StreamLanguage, syntaxHighlighting } from '@codemirror/language';
import { diff } from '@codemirror/legacy-modes/mode/diff';
import { shell } from '@codemirror/legacy-modes/mode/shell';
import { toml } from '@codemirror/legacy-modes/mode/toml';
import { EditorState } from '@codemirror/state';
import { drawSelection, EditorView, keymap } from '@codemirror/view';
import { styleTags, Tag, tags } from '@lezer/highlight';

/** The tag of inline code (`` `code` ``), apart from the body of a fence. */
const inlineCode = Tag.define();

export interface AttachOptions {
  /** Ids of elements that describe the field besides the ones of the textarea (the indent hint). */
  describedBy?: string[];
}

export interface Attached {
  /** Removes the editor and shows the textarea again. */
  destroy(): void;
  focus(): void;
}

declare global {
  interface Window {
    DfMarkdownEditor: { attach(textarea: HTMLTextAreaElement, options?: AttachOptions): Attached };
  }
}

const legacy = (parser: Parameters<typeof StreamLanguage.define>[0]): LanguageSupport =>
  new LanguageSupport(StreamLanguage.define(parser));

/** The languages of fenced code: Rust, TypeScript and JavaScript, JSON, YAML, TOML, shell, Python,
 * SQL, Markdown and diff. Unknown ones stay plain code. */
const FENCE_LANGUAGES = [
  LanguageDescription.of({ name: 'Rust', alias: ['rs'], support: rust() }),
  LanguageDescription.of({ name: 'TypeScript', alias: ['ts', 'tsx'], support: javascript({ typescript: true, jsx: true }) }),
  LanguageDescription.of({ name: 'JavaScript', alias: ['js', 'jsx', 'mjs'], support: javascript({ jsx: true }) }),
  LanguageDescription.of({ name: 'JSON', alias: ['jsonc'], support: json() }),
  LanguageDescription.of({ name: 'YAML', alias: ['yml'], support: yaml() }),
  LanguageDescription.of({ name: 'TOML', support: legacy(toml) }),
  LanguageDescription.of({ name: 'Shell', alias: ['sh', 'bash', 'zsh', 'console'], support: legacy(shell) }),
  LanguageDescription.of({ name: 'Python', alias: ['py'], support: python() }),
  LanguageDescription.of({ name: 'SQL', support: sql() }),
  LanguageDescription.of({ name: 'Markdown', alias: ['md'], support: markdown({ base: markdownLanguage }) }),
  LanguageDescription.of({ name: 'Diff', alias: ['patch'], support: legacy(diff) }),
];

/** Markdown structure: headings in the code theme, markers fainter, links in the accent color
 * and inline code on `sunk` from the tokens. */
const MARKDOWN_STYLE = HighlightStyle.define([
  // Headings in the function-name color of the configured code theme (the classes of syntect's
  // `entity.name.function`), semibold. Their `#` marker has its own, fainter style below.
  { tag: tags.heading, class: 'hl-entity hl-name hl-function cm-md-heading' },
  { tag: tags.strong, fontWeight: '600' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: [tags.processingInstruction, tags.contentSeparator, tags.url], class: 'cm-md-marker' },
  { tag: tags.link, color: 'var(--accent-text)' },
  { tag: tags.quote, color: 'var(--muted)' },
  // Only `InlineCode` has the tag of its own: `tags.monospace` also styles the body of a fence
  // whose language has no highlighter, which stays plain (the code theme's code, not a chip).
  { tag: inlineCode, backgroundColor: 'var(--sunk)', borderRadius: '3px' },
  { tag: tags.labelName, color: 'var(--faint)' },
]);

/** The syntect class names the code theme stylesheet colors. */
const CODE_STYLE = HighlightStyle.define([
  { tag: [tags.keyword, tags.controlKeyword, tags.operatorKeyword, tags.moduleKeyword, tags.modifier], class: 'hl-keyword' },
  { tag: [tags.definitionKeyword, tags.typeName, tags.className, tags.namespace], class: 'hl-storage hl-type' },
  { tag: [tags.string, tags.special(tags.string), tags.regexp], class: 'hl-string' },
  { tag: [tags.comment, tags.lineComment, tags.blockComment], class: 'hl-comment' },
  { tag: [tags.number, tags.integer, tags.float], class: 'hl-constant hl-numeric' },
  { tag: [tags.bool, tags.null, tags.atom], class: 'hl-constant hl-language' },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName), tags.definition(tags.function(tags.variableName))], class: 'hl-entity hl-name hl-function' },
  { tag: [tags.propertyName, tags.attributeName], class: 'hl-variable hl-other' },
  { tag: tags.tagName, class: 'hl-entity hl-name hl-tag' },
]);

const MONO = 'var(--font-mono, ui-monospace, monospace)';

/** Colors from the tokens; the size comes from the wrapper (`.md-editor` in `styles/input.css`). */
const THEME = EditorView.theme({
  '&': {
    backgroundColor: 'var(--panel)',
    color: 'var(--ink)',
    border: '1px solid var(--line-strong)',
    borderRadius: '0.5rem',
  },
  '&.cm-focused': { outline: '2px solid var(--accent-text)', outlineOffset: '2px' },
  '.cm-scroller': { fontFamily: MONO, lineHeight: 'inherit' },
  '.cm-content': { padding: '10px 12px', caretColor: 'var(--ink)' },
  '.cm-line': { padding: '0' },
  // A marker inside a heading also carries the heading's code theme classes, whose color and
  // (for the function-name scope) boldness are stronger than a plain class: `!important` keeps the
  // marker `faint` and the heading semibold.
  '.cm-md-marker': { color: 'var(--faint) !important' },
  '.cm-md-heading': { fontWeight: '600 !important' },
  '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--ink)' },
  '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground': {
    backgroundColor: 'color-mix(in srgb, var(--accent-text) 25%, transparent)',
  },
});

/** The keys of the editor: the default ones without Escape, which a dialog around the field needs. */
const KEYS = defaultKeymap.filter((binding) => binding.key !== 'Escape');

function attach(textarea: HTMLTextAreaElement, options: AttachOptions = {}): Attached {
  const label = document.querySelector<HTMLLabelElement>(`label[for="${textarea.id}"]`);
  const describedBy = [textarea.getAttribute('aria-describedby'), ...(options.describedBy ?? [])].filter(Boolean).join(' ');
  const attributes: Record<string, string> = {};
  if (label?.id) attributes['aria-labelledby'] = label.id;
  if (describedBy) attributes['aria-describedby'] = describedBy;
  if (textarea.getAttribute('aria-invalid')) attributes['aria-invalid'] = 'true';

  const host = document.createElement('div');
  host.className = 'md-editor min-w-0';
  const view = new EditorView({
    parent: host,
    state: EditorState.create({
      doc: textarea.value,
      extensions: [
        history(),
        keymap.of([...KEYS, ...historyKeymap]),
        drawSelection(),
        EditorView.lineWrapping,
        markdown({
          base: markdownLanguage,
          codeLanguages: FENCE_LANGUAGES,
          extensions: { props: [styleTags({ InlineCode: inlineCode })] },
        }),
        syntaxHighlighting(MARKDOWN_STYLE),
        syntaxHighlighting(CODE_STYLE),
        THEME,
        EditorView.contentAttributes.of(attributes),
        EditorView.updateListener.of((update) => {
          if (!update.docChanged) return;
          textarea.value = update.state.doc.toString();
          textarea.dispatchEvent(new Event('input', { bubbles: true }));
        }),
      ],
    }),
  });
  textarea.after(host);
  textarea.hidden = true;
  const focus = (): void => view.focus();
  label?.addEventListener('click', focus);

  return {
    focus,
    destroy(): void {
      label?.removeEventListener('click', focus);
      view.destroy();
      host.remove();
      textarea.hidden = false;
    },
  };
}

window.DfMarkdownEditor = { attach };
