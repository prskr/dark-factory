# GitHub plugin

WASI component implementing world `forge` of `contract/wit/integrations` (`pull-requests` and
`issues`) for github.com and GitHub Enterprise Server. Built for `wasm32-wasip2`; run by
`df-plugins`. The plugin never sees credentials: the host adds `Authorization`.

```bash
cargo build -p github-plugin --target wasm32-wasip2 --release   # target/wasm32-wasip2/release/github_plugin.wasm
wasm-tools component wit target/wasm32-wasip2/release/github_plugin.wasm
```

Exports: `pull-requests`, `issues`. Imports: `wasi:http`, `wasi:config`, `wasi:io`, `wasi:clocks`,
`wasi:cli` (pulled in by `std`) and `darkfactory:integrations/types`.

## Configuration (`wasi:config/store`)

`base-url` is set by the host from the route's `apiUrl`; all other keys come from `plugins.github.settings` in the instance configuration.

| Key | Required | Default | Effect |
|---|---|---|---|
| `base-url` | yes | none | API root; REST paths are appended to it. `https://api.github.com` for github.com, `https://<host>/api/v3` for GitHub Enterprise Server. Missing, without scheme or host, or the web host `github.com` (a route without `apiUrl`) → `config` error. |
| `draft-mode` | no | `native` | How new drafts are marked: `native`, `title-prefix` or `auto` (see below). Any other value → `config` error. |
| `wip-prefixes` | no | `WIP:,[WIP]` | Comma-separated work-in-progress title prefixes; the first one marks new drafts in `title-prefix` and `auto` mode, where an empty list is a `config` error. |
| `api-version` | no | `2026-03-10` | `X-GitHub-Api-Version`. Use `2022-11-28` for older GitHub Enterprise Server releases. |

GraphQL goes to `https://<host>/api/graphql` when `base-url` ends with `/api/v3`, else to
`<base-url>/graphql`.

Every request sends `User-Agent: dark-factory-github-plugin/<version>`,
`Accept: application/vnd.github+json`, `X-GitHub-Api-Version` and, with a body,
`Content-Type: application/json`. Redirects are not followed: a renamed repository answers 301 and
fails as `upstream` until the project's URL is updated.

## API calls

| Operation | GitHub endpoint |
|---|---|
| `create` | `POST /repos/{owner}/{repo}/pulls` (`draft: true` only in `native` and `auto` mode; in `auto` mode a second `POST` with the title prefix if GitHub has no drafts) |
| `find-open` | `GET .../pulls?state=open&head={owner}:{head}&base={base}&per_page=100`, then the entry whose head and base match exactly; one request, since GitHub allows one open pull request per head and base |
| `get` | `GET .../pulls/{number}` |
| `update` | `GET .../pulls/{number}` when title or draft change, GraphQL `markPullRequestReadyForReview` / `convertPullRequestToDraft` (with the pull request's `node_id`) for the native draft state, `PATCH .../pulls/{number}` for title and body; a final `GET` when only the native draft state changed |
| issues `get` | `GET .../issues/{number}` |
| issues `add-comment` | `POST .../issues/{number}/comments` (pull request numbers work as issue numbers) |
| `list-feedback` | one GraphQL query: `viewer.login`, and the pull request's `comments`, `reviews` and `reviewThreads` (each `last: 100`, the newest items); then `GET .../collaborators/{username}/permission` once per distinct author with association `OWNER`, `MEMBER` or `COLLABORATOR` |
| `reply` | GraphQL `node(id:)` for the thread's pull request, then `addPullRequestReviewThreadReply` |

`src/operations.rs` lists these REST calls by GitHub's `operationId`. The host tests
(`crates/df-plugins/tests/github.rs`) check every request and mocked response against
`tests/fixtures/github-openapi.json`, an extract of GitHub's OpenAPI description with only these
operations and the components they reference; a request to an operation missing from the list fails.
Mocked responses are checked without `required`, since they carry only the fields the plugin reads.
GraphQL has no description; the live test covers the two draft mutations, and the documents of
`list-feedback` and `reply` were validated against github.com with `gh api graphql` (2026-11-03).

| Task | Command |
|---|---|
| Regenerate the extract after changing `src/operations.rs` | `cargo xtask update-github-spec` |
| Move to a newer upstream commit | `cargo xtask update-github-spec --commit <sha>` |
| Change the default `api-version` | update `DESCRIPTION_PATH`, then `cargo xtask update-github-spec --commit <sha>` (a run without `--commit` reuses the recorded file) |
| Fail if the extract is stale | `cargo xtask update-github-spec --check` |

The extract records its source in `x-dark-factory-source`: the commit of
`github/rest-api-description`, the file for the default `api-version`
(`descriptions-next/api.github.com/api.github.com.2026-03-10.json`, `DESCRIPTION_PATH` in
`tools/xtask/src/update_github_spec.rs`) and its sha256. A run without `--commit` fails when the
downloaded file no longer matches that sha256. The upstream repository has published no release since
v2.1.0 (2022), which predates API version 2026-03-10, so the pin is a commit.

## Draft modes

| | `native` (default) | `title-prefix` | `auto` |
|---|---|---|---|
| Create a draft | `draft: true`, title unchanged | title `"<first prefix> <title>"` | `draft: true`; on 422 "not supported" once more like `title-prefix`, with a warning |
| Read | draft = native flag or a known prefix (case-insensitive); title without prefix | same | same |
| `draft: false` | remove both forms: GraphQL ready if natively draft, title without prefix | same | same |
| `draft: true` on a ready pull request | GraphQL convert to draft | add the prefix | add the prefix |
| New title, state unchanged | keeps the current form | same | same |

`native` is the default: GitHub has native drafts, also in private repositories on GitHub Free
(verified on 2026-10-30). Where a repository or GitHub Enterprise Server instance rejects them,
GitHub's 422 "Draft pull requests are not supported" becomes `invalid-input` naming
`draft-mode: title-prefix`. Choose `title-prefix` there, or for a WIP marker in the title like the
Forgejo plugin's; it never sends a request that fails.

`auto` is for setups where some repositories have drafts and some don't. It creates native drafts;
where GitHub answers that 422, the plugin sends the pull request once more with the title prefix and
logs a warning naming the repository (`draft-mode: title-prefix` silences it). The host forwards
plugin stderr at `info` level (event "plugin stderr"), so the warning appears there, not at `warn`.
In repositories without drafts this costs one failed request per draft pull request. The fallback
depends on GitHub's message text; if it changes, `auto` fails like `native`. `draft: true` on a
ready pull request adds the prefix.

Switching the mode while drafts are open is safe: reading recognizes both forms.

## Review feedback

- Threads, their ids and `resolved` come from GitHub's review threads (`isResolved`). The thread
  id is the GraphQL node id. `line` is GitHub's `line`: `none` for a comment on the whole file and
  for an outdated thread.
- Every connection is read with `last: 100` (the newest 100, in chronological order) and no further
  pages; `truncated` is set if any of them, including a thread's comments, has an older page. Threads
  are picked by creation order, so on a pull request with more than 100 threads an old thread with a
  new reply is left out.
- Pending reviews (`states` filter) and comments of a pending review are not listed; a thread with
  only such comments is dropped. A dismissed review is listed as `commented`.
- IDs of comments and reviews are GraphQL node ids, also the one `reply` returns.
- Roles: the `viewer` login (case-insensitive) is `bot`. An author with `authorAssociation`
  `OWNER`, `MEMBER` or `COLLABORATOR` is `member` only if the permission lookup returns `admin` or
  `write` (`maintain` is reported as `write`); `read`, `none` or a 404 is `outside`. The
  association alone says nothing about write access: an organization member with read access is
  `MEMBER`. Other associations (`CONTRIBUTOR`, `FIRST_TIME_CONTRIBUTOR`, `FIRST_TIMER`, `MANNEQUIN`,
  `NONE`) are `outside` without a lookup. A deleted author has an empty login and is `outside`.
  Any other failed lookup fails `list-feedback`. The lookup was not verified against github.com.
- `since` (RFC 3339, any offset) is applied in the plugin: comments by `createdAt` or `updatedAt`,
  reviews by `submittedAt` or `updatedAt`, threads if one comment matches, each kept whole.
- `reply` checks the thread's pull request number and repository (case-insensitive) first: a
  mismatch or a node that is not a review thread is `invalid-input`, an unknown id `not-found`
  (GraphQL `NOT_FOUND`).

## Webhooks

`handle-webhook` takes a delivery with content type `application/json`. The host checks the
signature: `X-Hub-Signature-256` is `sha256=` plus the hex HMAC-SHA256 of the body. A missing or
malformed header (also only the legacy `X-Hub-Signature`) or a mismatch is `unauthorized`.

| `X-GitHub-Event` | Condition | Event |
| --- | --- | --- |
| `pull_request_review` | action `submitted`, state `approved`, `changes_requested` or `commented` | `review-submitted` |
| `pull_request` | action `closed` | `pull-request-closed` (`merged` from `pull_request.merged`) |
| `check_suite` | action `completed`, conclusion `failure` or `timed_out`, a `head_branch` | `ci-failed` (check = app name, no URL) |
| `status` | state `failure` or `error` | one `ci-failed` per branch at the commit (check = `context`, URL = `target_url`) |
| `issue_comment` | action `created` | `comment-created` (`is-pull-request` if the issue has `pull_request`) |

Any other event or action (also `ping`) yields no event. `mentions` are `@login` outside code spans
and fenced blocks; `@org/team` and e-mail addresses are skipped. `whoami` is the GraphQL `viewer`;
`author-role` maps the permission lookup like the roles in review feedback, without the
`authorAssociation` filter.

The example payloads in `tests/fixtures/webhooks/` are from
[`octokit/webhooks`](https://github.com/octokit/webhooks) (MIT), some edited in the tests. Not
verified against a delivery from github.com.

## Token

A fine-grained personal access token restricted to the project's repositories, with Pull requests
read/write, Issues read/write, Contents read/write (Git clone and push) and Metadata read. A classic
token needs `repo`. The route sets `authScheme: bearer`; the host injects the `Authorization` header.

Status: verified against github.com (`cargo test -p df-plugins --test github_live -- --ignored`,
2026-10-30) in a private repository of a personal account on GitHub Free with `native` (default),
`title-prefix` and `auto`, and in a public one with `native`: creating a draft, finding it by head
and base, retitling it (stays a draft), marking it ready and commenting on the pull request and on
an issue all succeed. GitHub created native drafts in both repositories, so `auto` did not need its
fallback; the unit tests cover the fallback. A task on github.com ran end to end on the dev cluster
(2026-11-01): clone and push through the gateway's Git proxy, draft pull request after the first
push, ready and summary comment after `complete_task`.

## Error mapping

Rate limits are checked first, then the status table applies.

| GitHub response | `error` variant |
|---|---|
| 403 or 429 with `x-ratelimit-remaining: 0`, a `retry-after` header or a message naming a rate limit | `rate-limited`: `retry-after` seconds, else seconds until `x-ratelimit-reset` (0 if past), else 60 |
| 401, 403 | `unauthorized` |
| 404 | `not-found`, noting that GitHub also answers 404 when the token cannot see a private repository |
| 409 | `conflict` |
| 422 with "pull request already exists" (any case) in `message` or `errors[].message` | `conflict` |
| other 422, including other "already exists" messages | `invalid-input` (all messages) |
| other non-2xx | `upstream` (status, message; non-JSON bodies truncated to 200 characters) |
| GraphQL `errors` with HTTP 200, by the first error's `type`: `NOT_FOUND`, `FORBIDDEN`, `RATE_LIMITED`, other | `not-found`, `unauthorized`, `rate-limited` (60 s), `upstream` (status 200); all messages joined |
| missing or invalid `base-url`, unknown `draft-mode`, transport setup failure | `config`, `transport` |
| unparsable response | `internal` |
