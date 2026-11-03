# Forgejo plugin

WASI component implementing world `forge` of `contract/wit/integrations` (`pull-requests` and
`issues`) for Forgejo. Built for `wasm32-wasip2`; run by `df-plugins`. The plugin never sees
credentials: the host adds `Authorization`.

```bash
cargo build -p forgejo-plugin --target wasm32-wasip2 --release   # target/wasm32-wasip2/release/forgejo_plugin.wasm
wasm-tools component wit target/wasm32-wasip2/release/forgejo_plugin.wasm
```

Exports: `pull-requests`, `issues`. Imports: `wasi:http`, `wasi:config`, `wasi:io`, `wasi:clocks`,
`wasi:cli` (pulled in by `std`) and `darkfactory:integrations/types`.

## Configuration (`wasi:config/store`)

`base-url` is set by the host from the project; all other keys come from `plugins.forgejo.settings` in the instance configuration.

| Key | Required | Default | Effect |
|---|---|---|---|
| `base-url` | yes | none | Scheme and authority of Forgejo (no path; sub-path installs are unsupported) → a path is a `config` error. Missing → `config` error. |
| `wip-prefixes` | no | `WIP:,[WIP]` | Work-in-progress title prefixes; the first one marks drafts. |

## API calls

| Operation | Forgejo endpoint |
|---|---|
| `create` | `POST /api/v1/repos/{owner}/{repo}/pulls` |
| `find-open` | `GET /api/v1/repos/{owner}/{repo}/pulls?state=open&page=N&limit=50`, filtered by head and base branch; pages are read until a match or an empty page (the server may cap `limit`) |
| `get` | `GET .../pulls/{index}` |
| `update` | `PATCH .../pulls/{index}` (preceded by a `GET` when title or draft change) |
| issues `get` | `GET .../issues/{index}` |
| issues `add-comment` | `POST .../issues/{index}/comments` (pull request numbers work as issue numbers) |
| `list-feedback` | `GET /api/v1/user` (the bot), `GET .../issues/{index}/comments?since=` (PR comments), `GET .../pulls/{index}/reviews`, `GET .../pulls/{index}/reviews/{id}/comments` per submitted review, `GET .../collaborators/{login}/permission` once per distinct non-bot author |
| `reply` | `GET .../pulls/{index}/reviews/{review-id}`, `GET .../pulls/{index}/reviews/{review-id}/comments` (a path and line without a comment in that review yields `invalid-input`), then `POST .../pulls/{index}/reviews/{review-id}/comments` with `path` and `new_position` (or `old_position` for a negative line) |

`find-open` lists and filters instead of using `GET .../pulls/{base}/{head}`, because that route
breaks for base branches containing `/`.

## Review feedback

Forgejo has no review thread id.

- Review comments of the same review with the same path and line form a thread, as Forgejo 15's UI
  shows one conversation per review and line; a reply lands in the review it answers. Comments of
  two reviews on the same line are two threads. Old-side lines are negative in Forgejo; `line` is
  the absolute number. The thread is `resolved` if any comment has a `resolver`.
- The thread id is `<review-id>:<path>:<line>` (signed line). `reply`
  parses the review id from the front and the line from the back, so paths may contain `:`. A
  malformed id, a review of another pull request (404, or a `pull_request_url` of another pull
  request) or a pending review yields `invalid-input`.
- Reviews in state `PENDING` (drafts) and `REQUEST_REVIEW` (review requests) and their comments are
  not listed.
- Roles: the login of `GET /user` is `bot`. Otherwise the repository permission `write`, `admin` or
  `owner` is `member`; anything else, including an unknown user (404), is `outside`. Other lookup
  errors fail the call. Forgejo answers 403 unless the token's account is a site admin or an
  admin of the repository, so the plugin's credential needs repository admin: with write access
  only, `list-feedback` fails with `unauthorized`.
- `since` (RFC 3339, any offset) is passed to Forgejo for PR comments. Reviews are kept if submitted
  or updated at or after it, threads if one of their comments was created or updated at or after it;
  the plugin compares instants, because Forgejo writes timestamps in the server's time zone.
- Issue comments and reviews are read 50 per page, at most 20 pages each; `truncated` is set when
  the limit stopped a listing.

Status: verified against Forgejo 15.0.11 in a container
(`cargo test -p df-plugins --test homelab -- --ignored review_reply`, 2026-11-03): the reply to a
thread from another user's review lands in that review at the same line, the next `list-feedback`
returns it as the last comment of the thread with role `bot`, and the UI shows it in the same
conversation; a second review on the same line stays a separate thread; a thread id used on
another pull request yields `invalid-input` (Forgejo answers 404 for the review).

## Draft emulation

Forgejo has no draft flag in this API; a pull request is work in progress when its title starts with
a prefix from `WORK_IN_PROGRESS_PREFIXES` (default `WIP:`, `[WIP]`).

- `create` with `draft: true` sends `"<first prefix> <title>"`.
- Every response is split into `draft` and the title without the prefix (case-insensitive).
- `update` reads the current title, then sends only the title with the requested state:
  `draft: false` strips the prefix, `draft: true` adds it, a new title keeps the current state.

Status: verified against the homelab Forgejo with the default prefixes
(`cargo test -p df-plugins --test homelab -- --ignored`, 2026-10-21): creating a draft, finding it by
head and base, retitling it (stays a draft), marking it ready (prefix removed) and commenting on
the pull request and on an issue all succeed.

## Error mapping

| Forgejo response | `error` variant |
|---|---|
| 404 | `not-found` |
| 401, 403 | `unauthorized` |
| 409, or any message containing "already exists" | `conflict` |
| 422 | `invalid-input` |
| 429 (`Retry-After` in seconds if numeric) | `rate-limited` |
| other non-2xx | `upstream` (status, message) |
| missing `base-url`, transport setup failure | `config`, `transport` |
| unparsable response | `internal` |
