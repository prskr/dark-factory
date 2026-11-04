# Proposal: docs site

## Why

Dark Factory has no public page that says what it is, and its user documentation is spread over `docs/`, crate directories and contributor notes, readable only on GitHub at the state of `main`. An operator running release `2026.11.0` needs the docs of that release, not of `main`. Change `release-versioning` (prerequisite) introduces the releases to version the docs against.

## What Changes

- **Website at `https://www.dark-factory.eu`**, built with Hugo and the Hextra theme from `docs/`, hosted on GitHub Pages from the `gh-pages` branch. The apex `dark-factory.eu` redirects to `www`.
- **Landing page** at `/`, built from `main`: what Dark Factory is, a pre-MVP notice, what it does, who it is for and who it is not for, links to the docs and the repository.
- **User docs** under `/docs/`:
  - `logging.md`, `telemetry.md` and `keycloak.md`, moved into the site;
  - the configuration references of `factory-agent`, `df-agent-channel` and `df-plugins`, included from their crates;
  - a new install guide for an existing cluster with Kata, external PostgreSQL and Keycloak, installing the chart from OCI.

  Contributor docs (`development.md`, `design-system.md`, `design-workflow.md`, `tech_debt.md`, `demo.md`, `remote-execution.md`, ADRs, `deploy/README.md`) stay where they are and are not published.
- **Versioned docs**:
  - `/docs/dev/` from `main`;
  - `/docs/<version>/` per release, frozen;
  - `/docs/latest/` as a copy of the newest release;
  - `/docs/` forwarding to `latest`, or to `dev` before the first release.

  A version switcher keeps the current page, and a banner marks outdated and unreleased docs. Both read `/versions.json`, which a release rewrites, at runtime.
- **Theme** follows the app's design tokens: accent, neutrals, fonts, light and dark. Hextra's theme toggle stays and defaults to the system setting. Search is Hextra's built-in client-side search.
- **Tooling**: Hugo (extended) is pinned in `multitool.lock.json`, and Hextra is a Hugo module vendored into `docs/_vendor/`. A PR check builds the site and fails on broken internal links.
- **Docs entry in the profile menu** of the web UI, linking to the docs of the running version in a new tab.

## Capabilities

### New Capabilities

- `docs-site`: what the website publishes and where (landing page, user docs, domain), how it looks (theme, toggle, search), and the checks that keep it buildable.
- `docs-versioning`: the per-version URL layout, which builds a push to `main` and a release publish, the version switcher and the outdated-version banner.

### Modified Capabilities

- `web-ui-shell`: the profile menu gets a Docs entry pointing at the running version's docs.

## Out of scope

- Preview deployments for pull requests: backlog (needs a host with per-branch deploys, such as Cloudflare Pages).
- Pruning old doc versions: backlog, tracked in `docs/tech_debt.md`, revisit at about 500 MB.
- External link checking: backlog.
- Publishing ADRs, design documents or contributor docs: rejected; the site is for operators and users.
- Translations: rejected.
- A self-hosted docs copy served by the control plane for air-gapped instances: backlog.

## Impact

- `docs/` becomes a Hugo site:
  - new: `hugo.yaml`, `go.mod`, `_vendor/`, `content/`, `assets/`, `layouts/`, `static/`;
  - `logging.md`, `telemetry.md` and `keycloak.md` move into `content/`, so links to them in `AGENTS.md`, `README.md`, crate docs and source comments are updated.
- `multitool.lock.json` and `tools/xtask` (Hugo entry), `Justfile` (serve, build and theme-update recipes)
- `.github/workflows/`: new `docs.yaml` (PR check and deploy from `main`); `release.yaml` gains a docs job.
- `crates/df-web`: Docs entry in `profile_menu`.
- External: DNS records for `dark-factory.eu`; GitHub Pages settings (one-time steps in `docs/releasing.md`).
