// Alpine component of the traffic page (`/egress/log`): pause, graph divider and details panel.
// `cfg` carries the server values (see `state_args` in `src/egress/log/page.rs`).
interface EgressLogConfig {
  advanced: number;
  dirty: boolean;
  defaultWidth: number;
  widthKey: string;
  advancedFields: string[];
  minGraphPanel: number;
  dividerWidth: number;
  minTableWidth: number;
}

document.addEventListener('alpine:init', () => {
  Alpine.data('egressLog', (cfg: EgressLogConfig) => ({
    paused: false,
    reconnecting: false,
    adv: cfg.advanced,
    advOpen: cfg.advanced > 0,
    dirty: cfg.dirty,
    w: cfg.defaultWidth,
    want: cfg.defaultWidth,
    dragging: false,
    startX: 0,
    startW: 0,
    graphDragging: false,
    gx: 0,
    gy: 0,
    flow: null as Record<string, unknown> | null,
    flowId: '',
    flowRow: null as HTMLElement | null,
    init(): void {
      try {
        const stored = parseInt(localStorage.getItem(cfg.widthKey) ?? '', 10)
        if (stored) this.want = stored
      } catch (e) {}
      // The stored width may not fit this window: clamp it once the layout exists.
      this.$nextTick(() => this.setW(this.want, false))
      document.addEventListener('htmx:sse:error', () => { this.reconnecting = true })
      document.addEventListener('htmx:sse:after:connection', () => { this.reconnecting = false })
      document.addEventListener('htmx:after:swap', () => { this.swapped() })
    },
    badge(): string { return this.paused ? 'Paused' : this.reconnecting ? 'Reconnecting' : 'Live' },
    togglePause(): void {
      const slot = document.getElementById('sse-slot')
      if (!slot) return
      this.paused = !this.paused
      if (this.paused) {
        // Nothing observes the removal, so the extension's cleanup would never run: stop the
        // stream explicitly, then remove the element. A reconnect in flight has no reader yet,
        // only the abort ends it. Cancelling the reader of an aborted stream rejects with the
        // abort error: swallow it.
        const connected = slot.firstElementChild as SseElement | null
        const sse = connected && connected._htmx && connected._htmx.sse
        if (sse) {
          if (sse.abortController) sse.abortController.abort()
          if (sse.reader && sse.reader.cancel) sse.reader.cancel().catch(() => undefined)
        }
        slot.replaceChildren()
        this.reconnecting = false
      } else {
        const connect = document.createElement('div')
        connect.setAttribute('hx-sse:connect', slot.dataset.url ?? '')
        connect.setAttribute('hx-swap', 'none')
        slot.append(connect)
        htmx.process(connect)
      }
    },
    swapped(): void {
      const slot = document.getElementById('sse-slot')
      // A filter change swaps in a connected stream: that resumes.
      if (this.paused && slot && slot.firstElementChild) this.paused = false
      // New rows come in on top: drop the oldest past the row limit (the server does the same).
      const tbody = document.querySelector<HTMLTableSectionElement>('#flows tbody')
      const limit = tbody ? parseInt(tbody.dataset.limit ?? '', 10) : 0
      if (tbody && limit) while (tbody.rows.length > limit) tbody.lastElementChild?.remove()
      this.refreshFlow()
    },
    advancedCount(form: HTMLFormElement): number {
      return cfg.advancedFields.filter((name) => (form.elements.namedItem(name) as HTMLInputElement).value.trim() !== '').length
    },
    sync(form: HTMLFormElement): void {
      this.adv = this.advancedCount(form)
      const verdict = form.querySelector<HTMLInputElement>('input[name=verdict]:checked')
      const q = form.elements.namedItem('q') as HTMLInputElement
      this.dirty = this.adv > 0 || q.value.trim() !== '' || (verdict !== null && verdict.value !== 'all')
    },
    clearFilters(form: HTMLFormElement): void {
      form.querySelectorAll<HTMLInputElement | HTMLSelectElement>('input[type=search], input[type=text], select[name=persona]').forEach((input) => { input.value = '' })
      const all = form.querySelector<HTMLInputElement>('input[name=verdict][value=all]')
      if (all) all.checked = true
      this.adv = 0
      this.dirty = false
      form.dispatchEvent(new Event('change', { bubbles: true }))
    },
    limits(): [number, number] {
      const total = this.$refs.split ? this.$refs.split.clientWidth : 0
      return [cfg.minGraphPanel, Math.max(cfg.minGraphPanel, total - cfg.dividerWidth - cfg.minTableWidth)]
    },
    // `want` is what the board member chose (and the browser remembers); `w` is what fits now.
    // Fitting alone (`persist` false: start, window resize) never changes the remembered width.
    setW(value: number, persist = true): void {
      const [lo, hi] = this.limits()
      this.w = Math.min(hi, Math.max(lo, Math.round(persist ? value : this.want)))
      if (persist) {
        this.want = this.w
        try { localStorage.setItem(cfg.widthKey, String(this.want)) } catch (e) {}
      }
    },
    dragStart(e: PointerEvent): void {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
      this.dragging = true
      this.startX = e.clientX
      this.startW = this.w
    },
    dragMove(e: PointerEvent): void { if (this.dragging) this.setW(this.startW + e.clientX - this.startX) },
    dragEnd(): void { this.dragging = false },
    // Drag the graph background to scroll it; node cards keep their text selectable.
    graphDragStart(e: PointerEvent): void {
      const box = e.currentTarget as HTMLElement
      if (e.button !== 0 || (e.target as HTMLElement).closest('[data-node]')) return
      // A press on the scrollbar scrolls natively.
      const rect = box.getBoundingClientRect()
      if (e.clientX - rect.left - box.clientLeft >= box.clientWidth || e.clientY - rect.top - box.clientTop >= box.clientHeight) return
      box.setPointerCapture(e.pointerId)
      this.graphDragging = true
      this.gx = e.clientX + box.scrollLeft
      this.gy = e.clientY + box.scrollTop
    },
    graphDragMove(e: PointerEvent): void {
      if (!this.graphDragging) return
      const box = e.currentTarget as HTMLElement
      box.scrollLeft = this.gx - e.clientX
      box.scrollTop = this.gy - e.clientY
    },
    graphDragEnd(): void { this.graphDragging = false },
    divKey(e: KeyboardEvent): void {
      const step = e.shiftKey ? 96 : 32
      const [lo, hi] = this.limits()
      const targets: Record<string, number | undefined> = { ArrowLeft: this.w - step, ArrowRight: this.w + step, Home: lo, End: hi }
      const to = targets[e.key]
      if (to === undefined) return
      e.preventDefault()
      this.setW(to)
    },
    openDetails(button: HTMLElement): void {
      const row = button.closest('tr')
      if (!row) return
      this.flowId = row.id
      this.flowRow = row
      this.flow = JSON.parse(row.dataset.flow ?? 'null')
      this.$nextTick(() => this.$refs.closeButton.focus())
    },
    rowOf(): HTMLElement | null {
      if (this.flowId) return document.getElementById(this.flowId)
      return this.flowRow && this.flowRow.isConnected ? this.flowRow : null
    },
    refreshFlow(): void {
      if (!this.flow) return
      const row = this.rowOf()
      if (!row) { this.flow = null; return }
      this.flowRow = row
      this.flow = JSON.parse(row.dataset.flow ?? 'null')
    },
    close(): void {
      const row = this.rowOf()
      this.flow = null
      this.flowId = ''
      this.flowRow = null
      const opener = row && [...row.querySelectorAll('button')].find((b) => b.offsetParent !== null)
      if (opener) opener.focus()
    },
  }))
})
