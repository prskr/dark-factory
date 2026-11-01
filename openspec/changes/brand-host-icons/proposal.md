# Proposal

## Why

Egress destinations of well-known providers show letter badges (`GH`, `A`, `Py`) although recognizable logos exist, and a forge whose API lives on a separate host (`api.github.com`, a GitHub Enterprise `api.` host) shows the provider badge or the globe instead of the forge icon the repository host gets. Both make the network graph and the egress pages harder to read at a glance.

## What Changes

- Known providers get an SVG brand glyph (GitHub Octocat, Anthropic, npm, Go, Python, Google) inside the existing tone tile. The letter monogram stays as the fallback for a provider without a glyph; the server icon and the globe stay last.
- The host of a forge route's `apiUrl` gets the forge role of the projects served through that route, like their repository host. `api.github.com` of a served GitHub project then shows the GitHub forge icon instead of the provider badge.
- Precedence becomes: project host role (repository, forge API, issue tracker) → brand glyph → brand monogram → server icon (IP literal) → globe.
- **BREAKING (UI only):** the scenario "`api.github.com` keeps the `GH` monogram" no longer holds; unit tests on `data-host-icon="GH"` and other monograms change.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `egress-log`: "Project host icons" covers the forge API host; provider icons become glyph first, monogram as fallback.
- `egress-allowlist`: "Rules table host icons" follows the new precedence.

## Impact

- `crates/df-web/src/components/host_icon.rs` (`BRANDS`, `host_icon`), `components/icons.rs` (new brand glyphs), `components/project_host.rs` (`ProjectHosts::from_projects`).
- `crates/df-core/src/routing.rs`: `ForgeRoutes` exposes the API host of the route that serves a repository URL.
- Unit tests in `df-web` (`egress/tests/icons.rs`, `host_icon.rs`, allowlist page tests).
- No new dependencies; glyphs are inlined SVG paths (no third-party hosts, `web-ui-shell` "self-hosted assets").

Out of scope:

- Brand glyphs for project forge icons other than Forgejo and GitHub (plugins of kind `Other`): rejected, the plugin name is the only identity the UI has.
- Operator-configurable brand list: rejected, the built-in list is enough for the MVP.
