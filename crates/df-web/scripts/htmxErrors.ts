// Shows the "could not reach the server" toast when a request got no answer.
//
// Plain script (not an Alpine component), like `sse-cleanup.ts`. htmx fires `htmx:error` with
// `{ ctx, error }` when a request throws: a failed fetch, a timeout, and also an exception while
// swapping. `ctx.response` is set once `fetch` returned, so a missing response means that no
// answer arrived. An error without `ctx` is a script error. Streams that drop fire
// `htmx:sse:error` and reconnect, so they show nothing.
//
// The layout renders the toast region and the toast in a template and names both by selector in
// `data-toasts` and `data-network-error` of the body, so no element ID is written here.
document.addEventListener('htmx:error', (event) => {
  const { ctx } = (event as CustomEvent<{ ctx?: { response?: Response } }>).detail
  if (!ctx || ctx.response) return
  const region = selectorElement('data-toasts')
  const template = selectorElement('data-network-error')
  if (!region || !(template instanceof HTMLTemplateElement)) return
  region.replaceChildren(template.content.cloneNode(true))
})

function selectorElement(attribute: string): Element | null {
  const selector = document.body.getAttribute(attribute)
  return selector ? document.querySelector(selector) : null
}
