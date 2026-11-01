# Tasks

## 1. Brand glyphs (design D1, D2)

- [ ] 1.1 Write failing tests in `crates/df-web/src/components/host_icon.rs`: `api.github.com` renders `data-host-icon="brand-github"` with the Octocat path, `api.anthropic.com` renders `brand-anthropic`, `evilgithub.com` the globe, a test-only brand entry without glyph renders its monogram; update the existing `GH` assertions in `egress/tests/icons.rs` and the allowlist page tests. Verify they fail with `cargo test -p df-web host_icon` and `cargo test -p df-web icons`
- [ ] 1.2 Add `Icon::{Anthropic, Npm, Go, Python, Google}` as filled 24×24 Simple Icons (CC0) paths in `components/icons.rs`, with source and licence in a doc comment next to them; reuse `Icon::GitHub`. Verify `cargo test -p df-web icons` passes and the paths use `fill="currentColor" stroke="none"`
- [ ] 1.3 Extend `BRANDS` with an `Option<Icon>` glyph and render it in the tone tile in `host_icon`, monogram as fallback. Verify the tests of 1.1 pass
- [ ] 1.4 Check glyph contrast against each tone tile in light and dark theme (≥ 3:1, same tone foreground as the monograms) in a browser via `just serve-local` and the egress rules page; show the user a screenshot and note whether brand colours become a backlog item (design, Non-Goals)
- [ ] 1.5 Update `docs/design-system.md` (host icon section, line ~104): precedence role → glyph → monogram → server → globe, `data-host-icon="brand-<name>"` values, glyph source and licence; drop "only letters are drawn, no logos". Update the module doc of `host_icon.rs` ("Brands draw only letters")

## 2. Forge API host (design D3)

- [ ] 2.1 Write failing tests in `crates/df-core/src/routing.rs`: `ForgeRoutes::api_host_for_repo_url` returns `api.github.com` for `https://github.com/o/r.git` with `apiUrl` `https://api.github.com`, `api.ghe.example.com` for `apiUrl` `https://API.ghe.example.com:8443/api/v3`, `None` for a route without `apiUrl` and for an unrouted URL. Verify with `cargo test -p df-core routing`
- [ ] 2.2 Implement `ForgeRoutes::api_host_for_repo_url` from `Resolved::base_url`, normalized with `normalize_host`. Verify the tests of 2.1 pass
- [ ] 2.3 Write failing tests in `components/project_host.rs` / `egress/tests/icons.rs` for the spec scenarios "Project host before provider monogram", "Forge API on a separate host", "Forge API host without a served project" and the allowlist scenario "Forge API host"; then register the API host with `HostRole::Forge(kind)` in `ProjectHosts::from_projects` for projects with a forge kind. Verify `cargo test -p df-web project_host icons` passes
- [ ] 2.4 Update `docs/design-system.md` (host icon section): a project's forge API host (the `apiUrl` host of its route, exact match) shows the forge icon. Verify the doc names the same three scenarios as the spec

## 3. Integration checks

- [ ] 3.1 `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length` and `bazel --bazelrc=.bazelrc.agents build --config=fmt //crates/...` pass
- [ ] 3.2 `openspec validate brand-host-icons --strict` passes
- [ ] 3.3 With `just serve-local` and `just seed`, a GitHub project with a forge credential shows the GitHub forge icon for `github.com` and `api.github.com` on the network traffic page and the rules page

## Workflow follow-up

- Archive the change after review; check out-of-scope items against `product/iterations/roadmap.md` (both are "rejected", no backlog rows).
