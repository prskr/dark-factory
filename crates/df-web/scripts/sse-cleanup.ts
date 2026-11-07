// htmx extension that ends an SSE stream's reader before `hx-sse` cleans the stream up.
//
// When htmx removes an element with an open stream (a task opened from the list swaps
// `#ws-detail`), the cleanup hook of `hx-sse` aborts the request and then cancels the reader of
// the aborted stream without handling the rejection: an uncaught `AbortError` in the console.
// htmx calls the cleanup hooks of the extensions in registration order, and this file is loaded
// between htmx and `hx-sse` (`layout.rs`), so this hook runs first: it cancels the reader of the
// still open stream, handles the result and takes the reader away, leaving `hx-sse` only the abort.

htmx.registerExtension('sse-reader-cleanup', {
  htmx_before_cleanup: (element: Element) => {
    const sse = (element as SseElement)._htmx?.sse;
    const reader = sse?.reader;
    if (!sse || !reader) return;
    sse.reader = null;
    reader.cancel().catch(() => undefined);
  },
});
