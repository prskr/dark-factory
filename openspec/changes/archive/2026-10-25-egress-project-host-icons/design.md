# Design

## Context

See `proposal.md` for the motivation. Current state after `network-management-ui-polish`:

- `components/host_icon.rs`: `host_icon(host, size)` draws a provider monogram (suffix match on `BRANDS`), the server icon for IP literals, or the globe. Callers:
  - `egress/log/rows.rs`: the events row, plus the details panel icon sent as JSON;
  - `egress/log/graph.rs`: destination nodes;
  - `egress/allowlist.rs`: the rules table.
- `components/icons.rs`: `forge_icon(Option<&ForgeKind>)` draws the Forgejo logo, the GitHub icon or the Git icon. `Icon::Issue` exists.
- `config_pages.rs` holds two separate rules for the same question:
  - `project_forge_kind`: the instance forge (`WebState::forge`) when the project has a forge credential, otherwise `None` (Git icon);
  - `forge_feedback`: the issue tracker URL shows the forge icon when its host equals the repository host, otherwise an inline `Icon::Issue` span.
- There is one forge integration per instance (`ForgeKind`). No issue tracker integration exists: a project only stores `issue_tracker_url`.
- `df_db::catalog::list_projects(pool)` returns every project with `repo_url`, `issue_tracker_url` and `forge_credential_id`.

## Goals / Non-Goals

**Goals:**
- One rule that maps a project URL to the role of its host, used by the project pages and the egress pages.
- A seam where a future tracker kind (a Jira or Forgejo issues integration) changes one enum variant and one render arm.

**Non-Goals:**
- A `TrackerKind` type, tracker icons or tracker configuration. They come with the issue tracker integration.
- Live updates of the lookup while a stream is open.
- Matching subdomains or wildcard entries of project hosts.

## Decisions

### D1. `HostRole` is the seam

Add a small module in `components/`, for example `project_host.rs`:

```rust
/// What a host is to the projects of this instance.
pub enum HostRole {
    /// The repository host; `None` when no forge integration serves the project.
    Forge(Option<ForgeKind>),
    /// An issue tracker on a host other than the repository's.
    IssueTracker,
}
```

- `fn project_roles(kind: Option<&ForgeKind>, repo_url: &str, issue_tracker_url: Option<&str>) -> Vec<(String, HostRole)>` holds the rule of `forge_feedback`. The caller passes the kind that serves the project: the project list and the egress pages use `project_forge_kind` (instance forge if the project has a forge credential); the form keeps its `served` condition (instance forge if the repository URL is valid).
  - The repository host gets `Forge(kind)`.
  - A tracker host gets `Forge(kind)` only when it equals the repository host and `kind` is `Some`. Otherwise it gets `IssueTracker`, so the form's issue field still shows the issue icon when no forge integration serves the repository.
- `fn role_icon(&HostRole) -> Markup` draws `forge_icon` or the issue icon.
- `config_pages.rs` uses `role_icon` for the repository field, the issue field and the project list, so the inline `Icon::Issue` span goes away. The form's icons do not change.

When the issue tracker integration arrives, `IssueTracker` becomes `IssueTracker(Option<TrackerKind>)`. `project_roles` then reads the project's tracker integration, and `role_icon` draws its icon. Callers and the lookup stay as they are.

*Alternative:* a `TrackerKind` enum with only `Generic` now. Rejected: it is a type with one value and no source. The enum variant already marks the place.

*Alternative:* `HostRole` in `df-core`. Rejected for now: only `df-web` uses it. Move it when the tracker integration needs it outside the UI.

### D2. `ProjectHosts` lookup, passed to `host_icon`

`ProjectHosts` is a `HashMap<String, HostRole>`, keyed by the normalized host. `ProjectHosts::from_projects(forge, &projects)` folds `project_roles` over all projects. On a conflict the stronger role wins:

- `Forge(Some)` over `Forge(None)`;
- `Forge` over `IssueTracker`.

A project without a forge integration whose tracker is on the repository host therefore shows the Git icon on the egress pages. The host is still the repository first. The form's issue field shows the issue icon, because the form uses the tracker entry's own role, not the lookup.

The order of projects therefore does not matter.

`host_icon(host, size)` becomes `host_icon(host, size, &ProjectHosts)`. It checks the lookup first, then `BRANDS`, then IP literals, then the globe. A project role renders in the same tile frame as the globe (`bg-sunk`, `size-4` glyph), so sizes and alignment stay the same. `data-host-icon` is `forge-forgejo`, `forge-github`, `forge-git` or `issue`, for tests.

Keys and lookups use the same normalization: `normalize_host`, then `strip_brackets`. A URL host never has a port, and an event host is matched without its port. A wildcard entry such as `*.git.example.internal` is looked up as written, so it never matches a project host (spec scenario).

*Alternative:* a global cache in `WebState` refreshed on project changes. Rejected: it needs invalidation on every project write, and one query per stream connection is cheap.

### D3. Where the lookup is built

- `/egress/log/events`: built once before the stream starts and moved into the `Viewer`. Rows, graph and details render with it. New projects appear after a reload or reconnect (spec scenario).
- `/egress/allowlist`: built per request next to the existing queries.
- If `list_projects` fails, log a warning and use an empty lookup. The icons fall back to the current behaviour and the stream or page still works. Only decoration is lost, so failing the page would be worse.

### D4. Security

The event host is influenced by the agent. It is only used as a lookup key against hosts the board configured, so the agent can at most pick which icon is shown. The lookup holds only hosts, no credentials or paths. Both pages are already restricted to board members.

## Risks / Trade-offs

- [A project on `github.com` turns `github.com` into the GitHub icon while `api.github.com` keeps `GH`] → Accepted and stated in the spec. Both identify GitHub.
- [The lookup goes stale during long streams] → Accepted (spec scenario). A reconnect or reload rebuilds it.
- [Many projects] → One query per connection, and the map has at most two entries per project.

## Migration Plan

None: no schema, configuration or API change. Archive after `network-management-ui-polish`, whose "Rules table" requirement this change modifies.
