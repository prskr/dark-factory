# Design

## Context

- `host_icon` (`crates/df-web/src/components/host_icon.rs`) picks a host's icon: project role (`ProjectHosts`) → `BRANDS` monogram → server icon (IP literal) → globe. `BRANDS` is a `(domain, monogram, tone)` table matched on the domain and its subdomains.
- `ProjectHosts::from_projects` (`components/project_host.rs`) registers the repository and issue tracker hosts of every project. A project's forge kind comes from `ForgeRoutes::forge_for_repo_url`, which is only `Some` for a loaded plugin and a project with a forge credential.
- `RoutingTable::resolve` (`df-core/src/routing.rs`) already returns `Resolved::base_url`: the route's `apiUrl`, or the repository origin when the route has none.
- `icons.rs` holds inline Lucide-style glyphs (stroke, `currentColor`) plus a filled Octocat (`Icon::GitHub`) and the multi-colour Forgejo logo.
- UI assets must not come from third-party hosts (`web-ui-shell`).

## Goals / Non-Goals

**Goals:**
- One precedence for every host icon (events table, graph, details panel, rules table): role → glyph → monogram → server → globe.
- The forge API host gets the forge role of the projects it serves.

**Non-Goals:**
- Multi-colour brand logos (Forgejo stays the only one): not in this change. Monochrome glyphs on the existing tone tile pass in both themes without per-brand colour pairs. The visual check (task 1.4) decides: if the glyphs look off, brand colours go to the backlog in `product/iterations/roadmap.md` before archive; otherwise rejected.
- Subdomain matching for project hosts and forge API hosts: rejected, unchanged from "Project host icons" (a subdomain of a self-hosted forge is not the forge).

## Decisions

### D1: Provider table in an embedded data file

The providers live in `crates/df-web/src/components/brands.json`, one object per provider: `name`, `domains`, `monogram`, `tone`, `path` (optional). `include_str!` embeds it and `serde_json` parses it once into a `LazyLock<Vec<Brand>>`; the `expect` on that parse is an invariant by construction (the file is compiled in and a unit test parses it). `tone` deserializes into the existing `badges::Tone`, so the status classes stay in Rust where Tailwind sees them. `host_icon` renders the glyph in the tone tile (text colour = tone foreground) when `path` is present, else the monogram. `data-host-icon` is `brand-<name>` for a glyph and the monogram for the fallback, so tests tell them apart.

Adding a provider is one JSON object, no Rust change. A unit test guards the file: it parses; names are unique; every domain is lower-case, normalized and appears once across all entries; every `path` is non-empty and contains only digits, SVG path command letters, `e`/`E`, space, `.`, `,` and `-`; the monogram has 1 to 3 characters; unknown fields are rejected (`deny_unknown_fields`). The provider test pins a path prefix per provider, so swapped paths fail.

Alternative: the table and one `Icon` variant per glyph in Rust (the first implementation). Rejected after it grew to 20 providers: each provider touched five places in two files and pushed `Icon::body` over the function-length limit. Alternative: SVG files with the table in Rust. Rejected: keeps the five places per provider.

### D2: Glyphs from Simple Icons, as path data

Each glyph is the `d` attribute of a single filled 24×24 path from Simple Icons (CC0, simple-icons@14.0.0), stored in the `path` field; the source and licence are noted once in `docs/design-system.md` and next to the `include_str!`. `host_icon` renders `<svg viewBox="0 0 24 24"><path fill="currentColor" stroke="none" d=…/></svg>` with maud escaping the path data, so no markup from the data file reaches the page unescaped. The forge icon of the GitHub integration renders the `github` entry's path from the same table, so the Octocat has one source; `Icon` keeps only the UI icons. If the `github` entry had no path, `brand_glyph` would fall back to the generic Git icon instead of an empty one; a unit test requires the path, so the fallback only guards against an edited data file.

Alternative: an icon crate or font. Rejected: a new dependency for path data that fits in one file.

All providers have a glyph, so the monogram fallback is exercised only by a unit test with a test entry. It stays because a provider added later may have no usable mark.

Provider domains map to the project or ecosystem whose packages they serve: `gcr.io`, `pkg.dev` → Google; `crates.io` → Rust; `ghcr.io`, `github.io` (GitHub Pages; the glyph names the operator, not the page owner) → GitHub; `goproxy.io` (a third-party Go module proxy) → Go; `amazonaws.com`, `ecr.aws`, `awsstatic.com`, `aws.amazon.com` (not `amazon.com`, the retail site) and `cloudfront.net` → AWS. For `cloudfront.net` the glyph names the CDN operator, not the owner of the content it serves. `gitlab.com` gets the GitLab glyph only as a provider; without a GitLab forge integration a GitLab project's repository host keeps the generic Git icon (project role wins). `codeberg.org` gets the Codeberg glyph as a provider; a Codeberg project served by the forgejo integration keeps the Forgejo forge icon on `codeberg.org` (project role wins).

### D3: Forge API host from the resolved route

`ForgeRoutes` gets `api_host_for_repo_url(&str) -> Option<String>`: resolve the URL, take the normalized host of `Resolved::base_url`, return it only when it differs from the repository host. `ProjectHosts::from_projects` registers it with `HostRole::Forge(kind)` for every project whose `project_forge_kind` is `Some`; projects without a served integration add nothing, so an unrelated `api.github.com` keeps the provider glyph. Strength resolution is unchanged (forge with kind wins).

Alternative: register every route's `apiUrl` host from the routing table regardless of projects. Rejected: icons on this page mean "used by a project", and the spec keeps that ("Forge API host without a served project").

## Risks / Trade-offs

- [Trademark use of brand marks] → used only to identify the destination, as with Forgejo; source and licence noted next to each path.
- [`apiUrl` with a port or path (`https://ghe.example.com:8443/api/v3`)] → host is normalized like project hosts (case, trailing dot, port ignored); a test covers path and port.
- [Glyph contrast on tone tiles] → glyphs use the tone foreground token, the same colour the monogram letters already pass with.

## Migration Plan

UI only, no data or config change. Rollback is a revert.
