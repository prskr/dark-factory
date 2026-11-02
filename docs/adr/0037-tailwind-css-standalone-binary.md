# ADR-0037: Tailwind CSS via its standalone binary

- **Status:** Accepted
- **Date:** 2026-10-17
- **Related:** [ADR-0004](0004-server-rendered-ui-maud-htmx-alpine.md), [ADR-0024](0024-build-tooling-and-dev-loop.md), [ADR-0035](0035-ui-form-factors-phone-for-core-flows.md)

## Context

The UI needs a CSS approach that supports the mobile-first, responsive layout of ADR-0035. The build must not depend on Node (ADR-0004); styles should stay close to the maud templates.

## Decision

- Tailwind CSS (v4), built with its **standalone CLI binary** — no Node, no npm.
- The binary is a pinned, checksum-verified download in Bazel and runs as a build action; its output stylesheet is embedded into the control plane binary with the other static assets (`rust-embed`).
- Tailwind scans the UI crate's `.rs` files for class names (`@source`). Classes are written as complete string literals in `class="…"`; maud's `.class` shorthand is used only for plain names, since variants like `md:flex` or `w-1/2` can't be expressed with it. No class names assembled at runtime.
- Design tokens (colours, spacing, typography) live in the Tailwind theme in CSS (`@theme`); a few component classes (`@apply`) only where the same markup repeats and a maud component function isn't enough.

## Consequences

- Responsive variants (`sm:`, `md:`, …) give the phone-first layout with little custom CSS.
- One more pinned tool in the build; Cargo-only builds (without Bazel) need the binary on the path or a checked-in stylesheet.
- Stylesheet size depends on the classes actually used, not on a framework bundle.

## Alternatives considered

- **Hand-written CSS**: no tool, but layout and responsive rules would be written and maintained by hand.
- **Tailwind via npm/PostCSS**: requires Node.
- **Classless or component CSS frameworks (Pico, Bulma)**: less control over a dense, transcript-heavy layout.
