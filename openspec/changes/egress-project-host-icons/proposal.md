# Proposal

## Why

Most traffic from a sandbox goes to its project's forge and issue tracker. Self-hosted forges, such as a private Forgejo, are not in the built-in provider list, so on the network traffic and egress rules pages they show the generic globe. The UI already knows these hosts from the project catalog and already chooses forge and issue-tracker icons for them on the project pages.

## What Changes

- A host that is the repository host of a project shows that project's forge icon: the Forgejo logo, the GitHub icon, or the generic Git icon for another plugin or a project without a forge credential. These are the same rules the project list uses.
- A host that is a project's issue tracker host shows the forge icon when it is also the project's repository host. Otherwise it shows the generic issue-tracker icon, as in the project form.
- Precedence: project host, then the provider monogram, then the IP server icon, then the globe. A project host on `github.com` therefore shows the GitHub icon instead of the `GH` monogram.
- Project hosts match exactly: case-insensitive, trailing dot and port ignored, no subdomains.
- This applies to every use of the shared host icon: the events table, graph nodes, the details panel and the egress rules table.
- The host-to-project-role lookup is built from the project catalog: once per network traffic stream connection, and per request on the rules page. A new project's icon appears after a page reload.
- The project form and the egress pages share one rule that maps a project URL to its role. A later issue-tracker integration can then add a tracker kind with an icon in one place. No tracker kind is introduced now.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `egress-log`: new requirement "Project host icons" for destinations of the events table, graph and details panel.
- `egress-allowlist`: "Rules table" host icons follow the same precedence, project hosts first.

## Impact

- `crates/df-web`:
  - `components/host_icon.rs` takes the project host lookup.
  - The rendering of `egress/log` (events, graph, details) and `egress/allowlist.rs` changes.
  - `config_pages.rs` moves its URL-to-icon rule (`forge_feedback`, `project_forge_kind`) to a shared helper.
- The `egress/log/events` handler and the rules page read `df_db::catalog::list_projects` (one query per connection or request). No schema or migration change.
- Depends on `network-management-ui-polish`: its `egress-allowlist` "Rules table" and `host_icon` must be archived first.
