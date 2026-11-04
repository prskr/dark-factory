# Tasks

Prerequisite: change `release-versioning` is merged (release workflow, `profile_menu(display_name, version)`, `docs/releasing.md`). No container-backed tests are needed.

## 1. Tooling and skeleton

- [x] 1.1 Add Hugo extended to `cargo xtask update-tools` (design D2) with a test for the asset names per platform. Regenerate `multitool.lock.json`. Verify with `cargo test` in `tools/xtask` and `build/with_tools.sh hugo version` (prints `+extended`).
- [x] 1.2 Create the Hugo project in `docs/` (design D1): `config/_default/`, `config/landing/`, `go.mod`, vendored Hextra in `_vendor/`, empty `content/` and `landing/` home pages. Add `just docs-serve`, `just docs-build` and `just docs-theme-update`. Exclude `docs/_vendor/` from `typos`, `kingfisher` and `check-file-length`. Verify:
  - `just docs-build` succeeds without Go on `PATH`;
  - the landing build's `index.html` comes from `landing/_index.md`, and the docs build's from `content/_index.md`;
  - `lefthook run pre-commit --all-files` passes.
- [x] 1.3 Mount the three crate configuration references (design D3) and add their front matter. Verify:
  - the built site has `reference/sandbox-runtime/configuration/` with the variables table;
  - `cargo test -p factory-agent` and the `df-k8s` render tests still pass.

  If Hugo refuses the mounts, implement the copy fallback and record it in design D3.

## 2. Content

- [x] 2.1 Move `docs/logging.md`, `docs/telemetry.md` and `docs/keycloak.md` into `docs/content/operations/` with front matter, dropping the leading `# H1`. Update every link to them (`AGENTS.md`, `README.md`, crate docs, source comments, `docs/BUILD.bazel` if referenced). Verify with `grep -rn "docs/\(logging\|telemetry\|keycloak\).md"` (no matches outside `openspec/changes/archive`) and a passing `just docs-build`.
- [x] 2.2 Write the install guide `docs/content/install.md` for the scenario of the `Published documentation` requirement: prerequisites, PostgreSQL roles, a Keycloak client, Helm values, `helm install … --version`, first login. Verify by following it once on the homelab cluster, or by `helm template` against the published chart if no cluster is available, and say which was done.
- [x] 2.3 Write the landing page `docs/landing/_index.md` (hero with pre-MVP notice, feature cards, who it is for, who it is not for, links), with copy drafted from `product/context.md` and the ADRs. Verify by a review from the user and the `Landing page` scenario checked in the built output of the landing build (not the docs build).

## 3. Theme, links and search

- [x] 3.1 Write `custom.css` and the font mounts (design D6), and configure the theme toggle with `system` as default. Verify:
  - a browser check of light and dark at 1280 px, and at 390 px if a browser with viewport control is available;
  - link contrast ≥ 4.5:1 in both themes, measured and noted in the PR.
- [x] 3.2 Write failing checks, then add the link render hook (design D7). The fixture pages:
  - a broken relative link must fail `just docs-build` with the link in the message;
  - an absolute `https://www.dark-factory.eu/…` link must fail;
  - a link to a file under `static/` must pass.

  Replace links from published pages to unpublished files with GitHub URLs. Verify with `just docs-build` (passes) and the fixtures (as listed).
- [x] 3.3 Enable Hextra's built-in search. Verify that searching `OTEL_METRICS_EXPORTER` in a local build returns the telemetry page.

## 4. Versioning and publishing

- [x] 4.1 Write `docs/publish_test.sh` first, covering the `Publishing per source`, `Versions index` and `Version URL layout` scenarios against a temporary directory. Then implement `docs/publish.sh` (design D4). Verify with `docs/publish_test.sh` and `shellcheck`.
- [x] 4.2 Implement `versions.js`, the switcher placeholder, the banner and the root `404.html` script (design D5), with JS in `assets/js/`, not inline. Verify in a local build: serve a directory assembled by `publish.sh` (`main`, then releases `2026.11.0` and `2026.11.1`) and check the `Version switcher` and `Outdated version banner` scenarios in a browser.
- [x] 4.3 Add `.github/workflows/docs.yaml`:
  - on PRs that touch `docs/**`, `crates/*/docs/**`, `crates/df-web/assets/fonts/**`, `multitool.lock.json` or `.github/workflows/docs.yaml`: `just docs-build`;
  - on push to `main`: build, check out `gh-pages`, `publish.sh main`, commit and push through `docs/push.sh` (push retry), with `concurrency: gh-pages-main`.

  Verify that the PR job runs on this change's PR.
- [x] 4.4 Add a `docs` job to `release.yaml` after `publish` (the tag exists): check out the release SHA, build the slots `<version>` and `latest`, run `publish.sh release <version>`, and push through `docs/push.sh` (push retry), with `concurrency: gh-pages-release`. Verify with the first release after merge (6.2).

## 5. Web UI

- [x] 5.1 Write failing `df-web` tests for the `Docs link in the profile menu` scenarios:
  - the `2026.11.0` URL, and the `dev` URL;
  - `target="_blank"`, `rel="noopener"`;
  - the accessible name.

  Then add the entry and the `external-link` icon (design D8), after updating the Penpot profile menu board. Verify with `cargo test -p df-web`.
- [x] 5.2 Update `docs/design-system.md` (profile menu entries). Verify that it matches the Penpot board.

## 6. Integration

- [ ] 6.1 After merge, verify `https://www.dark-factory.eu/`:
  - the landing page shows;
  - `http://dark-factory.eu/` redirects;
  - `/docs/` forwards to `/docs/dev/` with the dev banner.
- [ ] 6.2 Run a release. Verify:
  - `/docs/<version>/` and `/docs/latest/` exist;
  - `/docs/` forwards to `latest/`;
  - the switcher lists `dev` and the version;
  - the Docs entry of an instance running that release opens its docs.
- [x] 6.3 Add the tech-debt entry for pruning old doc versions to `docs/tech_debt.md`. Add backlog rows to `product/iterations/roadmap.md` for PR previews, external link checking and docs served by the control plane. Verify with `openspec validate docs-site --strict`.

## Workflow follow-up

- Archive after review; archive `release-versioning` first.
