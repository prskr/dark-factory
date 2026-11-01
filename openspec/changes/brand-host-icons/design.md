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

### D1: Optional glyph per `BRANDS` entry

`BRANDS` gains an `Option<Icon>`: `(domain, monogram, tone, glyph)`. `host_icon` renders the glyph in the tone tile (text colour = tone foreground, `fill="currentColor"`) when present, else the monogram. `data-host-icon` becomes `brand-<name>` for a glyph (`brand-github`, `brand-anthropic`) and stays the monogram for the fallback, so tests tell them apart.

Alternative: a separate glyph table keyed by domain. Rejected: two tables that must stay in sync for one concept.

### D2: Glyphs from Simple Icons, inlined

New `Icon` variants `Anthropic`, `Npm`, `Go`, `Python`, `Google`, each a single filled 24×24 path from Simple Icons (CC0), with the source noted in a doc comment as for Forgejo. GitHub reuses the existing `Icon::GitHub` path. Filled glyphs need `fill="currentColor" stroke="none"` on the path, as `Icon::GitHub` already does; `icon()` keeps its stroke defaults.

Alternative: an icon crate or font. Rejected: a new dependency for six paths.

All six providers get a glyph, so the monogram fallback is currently exercised only by a unit test with a test entry. It stays because a provider added later may have no usable mark.

### D3: Forge API host from the resolved route

`ForgeRoutes` gets `api_host_for_repo_url(&str) -> Option<String>`: resolve the URL, take the normalized host of `Resolved::base_url`, return it only when it differs from the repository host. `ProjectHosts::from_projects` registers it with `HostRole::Forge(kind)` for every project whose `project_forge_kind` is `Some`; projects without a served integration add nothing, so an unrelated `api.github.com` keeps the provider glyph. Strength resolution is unchanged (forge with kind wins).

Alternative: register every route's `apiUrl` host from the routing table regardless of projects. Rejected: icons on this page mean "used by a project", and the spec keeps that ("Forge API host without a served project").

## Risks / Trade-offs

- [Trademark use of brand marks] → used only to identify the destination, as with Forgejo; source and licence noted next to each path.
- [`apiUrl` with a port or path (`https://ghe.example.com:8443/api/v3`)] → host is normalized like project hosts (case, trailing dot, port ignored); a test covers path and port.
- [Glyph contrast on tone tiles] → glyphs use the tone foreground token, the same colour the monogram letters already pass with.

## Migration Plan

UI only, no data or config change. Rollback is a revert.
