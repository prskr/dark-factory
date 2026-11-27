// Moves focus after a swap that a navigating element caused.
//
// An element with `data-focus-after-swap="<selector>"` (a row link of the task or chat list)
// focuses that element once its request was answered with a success status. An error answer shows
// a toast and leaves focus where it is, and so do swaps of elements without the attribute (live
// updates). Plain script (not an Alpine component), like `sse-cleanup.ts`.
//
// The focus waits for a microtask: this listener runs before Alpine's, and the row's own
// `selected()` (workspace.ts, chatLayout.ts) shows the phone pane of the head only after it.
document.addEventListener('htmx:after:swap', (event) => {
  const { ctx } = (event as CustomEvent<{ ctx?: { sourceElement?: Element; response?: { status: number } } }>).detail
  const selector = ctx?.sourceElement?.getAttribute('data-focus-after-swap')
  if (!selector || !ctx?.response || !(ctx.response.status < 400)) return
  queueMicrotask(() => document.querySelector<HTMLElement>(selector)?.focus())
})
