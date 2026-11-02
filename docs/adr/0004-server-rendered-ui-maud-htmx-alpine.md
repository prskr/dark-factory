# ADR-0004: Server-rendered UI with maud, htmx 4 and Alpine

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

The UI needs little interactivity beyond live agent activity streams and chat. Node.js should not be part of the build, and the author already uses htmx 4 and Alpine.js in other projects.

## Decision

- Server-rendered HTML with `maud` templates (compile-checked, similar to `templ` in Go). The UI lives in its own crate. Alpine's long attribute forms (`x-on:click`, `x-bind:class`) are used because `@` is maud syntax.
- `htmx.js` and `alpine.js` are vendored and embedded in the binary (`rust-embed`). No Node, no WASM toolchain.
- Activity stream: an axum SSE endpoint emits one HTML fragment per transcript event, appended with `beforeend`.
  - The in-progress assistant message has a stable id and is morphed as chunks arrive.
  - When the turn completes it's replaced with server-rendered Markdown (`comrak` + `syntect`); the same renderer serves artifacts.
- Token chunks are coalesced server-side (~100–200 ms) instead of one SSE event per token.
- SSE event ids are transcript sequence numbers; on reconnect the server replays everything after `Last-Event-ID`.
- Live dashboard state uses a page-level SSE stream fed by `LISTEN/NOTIFY`, delivering out-of-band swaps.
- Alpine handles small client-side pieces (textarea autosize, keyboard shortcuts, collapsing tool-call details).

## Consequences

- One binary, one image, no client build step and no WASM download.
- The compile loop is only the server binary.
- Must verify that htmx 4's fetch-based SSE sends `Last-Event-ID` on reconnect; fallback is a query parameter.

## Alternatives considered

- **TypeScript SPA (React/Svelte)**: requires Node.
- **Leptos (SSR + islands)**: slow compile loop on two targets, WASM bundles, API churn between minor versions, thin component ecosystem.
- **Datastar**: htmx 4 (fetch-based, SSE streaming, morph swaps) covers the same needs and is already known.
- **askama**: also compile-checked; maud preferred for its components-as-functions style.
