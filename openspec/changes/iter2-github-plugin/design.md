# Design

## Context

After `iter2-plugin-distribution` and `iter2-multi-forge`, plugins ship as OCI images mounted at `/plugins/<name>` (`plugin-forgejo`), share one `PluginEngine`, and the chart renders `plugins.<name>` entries with image, settings, `hosts` and `authScheme`. Every forge call is routed by the host of the project's repository URL. A route can set `apiUrl`, which becomes `base-url` and the only allowed authority, and the plugin's `authScheme`.

The Forgejo plugin (`plugins/forgejo`) is the template: its logic is generic over a `Transport` and unit-tested natively, `wasi_transport.rs` maps it onto `wasi:http`, and `draft.rs` emulates drafts with title prefixes. GitHub differs where it matters:
- its REST API lives on its own host and requires `User-Agent`, `Accept` and `X-GitHub-Api-Version`;
- drafts exist natively, but not in private repositories on GitHub Free and Pro, and the REST API cannot change the draft state;
- rate limits arrive as 403 or 429 with `x-ratelimit-*` or `retry-after`, and duplicate pull requests as 422.

The host retries `rate-limited` after at most 30 s, up to 3 times (`crates/df-plugins/src/retry.rs`). Motivation: see proposal.md.

## Goals / Non-Goals

**Goals:**
- The pull request lifecycle (draft at the first push, `find-open` for idempotency, ready plus summary comment at completion) and the agent tools work for projects on github.com and GitHub Enterprise Server, including private repositories on GitHub Free.
- The host stays plugin-agnostic apart from the retry rule (D6); the plugin never sees the token.

**Non-Goals:**
- GitHub App authentication: installation tokens need minting by the host (JWT signed with the app key, token cache) and an ADR (backlog).
- Webhooks (ADR-0015, iteration 3); `merge`, branches, checks and `list-open` (ADR-0033, ADR-0034, iteration 3).
- Detecting draft support automatically (rejected, D2); a crate shared by the Forgejo and GitHub plugins (backlog, D4).

## Decisions

### D1 — REST, GraphQL only for the draft state
All operations use the REST API. Only the native draft state changes through GraphQL (`markPullRequestReadyForReview`, `convertPullRequestToDraft`, with the pull request's `node_id` from the REST response), because `PATCH /repos/{o}/{r}/pulls/{n}` has no draft field. GraphQL answers HTTP 200 even on failure, so the plugin checks `errors` (D5). Rejected alternatives: everything in GraphQL (a second data model for every call); title prefixes only (no real drafts where GitHub has them).

### D2 — Draft modes
| | `title-prefix` (default) | `native` |
|---|---|---|
| Create a draft | title `"<first prefix> <title>"`, no draft flag | `draft: true`, title unchanged |
| Read | draft = native flag or a known prefix; title without prefix | same |
| `draft: false` | remove both forms: GraphQL ready if natively draft, title without prefix | same |
| `draft: true` on a ready pull request | add the prefix | GraphQL convert to draft |
| New title, state unchanged | keeps the current form | same |

`title-prefix` is the default because it works in every repository, including private repositories on GitHub Free and Pro, which have no draft pull requests. `native` is for public repositories, GitHub Team, Enterprise Cloud and Enterprise Server. Since reading recognizes both forms, operators can switch the mode while drafts are open, for example after making a repository public or moving to GitHub Team. In `native` mode GitHub's 422 "Draft pull requests are not supported" becomes `invalid-input` naming `draft-mode: title-prefix`: the control plane only logs failed PR creations as warnings, so that message is the operator's pointer. `update` reads the pull request first when title or draft changes, like Forgejo; after a GraphQL change without a title or body change it reads it again to return the final state. Rejected alternatives: automatic fallback on that 422 (depends on GitHub's message text, a failed request per pull request, hidden behaviour); `native` only (no pull requests in private Free repositories).

### D3 — `find-open` with server-side filters
`GET /repos/{o}/{r}/pulls?state=open&head={o}:{branch}&base={base}&per_page=100`, then the first entry whose `head.ref` and `base.ref` match exactly. GitHub allows one open pull request per head and base, so one request suffices. Query values are percent-encoded because branches contain `/`. Rejected alternative: Forgejo's approach of listing all open pull requests page by page (one request per 100 open pull requests, rate-limit budget).

### D4 — Transport, base URL and headers
`base-url` keeps its path (GHES: `/api/v3`), and REST paths are appended to it. GraphQL goes to `/api/graphql` on the same host when the path ends with `/api/v3`, else to `<base-url>/graphql` (github.com, GHE.com).

Every request carries:
- `User-Agent: dark-factory-github-plugin/<crate version>`;
- `Accept: application/vnd.github+json`;
- `X-GitHub-Api-Version` from the setting `api-version`, default `2026-03-10`, the current version on github.com. `2022-11-28` is supported until 2028-03-10 and is the one older GHES releases know;
- `Content-Type: application/json` when the request has a body.

The transport returns status, `retry-after`, `x-ratelimit-remaining`, `x-ratelimit-reset` and body. `wasi:http` does not follow redirects, so a renamed repository answers 301 and fails as `upstream` until the project's URL is updated. A `base-url` of `https://github.com` is the likely mistake of a route without `apiUrl` and fails with a `config` error that names the fix.

`wasi_transport.rs` and the prefix helpers of `draft.rs` are copied from the Forgejo plugin and adapted. Each plugin generates its own WIT bindings, so a shared crate would need its own binding layer; that pays off with a third plugin.

### D5 — Error and rate-limit mapping
The mapping is specified in `github-integration` ("Error mapping", "Rate limits"). Rate limits are checked first: 403 or 429 with `x-ratelimit-remaining: 0`, a `retry-after` header or a message naming a rate limit. The status table applies after that. 422 messages come from `message` and `errors[].message`, and "already exists" there means `conflict`. A 404 message adds that GitHub also answers 404 when the token cannot see a private repository. The wait until the reset uses the wall clock (`wasi:clocks/wall-clock`, part of `plugin-base`).

### D6 — Host: no early retries
`with_retries` returns a `rate-limited` error at once when its retry-after exceeds `RETRY_AFTER_CAP` (30 s); shorter or missing hints behave as before. GitHub asks clients not to retry before the reset and may block integrations that do. Forgejo's hints are short and unaffected. Callers already handle the error: PR creation is retried at the next push report or run end, tool calls report it to the agent, and marking ready at run end logs a warning. Rejected alternatives: raising the cap (a call would block for up to an hour, longer than the callers wait, e.g. `FORGE_TIMEOUT` of 60 s); waiting inside the plugin (bounded by the call timeout).

### D7 — Credentials and Git
The token is a fine-grained personal access token restricted to the project's repositories, referenced like the Forgejo token (credential reference → Secret). It needs Pull requests read/write, Issues read/write, Contents read/write (Git clone and push) and Metadata read; a classic token needs `repo`. The route sets `authScheme: bearer`; GitHub's REST API also accepts `token`, its GraphQL documentation uses `bearer`.

Git in iteration 1: the credential helper sends `agent.gitUsername` and the token. GitHub is expected to ignore the user name for tokens; task 7.1 verifies it. The instance egress allowlist needs `github.com` until `iter2-git-proxy` sends the same pair as Basic auth from the gateway; that change's spike 1.2 should also run against github.com.

### D8 — Packaging and default release
`plugins/github` mirrors `plugins/forgejo`: `cdylib` + `rlib`, `rust_shared_library` for `wasm32-wasip2`. Its optimized build (`//deploy:github_plugin`) becomes the plugin image `plugin-github` exactly like `plugin-forgejo` (`iter2-plugin-distribution` D1: `scratch`, `/plugin.wasm`, an index for `linux/amd64` and `linux/arm64`, pushed with the other images, its digest passed by the dev deploy). Default chart values next to the unchanged `forgejo` entry:
```yaml
plugins:
  github:
    image:
      repository: ghcr.io/prskr/dark-factory/plugin-github
      tag: ''          # the chart's appVersion
      digest: ''
    authScheme: bearer
    hosts:
      - host: github.com
        apiUrl: https://api.github.com
    settings: {}       # draft-mode: title-prefix (default) | native; api-version; wip-prefixes
```
Operators add GHES routes to `hosts` or drop the entry with `plugins.github: null`. The plugin name `github` maps to the GitHub icon (`ForgeKind::from_plugin`). Rejected alternative: an opt-in entry, which would send projects on github.com to the Forgejo default plugin, where they fail with confusing errors.

### D9 — Tests
Unit tests with a fake transport cover every scenario of `github-integration`, as in the Forgejo plugin. `df-plugins` integration tests run the built component against `wiremock`, first with `base-url` from a repository URL on the mock, after `iter2-multi-forge` through a route whose `apiUrl` is the mock, with `authScheme: bearer`; the `image_component` test loads the optimized component of the plugin image. A live test (`#[ignore]`, like `tests/homelab.rs`) runs the lifecycle against scratch repositories on github.com. GitHub cannot be part of the hermetic e2e run (`iter1-demo-e2e` runs Forgejo in kind).

### D10 — Order of work
The plugin reads `base-url` and gets the `Authorization` header injected; where both come from does not matter to it. Crate, plugin, the host's retry change and the plugin image therefore build on `main` and are implemented in parallel to `iter2-multi-forge`. Their integration tests take `base-url` from a repository URL on the mock and use today's `token` scheme, which GitHub accepts for personal access tokens. Routes, the default entry, the dev configuration and the runs against github.com follow `iter2-multi-forge`; this change is archived after it. Rejected alternative: a temporary setting for the single forge plugin plus an API URL, which `iter2-multi-forge` would remove again.

## Risks / Trade-offs

- [GitHub changes the text of the duplicate or "drafts not supported" errors] → status codes stay the main signal; recorded bodies in unit tests and the live test catch changes.
- [The pinned API version is closed down] → GitHub supports a version for 24 months after its successor, and `api-version` switches without a release.
- [In `title-prefix` mode the marker is part of the visible title and of notifications] → same as Forgejo; the README explains when to switch to `native`.
- [A token that can read but not write gets 403 on writes] → `unauthorized` with GitHub's message, which names the missing permission.
- [5,000 requests per hour per token] → the lifecycle needs a handful of requests per task; `find-open` is one request.
- [A draft's form changes outside Dark Factory] → reading recognizes both forms, so the next update still sees a draft.

## Migration Plan

No data migration. The default release gains `plugins.github`, one more image volume on the control plane pod: projects on `github.com`, which previously went to the Forgejo default plugin and failed, work once their token has the permissions of D7. Instances that do not want the plugin set `plugins.github: null`. Rollback: the previous release, whose chart has no `github` entry.
