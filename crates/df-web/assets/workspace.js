// Alpine component of the task workspace (`x-data="workspace"` of `.ws`), shared by the list,
// the transcript and the details panel.
//
// State:
// - `details`: the details panel is open (below 1280 px).
// - `collapsed`: the details column is collapsed to its strip (from 1280 px); remembered in
//   `localStorage['df.details.collapsed']`, read and written in `try/catch`.
// - `sheet`: height of the phone details sheet, `'half'` or `'full'`.
// - `dragging`: the sheet handle is being dragged (the sheet must not animate then).
// - `drag`: `{ y, h, el, moved }` of the running drag; `dragged`: the last pointer sequence moved,
//   so the click that follows must not toggle the sheet.
//
// Thresholds (named values): `closeBelow` is the share of the viewport height under which a
// released sheet closes, `fullAbove` the share over which it becomes full height (otherwise half),
// `dragSlop` the pixels a press has to move before it counts as a drag.
//
// Methods: `setCollapsed(value)`, `sheetToggle()` (tap or keyboard on the handle), and the
// pointer sequence `sheetDragStart(e)`, `sheetDragMove(e)`, `sheetDragEnd(e)` (pointerup) and
// `sheetDragCancel(e)` (pointercancel: no click follows, so `dragged` is reset).
document.addEventListener('alpine:init', () => {
  Alpine.data('workspace', () => ({
    details: false,
    collapsed: false,
    sheet: 'half',
    dragging: false,
    drag: null,
    dragged: false,
    closeBelow: 0.25,
    fullAbove: 0.7,
    dragSlop: 4,
    init() {
      try { this.collapsed = localStorage.getItem('df.details.collapsed') === '1' } catch (e) {}
    },
    setCollapsed(value) {
      this.collapsed = value
      try { localStorage.setItem('df.details.collapsed', value ? '1' : '0') } catch (e) {}
    },
    sheetToggle() {
      if (this.dragged) { this.dragged = false; return }
      this.sheet = this.sheet === 'half' ? 'full' : 'half'
    },
    sheetDragStart(e) {
      const el = e.currentTarget.closest('aside')
      e.currentTarget.setPointerCapture(e.pointerId)
      this.drag = { y: e.clientY, h: el.offsetHeight, el, moved: false }
      this.dragging = true
    },
    sheetDragMove(e) {
      const d = this.drag
      if (!d) return
      const dy = e.clientY - d.y
      if (Math.abs(dy) > this.dragSlop) d.moved = true
      if (d.moved) d.el.style.height = Math.max(0, d.h - dy) + 'px'
    },
    sheetDragEnd(e) {
      const d = this.drag
      this.drag = null
      this.dragging = false
      if (!d) return
      d.el.style.height = ''
      this.dragged = d.moved
      if (!d.moved) return
      const share = (d.h - (e.clientY - d.y)) / window.innerHeight
      if (share < this.closeBelow) { this.details = false; this.sheet = 'half' }
      else this.sheet = share > this.fullAbove ? 'full' : 'half'
    },
    sheetDragCancel(e) {
      const d = this.drag
      this.drag = null
      this.dragging = false
      this.dragged = false
      if (d) d.el.style.height = ''
    },
  }))
})
