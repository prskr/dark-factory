// Alpine component of the chat workspace (`x-data="chatLayout"` of `.ws` on the chat pages), shared
// by the chat list, the transcript and the details panel. Same state and methods as the task
// workspace (`workspace.ts`: the artifact split view and the details panel read them), with its own
// remembered choices.
//
// State:
// - `details`: the details panel is open (below 1280 px).
// - `collapsed`: the details column is collapsed to its strip (from 1280 px); remembered in
//   `localStorage['df.chat.details.collapsed']`, read and written in `try/catch`.
// - `listCollapsed`: the chat list is collapsed to the strip of persona avatars (from 768 px);
//   remembered in `localStorage['df.chat.list.collapsed']`, read and written in `try/catch`.
// - `sheet`: height of the phone details sheet, `'half'` or `'full'`.
// - `dragging`, `drag`, `dragged`: the running drag of the sheet handle (see `workspace.ts`).
//
// Methods: `setCollapsed(value)`, `setListCollapsed(value)`, `selected(row, event)`, `sheetToggle()` and the pointer sequence
// `sheetDragStart(e)`, `sheetDragMove(e)`, `sheetDragEnd(e)` and `sheetDragCancel(e)`.
//
// shortcut: the sheet handling repeats `workspace.ts`; merge both once a third workspace needs it.
interface ChatSheetDrag {
  y: number;
  h: number;
  el: HTMLElement;
  moved: boolean;
}

document.addEventListener('alpine:init', () => {
  Alpine.data('chatLayout', () => ({
    details: false,
    collapsed: false,
    listCollapsed: false,
    sheet: 'half' as 'half' | 'full',
    dragging: false,
    drag: null as ChatSheetDrag | null,
    dragged: false,
    closeBelow: 0.25,
    fullAbove: 0.7,
    dragSlop: 4,
    init(): void {
      try { this.collapsed = localStorage.getItem('df.chat.details.collapsed') === '1' } catch (e) {}
      try { this.listCollapsed = localStorage.getItem('df.chat.list.collapsed') === '1' } catch (e) {}
    },
    setCollapsed(value: boolean): void {
      this.collapsed = value
      try { localStorage.setItem('df.chat.details.collapsed', value ? '1' : '0') } catch (e) {}
    },
    setListCollapsed(value: boolean): void {
      this.listCollapsed = value
      try { localStorage.setItem('df.chat.list.collapsed', value ? '1' : '0') } catch (e) {}
    },
    // A swap ended (`htmx:after:swap` on the document, bound by each chat row). When it is the
    // successful load of `row`'s chat: show the chat pane (phone), close the details and mark the
    // row. A failed load keeps the open chat, the list (phone) and the focus on the row.
    // `focusAfterSwap.ts` focuses the head after this, in a microtask.
    selected(row: HTMLElement, event: Event): void {
      const { ctx } = (event as CustomEvent<{ ctx?: { sourceElement?: Element; response?: { status: number } } }>).detail
      if (ctx?.sourceElement !== row || !ctx.response || !(ctx.response.status < 400)) return
      document.body.dataset.view = 'task'
      this.details = false
      row.closest('[data-rows]')?.querySelectorAll('[aria-current]').forEach((e) => e.removeAttribute('aria-current'))
      row.setAttribute('aria-current', 'page')
    },
    sheetToggle(): void {
      if (this.dragged) { this.dragged = false; return }
      this.sheet = this.sheet === 'half' ? 'full' : 'half'
    },
    sheetDragStart(e: PointerEvent): void {
      const handle = e.currentTarget as HTMLElement
      const el = handle.closest('aside')
      if (!el) return
      handle.setPointerCapture(e.pointerId)
      this.drag = { y: e.clientY, h: el.offsetHeight, el, moved: false }
      this.dragging = true
    },
    sheetDragMove(e: PointerEvent): void {
      const d = this.drag
      if (!d) return
      const dy = e.clientY - d.y
      if (Math.abs(dy) > this.dragSlop) d.moved = true
      if (d.moved) d.el.style.height = Math.max(0, d.h - dy) + 'px'
    },
    sheetDragEnd(e: PointerEvent): void {
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
    sheetDragCancel(e: PointerEvent): void {
      const d = this.drag
      this.drag = null
      this.dragging = false
      this.dragged = false
      if (d) d.el.style.height = ''
    },
  }))
})
