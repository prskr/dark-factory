// Alpine component of the transcript's scroll container (`x-data="transcriptScroll"`).
//
// Follows new and growing items (DOM mutations) and a changed viewport or content height (queued
// messages, loaded images) while the reader is within `threshold` px of the bottom. A swapped-in
// container runs `init()` again, so opening another task lands at the bottom too.
//
// State: `pinned` (the reader is at the bottom), `threshold` (px). Methods: `onScroll()` (bound by
// `x-on:scroll`), `init()` and `destroy()`, which disconnects the observers.
document.addEventListener('alpine:init', () => {
  Alpine.data('transcriptScroll', () => ({
    pinned: true,
    threshold: 48,
    observers: [] as Array<MutationObserver | ResizeObserver>,
    init(): void {
      const el = this.$el
      const down = (): void => { if (this.pinned) el.scrollTop = el.scrollHeight }
      down()
      const mutations = new MutationObserver(down)
      mutations.observe(el, { childList: true, subtree: true, characterData: true })
      const viewport = new ResizeObserver(down)
      viewport.observe(el)
      // The transcript is the container's only child.
      const content = new ResizeObserver(down)
      if (el.firstElementChild) content.observe(el.firstElementChild)
      this.observers = [mutations, viewport, content]
    },
    destroy(): void {
      this.observers.forEach((observer) => observer.disconnect())
      this.observers = []
    },
    onScroll(): void {
      const el = this.$el
      this.pinned = el.scrollHeight - el.scrollTop - el.clientHeight < this.threshold
    },
  }))
})
