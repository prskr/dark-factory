// Alpine component of the traffic page (`/egress/log`): pause, graph divider and details panel.
// `cfg` carries the server values (see `state_args` in `src/egress/log/page.rs`).
document.addEventListener('alpine:init', () => {
  Alpine.data('egressLog', (cfg) => ({
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
    flow: null,
    flowId: '',
    flowRow: null,
    init() {
      try {
        const stored = parseInt(localStorage.getItem(cfg.widthKey), 10)
        if (stored) this.want = stored
      } catch (e) {}
      // The stored width may not fit this window: clamp it once the layout exists.
      this.$nextTick(() => this.setW(this.want, false))
      document.addEventListener('htmx:sse:error', () => { this.reconnecting = true })
      document.addEventListener('htmx:sse:after:connection', () => { this.reconnecting = false })
      document.addEventListener('htmx:after:swap', () => { this.swapped() })
    },
    badge() { return this.paused ? 'Paused' : this.reconnecting ? 'Reconnecting' : 'Live' },
    togglePause() {
      const slot = document.getElementById('sse-slot')
      if (!slot) return
      this.paused = !this.paused
      if (this.paused) {
        // Nothing observes the removal, so the extension's cleanup would never run: stop the
        // stream explicitly, then remove the element. A reconnect in flight has no reader yet,
        // only the abort ends it. Cancelling the reader of an aborted stream rejects with the
        // abort error: swallow it.
        const sse = slot.firstElementChild && slot.firstElementChild._htmx && slot.firstElementChild._htmx.sse
        if (sse) {
          if (sse.abortController) sse.abortController.abort()
          if (sse.reader && sse.reader.cancel) sse.reader.cancel().catch(() => undefined)
        }
        slot.replaceChildren()
        this.reconnecting = false
      } else {
        const connect = document.createElement('div')
        connect.setAttribute('hx-sse:connect', slot.dataset.url)
        connect.setAttribute('hx-swap', 'none')
        slot.append(connect)
        htmx.process(connect)
      }
    },
    swapped() {
      const slot = document.getElementById('sse-slot')
      // A filter change swaps in a connected stream: that resumes.
      if (this.paused && slot && slot.firstElementChild) this.paused = false
      // New rows come in on top: drop the oldest past the row limit (the server does the same).
      const tbody = document.querySelector('#flows tbody')
      const limit = tbody ? parseInt(tbody.dataset.limit, 10) : 0
      if (limit) while (tbody.rows.length > limit) tbody.lastElementChild.remove()
      this.refreshFlow()
    },
    advancedCount(form) {
      return cfg.advancedFields.filter((name) => form.elements[name].value.trim() !== '').length
    },
    sync(form) {
      this.adv = this.advancedCount(form)
      const verdict = form.querySelector('input[name=verdict]:checked')
      this.dirty = this.adv > 0 || form.elements.q.value.trim() !== '' || (verdict && verdict.value !== 'all')
    },
    clearFilters(form) {
      form.querySelectorAll('input[type=search], input[type=text], select[name=persona]').forEach((input) => { input.value = '' })
      form.querySelector('input[name=verdict][value=all]').checked = true
      this.adv = 0
      this.dirty = false
      form.dispatchEvent(new Event('change', { bubbles: true }))
    },
    limits() {
      const total = this.$refs.split ? this.$refs.split.clientWidth : 0
      return [cfg.minGraphPanel, Math.max(cfg.minGraphPanel, total - cfg.dividerWidth - cfg.minTableWidth)]
    },
    // `want` is what the board member chose (and the browser remembers); `w` is what fits now.
    // Fitting alone (`persist` false: start, window resize) never changes the remembered width.
    setW(value, persist = true) {
      const [lo, hi] = this.limits()
      this.w = Math.min(hi, Math.max(lo, Math.round(persist ? value : this.want)))
      if (persist) {
        this.want = this.w
        try { localStorage.setItem(cfg.widthKey, String(this.want)) } catch (e) {}
      }
    },
    dragStart(e) {
      e.currentTarget.setPointerCapture(e.pointerId)
      this.dragging = true
      this.startX = e.clientX
      this.startW = this.w
    },
    dragMove(e) { if (this.dragging) this.setW(this.startW + e.clientX - this.startX) },
    dragEnd() { this.dragging = false },
    // Drag the graph background to scroll it; node cards keep their text selectable.
    graphDragStart(e) {
      const box = e.currentTarget
      if (e.button !== 0 || e.target.closest('[data-node]')) return
      // A press on the scrollbar scrolls natively.
      const rect = box.getBoundingClientRect()
      if (e.clientX - rect.left - box.clientLeft >= box.clientWidth || e.clientY - rect.top - box.clientTop >= box.clientHeight) return
      box.setPointerCapture(e.pointerId)
      this.graphDragging = true
      this.gx = e.clientX + box.scrollLeft
      this.gy = e.clientY + box.scrollTop
    },
    graphDragMove(e) {
      if (!this.graphDragging) return
      e.currentTarget.scrollLeft = this.gx - e.clientX
      e.currentTarget.scrollTop = this.gy - e.clientY
    },
    graphDragEnd() { this.graphDragging = false },
    divKey(e) {
      const step = e.shiftKey ? 96 : 32
      const [lo, hi] = this.limits()
      const to = { ArrowLeft: this.w - step, ArrowRight: this.w + step, Home: lo, End: hi }[e.key]
      if (to === undefined) return
      e.preventDefault()
      this.setW(to)
    },
    openDetails(button) {
      const row = button.closest('tr')
      this.flowId = row.id
      this.flowRow = row
      this.flow = JSON.parse(row.dataset.flow)
      this.$nextTick(() => this.$refs.closeButton.focus())
    },
    rowOf() {
      if (this.flowId) return document.getElementById(this.flowId)
      return this.flowRow && this.flowRow.isConnected ? this.flowRow : null
    },
    refreshFlow() {
      if (!this.flow) return
      const row = this.rowOf()
      if (!row) { this.flow = null; return }
      this.flowRow = row
      this.flow = JSON.parse(row.dataset.flow)
    },
    close() {
      const row = this.rowOf()
      this.flow = null
      this.flowId = ''
      this.flowRow = null
      const opener = row && [...row.querySelectorAll('button')].find((b) => b.offsetParent !== null)
      if (opener) opener.focus()
    },
  }))
})
