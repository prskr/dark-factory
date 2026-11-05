# Proposal

## Why

The review loop on Forgejo is broken. `read_pull_request_feedback` returned each PR comment up to 20 times. It failed with "the forge rejected the credentials" as soon as a human commented, although the token had all scopes it needs. Agents also rarely pass `since`, because they have no timestamp to pass. The plugin host and forge plugin follow-ups of issue #125 (TD-40, TD-80, TD-95, TD-96) touch the same code and are fixed in the same change.

## What Changes

- **Forgejo: PR comments read once.** Forgejo's `GET /repos/{o}/{r}/issues/{n}/comments` ignores `page` and `limit` and always returns all comments. The plugin read it with the page loop, got the full list on every page up to the limit of 20 pages and set `truncated` every time. It now reads the endpoint with a single request.
- **Author role lookup no longer fails the call.** Forgejo answers the permission lookup (`GET .../collaborators/{login}/permission`) with 403 unless the bot account is site or repository admin. A refused lookup now yields role `outside` and a warning in the control plane log that names the missing access (the plugin writes it to stderr). This applies to both forge plugins. **BREAKING** for the spec: `list-feedback` no longer fails with `unauthorized` without repository admin.
- **Forge errors name what failed.** `unauthorized` errors carry the request and the forge's message. The tool error shows them instead of a fixed text that blames the credential.
- **Feedback newest first, with a cursor.** `read_pull_request_feedback` lists comments, reviews and threads newest activity first and returns `next_since`: the newest activity timestamp in the result minus a 5-minute overlap, so items created while the plugin was still listing are not skipped. `plugins.<name>.callTimeoutSeconds` gets a maximum of 300 s to keep that overlap sufficient. The tool description tells the agent to pass the `next_since` of its previous read as `since`. **BREAKING** for the tool output (order, new field).
- **Review `updated-at` (TD-95).** WIT `review`, core `Review` and both plugin conversions gain `updated-at`. Ranking and `next_since` use it. **BREAKING** WIT record change.
- **Bounded review comment reads on Forgejo (TD-96).** Forgejo has no per-thread listing and doesn't bump a review's `updated_at` when a comment is added. `list-feedback` therefore reads the comments of at most a fixed number of the newest reviews and sets `truncated` when it skips older ones.
- **Shared plugin code (TD-80).** New crate `plugins/forge-common` holds the title prefix logic, timestamp parsing, the `since` filter and the stderr warning, which both plugins currently copy.
- **Import allow-list checked against the contract (TD-40).** A test resolves WIT world `plugin-base` and fails if an import is missing from `ALLOWED_IMPORTS`.
- **Docs: bot account access.** The forge configuration reference gets a table per forge: access level of the bot account × what works (PR create and update, comments, feedback, author roles, webhook wake policy `members`), the exact token scopes, and what happens without repository admin.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `forgejo-integration`: PR comments read without paging; a refused author role lookup yields `outside`; at most N reviews' comments read per call; reviews carry `updated-at`.
- `github-integration`: a refused author role lookup yields `outside`; reviews carry `updated-at`.
- `factory-tools`: feedback newest first, `next_since`, ranking by review `updated-at`, forge errors show the forge's message.
- `forge-webhooks`: wake policy `members` treats an author whose role can't be looked up as not a member, instead of failing the event.
- `plugin-host`: the import allow-list matches world `plugin-base` (checked by a test); the plugin call timeout has a maximum of 300 s.

## Out of scope

- A role lookup on Forgejo that works without repository admin. `GET .../collaborators/{login}` only needs read access, but answers 404 for the repository owner and members of the owning organisation. It needs a Forgejo API change: backlog item "Forgejo author role lookup without repository admin" in `product/iterations/roadmap.md`.
- A separate `forbidden` error variant for 403 (account lacks permission, as opposed to 401 invalid credential): rejected for now. The forge message in the error text tells the two apart.
- Pagination or a backward cursor (`before`) for `read_pull_request_feedback`: rejected. Items the size budget drops are not returned by later incremental reads; `truncated` says so, and older feedback is usually superseded by newer.

## Impact

- Code: `plugins/forgejo/src/api/feedback.rs`, `plugins/github/src/api/feedback.rs`, both plugins' `draft.rs`, `timestamp.rs`, `wasi_transport.rs`; new crate `plugins/forge-common`; `contract/wit/integrations/integrations.wit`; `crates/df-core` (`Review`), `crates/df-plugins` (`convert.rs`, `host.rs`); `crates/df-agent-channel/src/tools/feedback.rs`, `tools.rs` (`forge_error`); `contract/mcp/factory-tools.v2.json`.
- Config: `crates/df-core/src/config.rs` and `deploy/helm/dark-factory/values.schema.json` (`callTimeoutSeconds` maximum 300).
- Docs: `crates/df-plugins/docs/configuration.md`, `deploy/helm/dark-factory/README.md`, `docs/content/integrations/forges/_index.md`, `docs/content/integrations/forges/webhooks.md`, plugin READMEs; `docs/tech_debt.md` loses TD-40, TD-80, TD-95, TD-96.
- Plugins must be rebuilt: the WIT `review` record changes.
