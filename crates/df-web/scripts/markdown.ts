// Alpine component of a Mermaid fence in rendered Markdown (`x-data="mermaidDiagram"` on the
// `<pre>` that `markdown::render` emits).
//
// Without JavaScript, or when the diagram cannot be drawn, the `<pre>` stays a plain code block.
// With it, the component loads Mermaid once per page (the URL is `data-mermaid-src` of `<body>`),
// draws the source into a sibling `<div>` and hides the `<pre>`. A build without Mermaid has no
// `data-mermaid-src`, and a failed load is the same: the source stays and "Diagram renderer not
// available" follows it.
//
// Isolation: `securityLevel: 'sandbox'` makes Mermaid return an iframe without script access to
// the page. `securityLevel` is also in `secure`, so `%%{init}%%` in the (untrusted) source cannot
// lower it. The source only ever reaches the page as text (`textContent`); `innerHTML` takes the
// render result.
//
// Theme: `dark` or `default` from `prefers-color-scheme`; a change redraws the diagram.
(() => {
  /** What the diagram currently shows: the drawn SVG or the error message. */
  type View = { svg: string; message?: undefined } | { svg?: undefined; message: string };
  /** The sibling elements added after the `<pre>`. */
  type Slot = 'diagram' | 'error';

  /** Mermaid is missing from this build or did not load. */
  class RendererUnavailable extends Error {
    constructor() {
      super('Diagram renderer not available');
    }
  }

  const SECURE = ['secure', 'securityLevel', 'startOnLoad', 'suppressErrorRendering', 'maxTextSize', 'maxEdges'];
  const scheme = window.matchMedia('(prefers-color-scheme: dark)');
  let loading: Promise<MermaidApi> | null = null;
  let counter = 0;

  // Loads Mermaid on first use; a failed load can be retried by the next diagram.
  function loadMermaid() {
    loading ??= new Promise<MermaidApi>((resolve, reject) => {
      const src = document.body.dataset.mermaidSrc;
      if (!src) {
        loading = null;
        reject(new RendererUnavailable());
        return;
      }
      const script = document.createElement('script');
      script.src = src;
      script.onload = () => resolve(window.mermaid);
      script.onerror = () => {
        loading = null;
        script.remove();
        reject(new RendererUnavailable());
      };
      document.head.append(script);
    });
    return loading;
  }

  // The `viewBox` of the SVG inside Mermaid's sandbox iframe (a data URL, or `srcdoc`).
  function viewBoxOf(frame: Element): { width: number; height: number } | null {
    const src = frame.getAttribute('srcdoc') ?? atob((frame.getAttribute('src') ?? '').split(',')[1] ?? '');
    const box = /viewBox="[\d.-]+ [\d.-]+ ([\d.]+) ([\d.]+)"/.exec(src);
    return box && { width: Number(box[1]), height: Number(box[2]) };
  }

  document.addEventListener('alpine:init', () => {
    Alpine.data('mermaidDiagram', () => ({
      source: '',
      drawing: 0,
      // What the diagram currently shows: `{ svg }` or `{ message }` (the error); null until drawn.
      view: null as View | null,
      diagram: null as HTMLElement | null,
      error: null as HTMLElement | null,
      observer: null as MutationObserver | null,
      resizer: null as ResizeObserver | null,
      onSchemeChange: null as (() => void) | null,

      init(): void {
        this.source = this.$el.textContent ?? '';
        this.onSchemeChange = () => void this.draw();
        scheme.addEventListener('change', this.onSchemeChange);
        // An htmx morph of the surrounding element (a request card) keeps this `<pre>` but resets
        // its attributes and drops the siblings added below. Whatever it changes, `apply()` puts
        // the current view back.
        this.observer = new MutationObserver(() => this.apply());
        this.observer.observe(this.$el, { attributes: true, attributeFilter: ['hidden'] });
        if (this.$el.parentNode) this.observer.observe(this.$el.parentNode, { childList: true });
        this.resizer = new ResizeObserver(() => this.fit());
        void this.draw();
      },

      destroy(): void {
        if (this.onSchemeChange) scheme.removeEventListener('change', this.onSchemeChange);
        this.observer?.disconnect();
        this.resizer?.disconnect();
        this.discard('diagram');
        this.discard('error');
      },

      // Draws the source in the current theme. A newer call makes an older one that is still
      // waiting for Mermaid give up.
      async draw(): Promise<void> {
        const run = ++this.drawing;
        try {
          const mermaid = await loadMermaid();
          mermaid.initialize({
            startOnLoad: false,
            securityLevel: 'sandbox',
            secure: SECURE,
            suppressErrorRendering: true,
            theme: scheme.matches ? 'dark' : 'default',
          });
          const { svg } = await mermaid.render(`mermaid-${++counter}`, this.source);
          if (run === this.drawing) this.show({ svg });
        } catch (err) {
          const message = String((err as Error | undefined)?.message ?? err).split('\n')[0];
          const text = err instanceof RendererUnavailable ? message : `Diagram error: ${message}`;
          if (run === this.drawing) this.show({ message: text });
        }
      },

      show(view: View): void {
        this.view = view;
        // A new drawing replaces the old one; apply() creates what is missing.
        this.discard('diagram');
        this.discard('error');
        this.apply();
      },

      // Makes the page match `view`: the diagram in a sibling `<div>` and the source hidden, or
      // the source with a one-line error below it. It only touches what differs, because its own
      // changes trigger the observer again.
      apply(): void {
        if (!this.view) return;
        const { svg, message } = this.view;
        if (this.$el.hidden !== (svg !== undefined)) this.$el.hidden = svg !== undefined;
        if (svg !== undefined) {
          const diagram = this.sibling('diagram', 'div', 'diagram');
          if (diagram) {
            diagram.innerHTML = svg;
            this.resizer?.observe(diagram);
          }
        } else {
          const error = this.sibling('error', 'p', 'diagram-error');
          if (error) error.textContent = message;
        }
      },

      // Mermaid sizes the iframe for the SVG at its natural width, but the SVG shrinks to the
      // iframe's width: the height follows the scale, so no empty space is left below.
      fit(): void {
        const frame = this.diagram?.querySelector('iframe');
        const box = frame && viewBoxOf(frame);
        if (!box || !frame.clientWidth) return;
        frame.style.height = `${Math.ceil(box.height * Math.min(1, frame.clientWidth / box.width))}px`;
      },

      // Removes the sibling `this[name]`; the resizer only ever observes the current diagram.
      discard(name: Slot): void {
        const element = this[name];
        if (!element) return;
        this.resizer?.unobserve(element);
        element.remove();
        this[name] = null;
      },

      // Ensures the element `this[name]` follows the `<pre>`; returns it if it had to be created,
      // otherwise null.
      sibling(name: Slot, tag: 'div' | 'p', className: string): HTMLElement | null {
        if (this[name]?.isConnected) return null;
        this.discard(name); // one an htmx morph removed
        const element = document.createElement(tag);
        element.className = className;
        this.$el.after(element);
        this[name] = element;
        return element;
      },
    }));
  });
})();
