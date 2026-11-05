// Alpine component of a Mermaid fence in rendered Markdown (`x-data="mermaidDiagram"` on the
// `<pre>` that `markdown::render` emits).
//
// Without JavaScript, or when the diagram cannot be drawn, the `<pre>` stays a plain code block.
// With it, the component loads Mermaid once per page (the URL is `data-mermaid-src` of `<body>`),
// draws the source into a sibling `<div>` and hides the `<pre>`.
//
// Isolation: `securityLevel: 'sandbox'` makes Mermaid return an iframe without script access to
// the page. `securityLevel` is also in `secure`, so `%%{init}%%` in the (untrusted) source cannot
// lower it. The source only ever reaches the page as text (`textContent`); `innerHTML` takes the
// render result.
//
// Theme: `dark` or `default` from `prefers-color-scheme`; a change redraws the diagram.
(() => {
  const SECURE = ['secure', 'securityLevel', 'startOnLoad', 'suppressErrorRendering', 'maxTextSize', 'maxEdges'];
  const scheme = window.matchMedia('(prefers-color-scheme: dark)');
  let loading = null;
  let counter = 0;

  // Loads Mermaid on first use; a failed load can be retried by the next diagram.
  function loadMermaid() {
    loading ??= new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = document.body.dataset.mermaidSrc;
      script.onload = () => resolve(window.mermaid);
      script.onerror = () => {
        loading = null;
        script.remove();
        reject(new Error('The diagram renderer could not be loaded'));
      };
      document.head.append(script);
    });
    return loading;
  }

  // The `viewBox` of the SVG inside Mermaid's sandbox iframe (a data URL, or `srcdoc`).
  function viewBoxOf(frame) {
    const src = frame.getAttribute('srcdoc') ?? atob((frame.getAttribute('src') ?? '').split(',')[1] ?? '');
    const box = /viewBox="[\d.-]+ [\d.-]+ ([\d.]+) ([\d.]+)"/.exec(src);
    return box && { width: Number(box[1]), height: Number(box[2]) };
  }

  document.addEventListener('alpine:init', () => {
    Alpine.data('mermaidDiagram', () => ({
      source: '',
      drawing: 0,
      // What the diagram currently shows: `{ svg }` or `{ message }` (the error); null until drawn.
      view: null,
      diagram: null,
      error: null,
      observer: null,
      resizer: null,
      onSchemeChange: null,

      init() {
        this.source = this.$el.textContent;
        this.onSchemeChange = () => this.draw();
        scheme.addEventListener('change', this.onSchemeChange);
        // An htmx morph of the surrounding element (a request card) keeps this `<pre>` but resets
        // its attributes and drops the siblings added below. Whatever it changes, `apply()` puts
        // the current view back.
        this.observer = new MutationObserver(() => this.apply());
        this.observer.observe(this.$el, { attributes: true, attributeFilter: ['hidden'] });
        this.observer.observe(this.$el.parentNode, { childList: true });
        this.resizer = new ResizeObserver(() => this.fit());
        this.draw();
      },

      destroy() {
        scheme.removeEventListener('change', this.onSchemeChange);
        this.observer.disconnect();
        this.resizer.disconnect();
        this.discard('diagram');
        this.discard('error');
      },

      // Draws the source in the current theme. A newer call makes an older one that is still
      // waiting for Mermaid give up.
      async draw() {
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
          const message = String(err?.message ?? err).split('\n')[0];
          if (run === this.drawing) this.show({ message: `Diagram error: ${message}` });
        }
      },

      show(view) {
        this.view = view;
        // A new drawing replaces the old one; apply() creates what is missing.
        this.discard('diagram');
        this.discard('error');
        this.apply();
      },

      // Makes the page match `view`: the diagram in a sibling `<div>` and the source hidden, or
      // the source with a one-line error below it. It only touches what differs, because its own
      // changes trigger the observer again.
      apply() {
        if (!this.view) return;
        const { svg, message } = this.view;
        if (this.$el.hidden !== (svg !== undefined)) this.$el.hidden = svg !== undefined;
        if (svg !== undefined) {
          if (this.sibling('diagram', 'div', 'diagram')) {
            this.diagram.innerHTML = svg;
            this.resizer.observe(this.diagram);
          }
        } else if (this.sibling('error', 'p', 'diagram-error')) {
          this.error.textContent = message;
        }
      },

      // Mermaid sizes the iframe for the SVG at its natural width, but the SVG shrinks to the
      // iframe's width: the height follows the scale, so no empty space is left below.
      fit() {
        const frame = this.diagram?.querySelector('iframe');
        const box = frame && viewBoxOf(frame);
        if (!box || !frame.clientWidth) return;
        frame.style.height = `${Math.ceil(box.height * Math.min(1, frame.clientWidth / box.width))}px`;
      },

      // Removes the sibling `this[name]`; the resizer only ever observes the current diagram.
      discard(name) {
        if (!this[name]) return;
        this.resizer.unobserve(this[name]);
        this[name].remove();
        this[name] = null;
      },

      // Ensures the element `this[name]` follows the `<pre>`; true if it had to be created.
      sibling(name, tag, className) {
        if (this[name]?.isConnected) return false;
        this.discard(name); // one an htmx morph removed
        this[name] = this.$el.insertAdjacentElement('afterend', document.createElement(tag));
        this[name].className = className;
        return true;
      },
    }));
  });
})();
