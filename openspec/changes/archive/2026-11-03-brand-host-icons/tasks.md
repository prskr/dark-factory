# Tasks

## 1. Brand glyphs (design D1, D2)

- [x] 1.1 Write failing tests in `crates/df-web/src/components/host_icon.rs`: `api.github.com` renders `data-host-icon="brand-github"` with the Octocat path, `api.anthropic.com` renders `brand-anthropic`, `evilgithub.com` the globe, a test-only brand entry without glyph renders its monogram; update the existing `GH` assertions in `egress/tests/icons.rs` and the allowlist page tests. Verify they fail with `cargo test -p df-web host_icon` and `cargo test -p df-web icons`
- [x] 1.2 Add `Icon::{Anthropic, Npm, Go, Python, Google}` as filled 24×24 Simple Icons (CC0) paths in `components/icons.rs`, with source and licence in a doc comment next to them; reuse `Icon::GitHub`. Verify `cargo test -p df-web icons` passes and the paths use `fill="currentColor" stroke="none"`
- [x] 1.3 Extend `BRANDS` with an `Option<Icon>` glyph and render it in the tone tile in `host_icon`, monogram as fallback. Verify the tests of 1.1 pass
- [x] 1.4 Check glyph contrast against each tone tile in light and dark theme (≥ 3:1, same tone foreground as the monograms) in a browser via `just serve-local` and the egress rules page; show the user a screenshot and note whether brand colours become a backlog item (design, Non-Goals)
- [x] 1.5 Update `docs/design-system.md` (host icon section, line ~104): precedence role → glyph → monogram → server → globe, `data-host-icon="brand-<name>"` values, glyph source and licence; drop "only letters are drawn, no logos". Update the module doc of `host_icon.rs` ("Brands draw only letters")

## 2. Forge API host (design D3)

- [x] 2.1 Write failing tests in `crates/df-core/src/routing.rs`: `ForgeRoutes::api_host_for_repo_url` returns `api.github.com` for `https://github.com/o/r.git` with `apiUrl` `https://api.github.com`, `api.ghe.example.com` for `apiUrl` `https://API.ghe.example.com:8443/api/v3`, `None` for a route without `apiUrl` and for an unrouted URL. Verify with `cargo test -p df-core routing`
- [x] 2.2 Implement `ForgeRoutes::api_host_for_repo_url` from `Resolved::base_url`, normalized with `normalize_host`. Verify the tests of 2.1 pass
- [x] 2.3 Write failing tests in `components/project_host.rs` / `egress/tests/icons.rs` for the spec scenarios "Project host before provider monogram", "Forge API on a separate host", "Forge API host without a served project" and the allowlist scenario "Forge API host"; then register the API host with `HostRole::Forge(kind)` in `ProjectHosts::from_projects` for projects with a forge kind. Verify `cargo test -p df-web project_host icons` passes
- [x] 2.4 Update `docs/design-system.md` (host icon section): a project's forge API host (the `apiUrl` host of its route, exact match) shows the forge icon. Verify the doc names the same three scenarios as the spec

## 3. Integration checks

- [x] 3.1 `cargo clippy --all-targets --all-features -- -D warnings`, `cargo xtask check-file-length` and `bazel --bazelrc=.bazelrc.agents build --config=fmt //crates/...` pass
- [x] 3.2 `openspec validate brand-host-icons --strict` passes
- [x] 3.3 With `just serve-local` and `just seed`, a GitHub project with a forge credential shows the GitHub forge icon for `github.com` and `api.github.com` on the network traffic page and the rules page

## 4. More providers (design D2)

- [x] 4.1 Write failing tests: extend `every_known_provider_domain_shows_its_glyph` to all 21 domains of the "Known providers" scenario; switch every test that uses `ghcr.io` as the globe example to `example.org`. Verify they fail with `cargo test -p df-web host_icon` and `cargo test -p df-web rules`
- [x] 4.2 Add `Icon::{Bazel, Kubernetes, NodeJs, Rust, GitLab}` as Simple Icons (CC0) consts under the existing licence note in `components/icons.rs`; add the `BRANDS` entries for the new domains (reuse the Google, GitHub and Go entries' name, tone and glyph for their extra domains). Verify the tests of 4.1 pass
- [x] 4.3 Update the domain and glyph list in `docs/design-system.md` (host icon section)
- [x] 4.4 Live check of the rules page in light and dark theme: every new glyph has a contrast of at least 3:1 against its tile and is readable at table size
- [x] 4.5 Run the checks of 3.1 again and `openspec validate brand-host-icons --strict`
- [x] 4.6 Add `helm.sh` → Helm: extend `every_known_provider_domain_shows_its_glyph` with `helm.sh` first (failing), then `Icon::Helm` (Simple Icons, CC0) and its `BRANDS` entry; update `docs/design-system.md`; live check of `get.helm.sh` on the rules page in both themes (≥ 3:1)
- [x] 4.7 Add AWS: extend `every_known_provider_domain_shows_its_glyph` with `amazonaws.com`, `ecr.aws`, `awsstatic.com`, `aws.amazon.com` and `cloudfront.net` first (failing), plus a check that `amazon.com` and `www.amazon.com` stay the globe; then `Icon::Aws` (Simple Icons `amazonwebservices`, CC0) and one `BRANDS` entry; update `docs/design-system.md`; live check of an S3 host (`*.s3.eu-west-1.amazonaws.com`) on the rules page in both themes (≥ 3:1)
- [x] 4.8 Add Codeberg: extend `every_known_provider_domain_shows_its_glyph` with `codeberg.org` first (failing), plus a test that a Codeberg project served by the forgejo integration keeps `forge-forgejo` on `codeberg.org`; then `Icon::Codeberg` (Simple Icons `codeberg`, CC0) and its `BRANDS` entry; update `docs/design-system.md`; live check of `codeberg.org` on the rules page in both themes (≥ 3:1, 16 px)
- [x] 4.9 Add `github.io` to the GitHub entry: extend `every_known_provider_domain_shows_its_glyph` with `github.io` first (failing), then add the domain to `BRANDS`; update `docs/design-system.md`; live check of a `*.github.io` host on the rules page
- [x] 4.10 Add Linux distributions: extend `every_known_provider_domain_shows_its_glyph` with `debian.org`, `ubuntu.com`, `alpinelinux.org`, `fedoraproject.org`, `redhat.com`, `redhat.io` and the package hosts `deb.debian.org`, `archive.ubuntu.com`, `dl-cdn.alpinelinux.org`, `mirrors.fedoraproject.org`, `registry.redhat.io` first (failing); then the five providers with their Simple Icons (CC0) glyphs (as `brands.json` entries since 4.11); update `docs/design-system.md`; live check on the rules page in both themes (≥ 3:1, 16 px)
- [x] 4.11 Move the providers to `components/brands.json` (design D1, D2): first a failing test that parses the file and checks unique names, normalized unique domains, non-empty path data and 1–3 character monograms; then load it with `include_str!` + `serde_json` into a `LazyLock`, deserialize `tone` into `badges::Tone`, render the path with maud escaping, let the GitHub forge icon use the `github` entry, and remove the brand variants and their consts from `Icon`. All existing provider, look-alike, size and forge tests pass unchanged in behaviour; every glyph renders the same (equivalent SVG: maud writes `<path …></path>` where the consts had `<path …/>`). Update `docs/design-system.md` (where providers are added, source and licence)

## Workflow follow-up

- Archive the change after review; check out-of-scope items against `product/iterations/roadmap.md` (both are "rejected", no backlog rows).
