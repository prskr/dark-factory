// Alpine component of a Markdown field (`x-data='markdownField({"split": true})'`, see
// `markdown_field` in `components/forms.rs`).
//
// Without this script the field is a plain labelled textarea: the view switch is `x-cloak`.
//
// Plain tabs layout (`markdownField({"plain": true})`, `Preview::PlainTabs`): "Write" and "Preview"
// are WAI-ARIA tabs (same keys as the switch), the view is `editor` or `preview`, and the
// highlighting editor is never initialised, so its bundle is never requested.
//
// State: `view` (`editor`, `split` or `preview`; not stored), `status` of the preview (`ok`,
// `failed`, `too-long`, `session`) and `rendered`, the text of the last good preview. The server
// renders the first preview of the split layout, so its text counts as rendered.
//
// Views: the field opens on "Split" from 1280 px with `split: true`, else on "Editor". A window
// that gets narrower than 1280 px while "Split" is shown falls back to "Editor". The switch is a
// WAI-ARIA radio group with its own `move(event)` (arrow keys, Home and End select and focus); it
// is not a `tabs` component, so the tabs of a settings page around the field are not affected.
//
// Preview: a `fetch` that sends only the field's text and `HX-Request` (the CSRF header), never
// follows a redirect and is aborted by a newer request. It runs when "Preview" opens and the text
// changed, and in "Split" 500 ms after the last change. The preview is swapped only on 200 (htmx
// morph: unchanged blocks keep their state), an error never replaces the last good preview.
//
// Mermaid: a fence whose source did not change keeps its drawn diagram. The morph skips an
// element with `hx-morph-skip`, so `keepDiagrams()` gives the new fence an empty slot after it
// that matches the old diagram element, which `markdown.ts` marks that way.
//
// Height: browsers without `field-sizing` (Firefox) get the growth from `fit()`.
//
// Highlighting editor (`editor/markdown-editor.ts`, bundled as `/assets/editor.js`): the URL is
// `data-editor-src` of `<body>`, which only a build with the bundle has. The field loads it once
// per page (like Mermaid) and attaches it to the textarea from 768 px on every device, a touch-only
// tablet included: the kind of input device decides nothing. A phone, a failed load, a build without the
// bundle and the switch "Highlighting" turned off (`localStorage['df.editor.plain']`) leave the
// textarea as the field. `canEdit` is whether an editor can attach (the switch shows then),
// `highlighting` the switch, `editing` whether an editor is attached (the indent hint shows then).
// The editor writes its text into the textarea and fires `input` on it, so the live preview
// follows both the same way.
(() => {
  type View = 'editor' | 'split' | 'preview';
  type Status = 'ok' | 'failed' | 'too-long' | 'session';

  const PREVIEW_URL = '/markdown/preview';
  const WIDE = '(min-width: 1280px)';
  const DESKTOP = '(min-width: 768px)';
  const DEBOUNCE_MS = 500;
  const MAX_HEIGHT = 0.7;
  const DIAGRAM = 'pre[x-data="mermaidDiagram"]';
  const PLAIN_KEY = 'df.editor.plain';

  let editorLoading: Promise<void> | null = null;

  // Loads the editor bundle on first use; a failed load can be retried by the next field.
  function loadEditor(src: string): Promise<void> {
    if (window.DfMarkdownEditor) return Promise.resolve();
    editorLoading ??= new Promise<void>((resolve, reject) => {
      const script = document.createElement('script');
      script.src = src;
      script.onload = () => resolve();
      script.onerror = () => {
        editorLoading = null;
        script.remove();
        reject(new Error('The editor bundle did not load'));
      };
      document.head.append(script);
    });
    return editorLoading;
  }

  // `localStorage` can throw (private windows, blocked site data): the choice then lasts as long
  // as the page.
  function storedPlain(): boolean {
    try {
      return localStorage.getItem(PLAIN_KEY) !== null;
    } catch {
      return false;
    }
  }

  function storePlain(plain: boolean): void {
    try {
      if (plain) localStorage.setItem(PLAIN_KEY, '1');
      else localStorage.removeItem(PLAIN_KEY);
    } catch {
      // The switch still works for this page.
    }
  }

  document.addEventListener('alpine:init', () => {
    Alpine.data('markdownField', (options: { split?: boolean; plain?: boolean }) => ({
      // The view switch row shows once the component runs (`x-show="enhanced"`).
      enhanced: true,
      view: 'editor' as View,
      status: 'ok' as Status,
      rendered: null as string | null,
      root: null as HTMLElement | null,
      textarea: null as HTMLTextAreaElement | null,
      timer: 0,
      pending: null as AbortController | null,
      observer: null as IntersectionObserver | null,
      wide: null as MediaQueryList | null,
      onWidthChange: null as (() => void) | null,
      desktop: null as MediaQueryList | null,
      onDesktopChange: null as (() => void) | null,
      canEdit: false,
      highlighting: true,
      editing: false,
      editor: null as { destroy(): void } | null,
      indentHint: 'Tab moves to the next field. Indent with Ctrl+] and Ctrl+[.',

      init(): void {
        // `$el` is the element an event handler runs on once Alpine evaluates one: keep the root.
        this.root = this.$el;
        this.textarea = this.root.querySelector('textarea');
        if (!this.textarea) return;
        // The plain tabs layout has no highlighting editor and never loads its bundle.
        if (!options.plain) this.initEditor(this.textarea);
        this.wide = window.matchMedia(WIDE);
        this.view = options.split && this.wide.matches ? 'split' : 'editor';
        // A moved field (a card that is sent again keeps its form) opens on the tab it had.
        if (options.plain && this.root.dataset.view === 'preview') {
          this.view = 'preview';
          void this.refresh();
        }
        if (options.split) this.rendered = this.textarea.defaultValue;
        this.onWidthChange = () => {
          if (this.view === 'split' && !this.wide?.matches) this.select('editor');
        };
        this.wide.addEventListener('change', this.onWidthChange);
        this.textarea.addEventListener('input', () => this.changed());
        // A hidden panel (another tab of the page) measures 0: fit when the field shows up.
        this.observer = new IntersectionObserver(() => this.fit());
        this.observer.observe(this.textarea);
        this.fit();
      },

      destroy(): void {
        window.clearTimeout(this.timer);
        this.pending?.abort();
        this.observer?.disconnect();
        if (this.onWidthChange) this.wide?.removeEventListener('change', this.onWidthChange);
        if (this.onDesktopChange) this.desktop?.removeEventListener('change', this.onDesktopChange);
        this.detach();
      },

      // Whether an editor can attach follows the window width; the switch and the stored choice
      // decide whether it does.
      initEditor(textarea: HTMLTextAreaElement): void {
        const src = document.body.dataset.editorSrc;
        this.desktop = window.matchMedia(DESKTOP);
        this.highlighting = !storedPlain();
        this.canEdit = Boolean(src) && this.desktop.matches;
        if (/Mac|iPhone|iPad/.test(navigator.platform)) this.indentHint = this.indentHint.replaceAll('Ctrl', 'Cmd');
        this.onDesktopChange = () => {
          this.canEdit = Boolean(src) && (this.desktop?.matches ?? false);
          this.sync(textarea, src);
        };
        this.desktop.addEventListener('change', this.onDesktopChange);
        this.sync(textarea, src);
      },

      // Makes the field match the state: the editor attached while it can and is switched on.
      sync(textarea: HTMLTextAreaElement, src: string | undefined): void {
        if (this.canEdit && this.highlighting) void this.attach(textarea, src);
        else this.detach();
      },

      async attach(textarea: HTMLTextAreaElement, src: string | undefined): Promise<void> {
        if (this.editor || !src) return;
        try {
          await loadEditor(src);
        } catch {
          // The textarea stays the field and the switch goes away.
          this.canEdit = false;
          return;
        }
        // The state may have changed while the bundle loaded.
        if (this.editor || !this.canEdit || !this.highlighting) return;
        const hint = this.root?.querySelector<HTMLElement>('[data-indent-hint]');
        this.editor = window.DfMarkdownEditor.attach(textarea, { describedBy: hint ? [hint.id] : [] });
        this.editing = true;
      },

      detach(): void {
        if (!this.editor) return;
        this.editor.destroy();
        this.editor = null;
        this.editing = false;
      },

      // The switch "Highlighting": remembered in this browser.
      toggle(): void {
        this.highlighting = !this.highlighting;
        storePlain(!this.highlighting);
        if (this.textarea) this.sync(this.textarea, document.body.dataset.editorSrc);
      },

      select(view: View): void {
        if (this.view === view) return;
        this.view = view;
        if (this.root) this.root.dataset.view = view;
        window.clearTimeout(this.timer);
        if (view !== 'preview') void this.$nextTick(() => this.fit());
        if (view !== 'editor') void this.refresh();
      },

      // Arrow keys, Home and End on the switch: the focused segment's neighbour is selected.
      move(event: KeyboardEvent): void {
        const group = event.currentTarget as HTMLElement;
        const radios = [...group.querySelectorAll<HTMLElement>('[role="radio"], [role="tab"]')].filter((r) => r.offsetParent !== null);
        const at = radios.indexOf(document.activeElement as HTMLElement);
        if (at < 0) return;
        const steps: Record<string, number | undefined> = { ArrowUp: -1, ArrowLeft: -1, ArrowDown: 1, ArrowRight: 1 };
        const step = steps[event.key];
        let next: number;
        if (step !== undefined) next = (at + step + radios.length) % radios.length;
        else if (event.key === 'Home') next = 0;
        else if (event.key === 'End') next = radios.length - 1;
        else return;
        event.preventDefault();
        this.select((radios[next].dataset.view ?? 'editor') as View);
        radios[next].focus();
      },

      // The text changed: the live preview of "Split" follows after a pause; other views do not
      // request anything while typing.
      changed(): void {
        this.fit();
        if (this.view !== 'split') return;
        window.clearTimeout(this.timer);
        this.timer = window.setTimeout(() => void this.refresh(), DEBOUNCE_MS);
      },

      retry(): void {
        void this.refresh(true);
      },

      text(): string {
        return this.textarea?.value ?? '';
      },

      // Requests the preview unless the one shown is of the same text. One request at a time: a
      // newer one aborts the older, so a response of an older text never replaces a newer one.
      async refresh(force = false): Promise<void> {
        const text = this.text();
        if (!force && this.status === 'ok' && text === this.rendered) return;
        this.pending?.abort();
        const controller = new AbortController();
        this.pending = controller;
        let status: Status = 'failed';
        let html: string | null = null;
        try {
          const response = await fetch(PREVIEW_URL, {
            method: 'POST',
            body: new URLSearchParams({ source: text }),
            headers: { 'HX-Request': 'true' },
            redirect: 'manual',
            signal: controller.signal,
          });
          if (response.status === 200) {
            status = 'ok';
            html = await response.text();
          } else if (response.status === 413) status = 'too-long';
          else if (response.status === 401) status = 'session';
        } catch {
          if (controller.signal.aborted) return;
        }
        if (this.pending !== controller) return;
        this.pending = null;
        if (html !== null) {
          await this.swap(html);
          this.rendered = text;
        }
        this.status = status;
      },

      async swap(html: string): Promise<void> {
        const target = this.root?.querySelector<HTMLElement>('[data-preview]');
        if (!target) return;
        await htmx.swap({ target, text: this.keepDiagrams(target, html), swap: 'innerMorph', sourceElement: target });
      },

      // Puts an empty `div.diagram` after each new fence whose source equals one that is drawn,
      // so the morph matches it with the old diagram element and leaves it alone.
      keepDiagrams(target: HTMLElement, html: string): string {
        const template = document.createElement('template');
        template.innerHTML = html;
        const drawn = [...target.querySelectorAll(DIAGRAM)].filter((pre) => pre.nextElementSibling?.matches('.diagram'));
        for (const pre of template.content.querySelectorAll(DIAGRAM)) {
          const at = drawn.findIndex((old) => old.textContent === pre.textContent);
          if (at < 0) continue;
          drawn.splice(at, 1);
          const slot = document.createElement('div');
          slot.className = 'diagram';
          slot.setAttribute('hx-morph-skip', '');
          pre.after(slot);
        }
        return template.innerHTML;
      },

      // Sizes the textarea to its text where `field-sizing` is not supported: up to 70 % of the
      // viewport, in Split to the whole text (the column caps the shared height and scrolls). The
      // measure is taken without the minimum height, so a shorter text shrinks the field again.
      fit(): void {
        const el = this.textarea;
        if (!el || CSS.supports('field-sizing', 'content') || el.offsetParent === null) return;
        el.style.minHeight = '0';
        el.style.height = '0';
        const content = el.scrollHeight + (el.offsetHeight - el.clientHeight);
        el.style.minHeight = '';
        el.style.height = `${this.view === 'split' ? content : Math.min(content, window.innerHeight * MAX_HEIGHT)}px`;
      },
    }));
  });
})();
