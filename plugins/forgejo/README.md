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

`find-open` lists and filters instead of using `GET .../pulls/{base}/{head}`, because that route
breaks for base branches containing `/`.

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
