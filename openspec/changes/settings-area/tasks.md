# Tasks

## 1. Rail

- [ ] 1.1 Tests first (`components/shell.rs`, `layout.rs` tests): the rail lists Tasks, Requests, Projects, Network traffic and, at the bottom above the avatar, Settings (href `/personas`, icon-only, accessible name and title "Settings"); no Personas, Credentials, Model providers or Egress rules links; Settings has `aria-current="page"` for `Nav::Personas`, `Nav::Credentials`, `Nav::ModelProviders` and `Nav::Allowlist`. Add the Lucide `settings` icon and `SETTINGS_SECTIONS` (design D1); verify with `cargo test -p df-web shell` and `cargo test -p df-web layout`

## 2. Settings sidebar

- [ ] 2.1 Tests first: pages with a settings `Nav` render the sidebar (`hidden md:flex`, title "Settings", Personas, Credentials, Model providers, Egress rules with icons, the current one with `aria-current` and the bar); other pages don't (design D2). Implement in `layout.rs`; verify with `cargo test -p df-web layout`
- [ ] 2.2 Check the phone pages of personas and credentials at 390 px (no sidebar, no horizontal scroll) with Playwright; verify against the existing phone boards

## 3. Design and docs

- [ ] 3.1 Compare "Settings · personas" with `/personas` at 1440 px and the egress rules page at 1440 and 1280 px (`just serve-local`, Playwright, `docs/design-workflow.md`); fix defects or record deviations
- [ ] 3.2 PenPot: update the shared `Rail` component (page "Components") to the new rail (Settings above the avatar, no Personas, Credentials, Egress rules, Model providers; variant value `Active=Settings` replaces the four removed ones) and switch the boards that use the removed variants to `Active=Settings`; add the settings sidebar to the boards of those pages; verify by exporting "Personas · cards", "Network / Egress rules · table with task entries" and one task board
- [ ] 3.3 Update the docs that send users to those pages: `docs/content/using/setup.md` ("Settings → Credentials", "Settings → Personas"), `docs/content/integrations/forges/_index.md`, `docs/content/using/network-access.md` ("Settings → Egress rules"), `docs/content/using/model-providers.md` and `docs/content/operations/upgrades.md` ("Model providers" → "Settings → Model providers"), the diagram in `docs/content/using/_index.md` if it shows the navigation; add the board "Settings · personas" to "Mapping views to boards" in `docs/design-workflow.md`; verify with `just docs-build`
- [ ] 3.4 Update the rail requirement delta of `iter3-chats` (keep Chats, drop Personas, Credentials, Model providers and Egress rules); verify with `openspec validate iter3-chats --strict`

## 4. Integration checks

- [ ] 4.1 `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `just bazel-test` and the e2e suite (`just e2e-local`, container-backed; state in the report if it could not run) pass; `openspec validate settings-area --strict` passes
