# Review log: iter3-markdown-rendering

- BASE: 4f9c016511e0841e6371b985761404600cb67d07
- Store: none
- Status: done
- Implementer agent: a5ae0e7d2d256bc8d

## Round 1 (2026-11-05)

- Implementer: e6d0eafa, tasks 17/17, checks fmt/clippy/machete/deny/file-length/check-crates/openspec/helm-unittest (109)/docs-build ok; cargo df-web 645, df-core 157; bazel build lockfile_mode=error ok, bazel tests ok; container tests ran (podman)
- Reviewed range: BASE...e6d0eafa
- Deviations reported: own syntect adapter (no comrak `syntect` feature); syntect features `default-syntaxes, default-themes, html, regex-fancy`; deny ignore RUSTSEC-2025-0141 (bincode 1); file-length allowlist for vendored mermaid.min.js (11.17.2); known gap: htmx morph of an already drawn diagram restores the plain `<pre>`.

### Browser check (tasks 3.4, 4.4)

- Flowchart renders in a sandboxed iframe (`allow-top-navigation-by-user-activation allow-popups`), height fits the SVG (115 px).
- Invalid diagram shows source plus "Diagram error: Parse error on line 3:".
- `%%{init: {"securityLevel":"loose"}}%%` with `<img onerror>` label: stays sandboxed, no script ran (img inside iframe still requests a relative URL, 404).
- Page without diagrams: no `mermaid.min.js` request.
- Color scheme switch redraws diagrams and swaps code themes without reload. Code backgrounds `#ffffff` / `#2b303b`, also inside bubbles. Text contrast 12.6:1 / ~7.6:1; comment colour of both default themes below 3:1 (theme design).

### Findings

Security reviewer: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | P1 | `crates/df-web/assets/markdown.js:277-296`, `crates/df-web/src/sse.rs:298` | Diagram (sibling div + `hidden` on `<pre>`) is lost on a later `outerMorph`; request cards with a mermaid fence are morphed in place on state change and fall back to source | fix-now | fixed (26da9ed0) |
| P2 | Spec | P1 | `design.md` D2/D5, `proposal.md` Impact, `tasks.md` 2.1 | Change docs describe the comrak `SyntectAdapter` wrapper, `default-fancy`, `data-mermaid` marker; built design differs; bincode advisory missing in Risks | fix-now | fixed (26da9ed0) |
| V1 | Browser (orchestrator) | P1 | `crates/df-web/assets/markdown.js` | Diagram iframe ~50 px taller than the diagram (screenshots light-code.png/dark-code.png) | fix-now | fixed (26da9ed0) |
| S2 | Standards | P1 | `crates/df-web/src/markdown.rs:194-196` | `.ok()?` discards a syntect parse error without the comment AGENTS.md requires | fix-now | fixed (26da9ed0) |
| S1 | Standards | P1 | `crates/df-web/src/markdown.rs:112` | `CODE_THEME_PATH` const placed after a public fn | fix-now | fixed (26da9ed0) |
| S3 | Standards | P1 | `crates/df-web/src/markdown.rs:166-172` | `impl Default for CodeTheme` only for tests, with `expect` in production code | fix-now | fixed (26da9ed0) |
| S5 | Standards | P1 | `crates/df-web/src/layout.rs:68-69` | `code_theme_url: String`, empty means none → `Option<String>` | fix-now | fixed (26da9ed0) |
| S6 | Standards | P1 | `crates/df-web/src/markdown.rs:72-76` | `{source}` in the message duplicates the error cause in the anyhow chain | fix-now | fixed (26da9ed0) |
| P3 | Spec | P1 | `crates/dark-factory/src/main.rs:656-659` | No test that startup (`web_router`) fails for an unknown theme name | fix-now | fixed (26da9ed0) |
| P4 | Spec | P1 | `review-log.md` | Browser check does not say whether the diagram was seen after a live turn-end morph or only on a seeded page | fix-now | fixed (26da9ed0) |
| S4 | Standards | P3 | `crates/df-web/src/markdown.rs:83` | `theme_css` could be private | fix-now | fixed (26da9ed0) |
| S7 | Standards | P3 | `crates/df-web/src/lib.rs:186-197` | Router test repeats header checks of the markdown route test | fix-now | fixed (26da9ed0) |
| S8 | Standards | P3 | `crates/df-web/src/transcript/mod.rs:620` | Full path `crate::markdown::render` vs import elsewhere | fix-now | fixed (26da9ed0) |
| S9 | Standards | P3 | docs, README, values.yaml, schema | Theme list written out in four places | none | cosmetic |
| S10 | Standards | P3 | `crates/df-web/src/markdown.rs:252` | 215-line `tests.rs` could be inline (guidance) | none | cosmetic |

User decision: fix Blocking (none) + all P1 + S4, S7, S8. S9, S10 stay cosmetic.

## Round 2 (2026-11-05)

- Implementer: 26da9ed0, tasks 17/17, checks fmt/clippy/machete/deny/file-length/openspec ok; cargo df-web 647, df-core 157, dark-factory 45+3; bazel test df-web/dark-factory/df-core ok; container tests ran (podman)
- Reviewed range: e6d0eafa...26da9ed0
- Extra change: request card Markdown now sits in `.md-body` (diagram backdrop, code padding), with test.

### Browser check (fix round 1)

- P1: request card with a diagram morphed via SSE `outerMorph` (blocking flag change): still exactly one `.diagram` and one iframe, `<pre>` hidden.
- V1: flowchart iframe 81 px (was 115), tall card diagram 456 px, no empty space.
- P4 live path: `agent_message_chunk` + `turn_ended` inserted and notified to the running control plane; streaming message plain, after turn end `.md-body` with `hl-` classes and a drawn diagram, Mermaid loaded.

### Findings

Security reviewer: 0 findings. Ledger: P1–P4, V1, S1–S8 confirmed fixed by the reviewers.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P5 | Spec | Blocking | `docs/content/using/requests.md:33-34` | Published Requests page says request texts are plain text; they render through the shared Markdown renderer (highlighted code, diagrams) | fix-now | fixed (02b8a52d) |
| S11 | Standards | P1 | `crates/df-web/assets/markdown.js:98-118` | `show()`/`sibling()` replace the diagram div without `resizer.unobserve`; removed divs stay observed until `destroy()` | fix-now | fixed (02b8a52d) |
| S12 | Standards | P1 | `crates/dark-factory/src/main.rs:849-860` | Unknown-theme test also asserts a valid name passes; variable `defaults` misnamed | fix-now | fixed (02b8a52d) |
| S14 | Standards | P1 | `crates/df-web/src/markdown.rs:4` | Module doc links private `theme_css` (`private_intra_doc_links`) | fix-now | fixed (02b8a52d) |
| S13 | Standards | P3 | `crates/df-web/src/layout.rs:68` | Doc comment "none, no link." not a sentence | none | cosmetic |

User decision: fix Blocking + P1 (P5, S11, S12, S14). S13 cosmetic.

## Round 3 (2026-11-05)

- Implementer: 02b8a52d, checks clippy/fmt/docs-build ok; cargo df-web 647, dark-factory 46+3; bazel test df-web/dark-factory ok; container tests ran (podman). Disk filled twice during the round (target/debug cleaned, tests re-run).
- Reviewed range: 26da9ed0...02b8a52d

### Findings

Security: 0. Spec: 0. Ledger: P5, S11, S12, S14 confirmed fixed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S16 | Standards | P3 | `crates/dark-factory/src/main.rs:861` | Two theme tests repeat the same config YAML; a helper could share it | fix-now | fixed (04ed8109) |

User decision: fix S16 now.

## Round 4 (2026-11-05)

- Implementer: 04ed8109 (S16: test helper `config_with_dark_theme`), fmt/clippy ok, `cargo test -p dark-factory` 46+3; container tests ran.
- Reviewed range: 02b8a52d...04ed8109, checked by the orchestrator (15-line test-only change, no reviewer agents).

## Finish (2026-11-05)

- Container tests: ran in every round (podman socket), none skipped.
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ok.
- Docs: `just docs-build` ok. Pages changed: `docs/content/using/tasks.md`, `docs/content/using/requests.md`, `crates/df-agent-channel/docs/configuration.md` (published); `crates/df-web/README.md`, `deploy/helm/dark-factory/README.md`, `docs/design-system.md` (contributor/chart).
- Tech debt: none (no P2 findings).
- Deferred work: already in `product/iterations/roadmap.md` backlog (tool-call highlighting, `.tmTheme` files, bubble-aware themes, CSP).
- Cosmetic, not fixed: S9, S10, S13.
