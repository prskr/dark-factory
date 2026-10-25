# Tasks

In every task, write the failing test first, then the code. Database-backed tests of `df-web` need a container socket. List the ones that could not run in your report.

## 1. Host roles of projects

- [ ] 1.1 Add `HostRole`, `project_roles` and `role_icon` (design D1) in `crates/df-web/src/components/`. Verify with unit tests:
  - repository host → `Forge(kind)`;
  - a project without a forge credential → `Forge(None)`;
  - a tracker on the repository host → `Forge(kind)` when `kind` is `Some`, and `IssueTracker` when it is `None` (the form's existing rule);
  - a tracker on another host → `IssueTracker`;
  - an unparsable URL → no entry.
- [ ] 1.2 Switch `config_pages.rs` (repository and issue field icons, project list icon) to `role_icon` and `project_roles`, and remove the inline issue span. The form keeps its `served` kind and the list keeps `project_forge_kind`. Verify that the existing project-page tests (`configuration-management` forge icon scenarios) pass unchanged, and add a form test showing that a tracker on the repository host of an unserved repository shows the issue icon.
- [ ] 1.3 Add `ProjectHosts::from_projects` (design D2: normalized keys, stronger role wins on conflict). Verify with unit tests:
  - lookup ignores case, a trailing dot and IPv6 brackets;
  - `Forge(Some)` beats `Forge(None)`, and `Forge` beats `IssueTracker`, in both project orders;
  - subdomains and `*.` entries do not match.

## 2. Host icon with project hosts

- [ ] 2.1 Extend `host_icon` with `&ProjectHosts` (precedence project role > `BRANDS` > IP > globe, tile frame, `data-host-icon` values from design D2) and update all callers. Verify with unit tests:
  - a project host shows the Forgejo, GitHub and Git icons;
  - an issue host shows the issue icon;
  - `github.com` as a project host shows the GitHub icon while `api.github.com` keeps `GH`;
  - the existing `host_icon` tests still pass with an empty lookup.
- [ ] 2.2 Document the precedence and the role icons under `host_icon` in `docs/design-system.md`. Verify that the docs test listing components passes.

## 3. Egress pages

- [ ] 3.1 Build `ProjectHosts` in the `/egress/log/events` handler and pass it through `Viewer` to the rows, graph nodes and details JSON. If `list_projects` fails, log a `warn!` and use an empty lookup (design D3). Verify the `egress-log` scenarios "Self-hosted forge", "Project without forge credential", "Issue tracker on its own host", "Issue tracker on the repository host", "Subdomain of a project host", "Project host before provider monogram" and "Project created while the page is open" in rendered-stream tests with seeded projects.
- [ ] 3.2 Build `ProjectHosts` per request on `/egress/allowlist`, with the same fallback. Verify the `egress-allowlist` scenarios "Known host", "Project host" and "Wildcard entry over a project host".
- [ ] 3.3 Add a project with a self-hosted forge URL and a separate tracker host to the `--demo-flows` seed data or the `just serve-local` docs, so the icons can be checked locally. Verify that `just serve-local` shows the Forgejo and issue icons on `/egress/log` and `/egress/allowlist`.

## 4. Integration checks

- [ ] 4.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test -p df-web` (with a container socket) and `openspec validate egress-project-host-icons --strict`. Verify that all three pass.

## Workflow follow-up

- Archive `network-management-ui-polish` before this change; this change modifies its "Rules table" requirement.
- Archive this change after review.
