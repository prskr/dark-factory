// Alpine component of the artifact split view on the task page (`x-data="artifactSplit({...})"` on
// `#ws-detail`, see `split_args` in `src/artifacts/view.rs`). The transcript, the details column
// and the pane all sit inside it.
//
// Config: `key` (task key), `splitKey` (localStorage key of the pane's share of the split),
// `defaultFraction` (share when nothing is stored), `minPane` and `minTranscript` (pixels).
//
// State:
// - `openName`: the artifact open in the pane, `''` for none.
// - `frac`: the pane's share of the split row (0 to 1), so the pane and the transcript both grow
//   when the task list collapses; remembered in `localStorage` inside `try/catch`. The pane's
//   `min-width` and `max-width` (see `paneStyle()`) keep both parts usable.
// - `dragging`, `startX`, `startW`: the running drag of the divider.
// - `restore`: the `collapsed` (column from 1280 px) and `details` (slide-over below it) state of
//   the workspace from before the pane opened, or `null`. Opening an artifact collapses the
//   column and hides the slide-over; closing restores both. This is temporary and never stored:
//   it sets the flags directly, not the workspace's method that writes to `localStorage`.
//
// `init()` watches the transcript: a row with `data-artifact-written` that is added or changes
// triggers `artifacts-changed` (the Artifacts tab reloads itself on it) and reloads the open
// artifact if it has that name.
//
// Methods: `paneStyle()` (the pane's inline style), `open(name)` (below 768 px it opens the artifact page), `close()`, `reload()`, the
// divider's `dragStart`/`dragMove`/`dragEnd` pointer sequence and `dividerKey(event)` (arrows).
interface ArtifactSplitConfig {
  key: string;
  splitKey: string;
  defaultFraction: number;
  minPane: number;
  minTranscript: number;
}

const PHONE_BELOW = 768;
const KEY_STEP = 24;

document.addEventListener('alpine:init', () => {
  Alpine.data('artifactSplit', (cfg: ArtifactSplitConfig) => ({
    openName: '',
    frac: cfg.defaultFraction,
    dragging: false,
    startX: 0,
    startW: 0,
    restore: null as { collapsed: boolean; details: boolean } | null,

    init(): void {
      try {
        const stored = parseFloat(localStorage.getItem(cfg.splitKey) ?? '');
        if (stored > 0 && stored < 1) this.frac = stored;
      } catch (e) {}
      const transcript = document.getElementById('transcript');
      if (!transcript) return;
      new MutationObserver((records) => this.written(records)).observe(transcript, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: ['data-artifact-written'],
      });
    },

    url(name: string): string {
      return `/tasks/${cfg.key}/artifacts/${encodeURIComponent(name)}`;
    },

    workspace(): { collapsed: boolean; details: boolean } | null {
      // Not `$el.closest`: a click on the pane's Close button runs with that button as `$el`, which
      // is gone once the pane is emptied.
      const ws = document.querySelector('.ws');
      return ws ? Alpine.$data(ws) : null;
    },

    open(name: string): void {
      if (window.innerWidth < PHONE_BELOW) {
        window.location.assign(this.url(name));
        return;
      }
      const ws = this.workspace();
      if (ws && this.restore === null) {
        this.restore = { collapsed: ws.collapsed, details: ws.details };
        ws.collapsed = true;
        ws.details = false;
      }
      this.openName = name;
      void this.reload();
    },

    close(): void {
      this.openName = '';
      document.getElementById('artifact-pane')?.replaceChildren();
      const ws = this.workspace();
      if (ws && this.restore !== null) {
        ws.collapsed = this.restore.collapsed;
        ws.details = this.restore.details;
      }
      this.restore = null;
    },

    async reload(): Promise<void> {
      const name = this.openName;
      if (!name) return;
      try {
        const response = await fetch(this.url(name), { headers: { 'HX-Request': 'true' } });
        if (name !== this.openName) return;
        const pane = document.getElementById('artifact-pane');
        if (response.ok && pane) pane.innerHTML = await response.text();
        else if (pane) pane.replaceChildren();
      } catch (e) {}
    },

    written(records: MutationRecord[]): void {
      const names = new Set<string>();
      for (const record of records) {
        const nodes = record.type === 'attributes' ? [record.target] : [...record.addedNodes];
        for (const node of nodes) {
          if (!(node instanceof Element)) continue;
          const rows = [...node.querySelectorAll('[data-artifact-written]')];
          if (node.matches('[data-artifact-written]')) rows.push(node);
          for (const row of rows) names.add(row.getAttribute('data-artifact-written') ?? '');
        }
      }
      if (names.size === 0) return;
      document.body.dispatchEvent(new CustomEvent('artifacts-changed'));
      if (names.has(this.openName)) void this.reload();
    },

    paneStyle(): string {
      return `width:${(this.frac * 100).toFixed(2)}%;min-width:${cfg.minPane}px;max-width:calc(100% - ${cfg.minTranscript}px)`;
    },

    paneWidth(): number {
      return document.getElementById('artifact-pane')?.getBoundingClientRect().width ?? 0;
    },

    setPaneWidth(px: number): void {
      const row = document.getElementById('artifact-pane')?.parentElement?.clientWidth ?? 0;
      if (row <= 0) return;
      const low = cfg.minPane / row;
      const high = Math.max(low, 1 - cfg.minTranscript / row);
      this.frac = Math.max(low, Math.min(high, px / row));
    },

    dragStart(e: PointerEvent): void {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      this.dragging = true;
      this.startX = e.clientX;
      this.startW = this.paneWidth();
    },

    dragMove(e: PointerEvent): void {
      if (this.dragging) this.setPaneWidth(this.startW - (e.clientX - this.startX));
    },

    dragEnd(): void {
      if (!this.dragging) return;
      this.dragging = false;
      this.store();
    },

    dividerKey(e: KeyboardEvent): void {
      const step = e.key === 'ArrowLeft' ? KEY_STEP : e.key === 'ArrowRight' ? -KEY_STEP : 0;
      if (!step) return;
      e.preventDefault();
      this.setPaneWidth(this.paneWidth() + step);
      this.store();
    },

    store(): void {
      try { localStorage.setItem(cfg.splitKey, String(this.frac)) } catch (e) {}
    },
  }));
});
