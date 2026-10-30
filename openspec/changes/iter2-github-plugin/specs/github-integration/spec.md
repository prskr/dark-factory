# Spec Delta

## Purpose

The GitHub plugin implements pull requests and issues for GitHub (github.com and GitHub Enterprise Server) behind the forge-neutral `darkfactory:integrations@0.1.0` interfaces, hiding GitHub specifics such as its draft handling, required request headers and rate-limit responses.

## ADDED Requirements

### Requirement: Create pull request
`create` SHALL open a pull request from head into base with title and body. With `draft: true` it SHALL mark the pull request as draft in the configured draft mode; the result SHALL have `draft: true` and the title without any marker. An open pull request for the same head and base SHALL yield `conflict`.

#### Scenario: Draft with title prefix
- **WHEN** `create` is called with title "DF-1: Fix login" and `draft: true`, and `draft-mode` is not set
- **THEN** GitHub receives the title "WIP: DF-1: Fix login" without the draft flag, and the result has title "DF-1: Fix login" and `draft: true`

#### Scenario: Native draft
- **WHEN** `draft-mode` is `native` and `create` is called with title "DF-1: Fix login" and `draft: true`
- **THEN** GitHub receives the unchanged title with `draft: true`, and the result has `draft: true`

#### Scenario: Duplicate
- **WHEN** GitHub answers 422 with "A pull request already exists for o:factory/task/DF-1."
- **THEN** the result is `conflict`

#### Scenario: Drafts unsupported
- **WHEN** `draft-mode` is `native` and GitHub answers 422 with "Draft pull requests are not supported in this repository."
- **THEN** the result is `invalid-input` and its message names `draft-mode: title-prefix`

### Requirement: Find open pull request
`find-open` SHALL return the open pull request from head into base if one exists, else none. It SHALL ask GitHub with one request filtered by head (`<owner>:<branch>`) and base, and SHALL only return a pull request whose head and base branches match exactly.

#### Scenario: Recover after crash
- **WHEN** a pull request from `factory/task/DF-1` into `main` was created but not recorded
- **THEN** `find-open` returns it with number, URL and head SHA after one request

#### Scenario: No open pull request
- **WHEN** GitHub returns an empty list for head `factory/task/DF-1` and base `main`
- **THEN** the result is none

### Requirement: Update pull request
`update` SHALL change only the given fields. `draft: false` SHALL remove the draft marker in either form (native draft or title prefix), `draft: true` SHALL add it in the configured draft mode, and a new title SHALL keep the current draft state in its current form.

#### Scenario: Ready for review with title prefix
- **WHEN** a pull request titled "WIP: DF-1: Fix login" is updated with `draft: false`
- **THEN** its title on GitHub becomes "DF-1: Fix login" and the result has `draft: false`

#### Scenario: Ready for review with native draft
- **WHEN** a native draft pull request is updated with `draft: false`
- **THEN** GitHub no longer treats it as a draft and the result has `draft: false`

#### Scenario: Mode switched with an open draft
- **WHEN** a pull request was created as draft in `title-prefix` mode, `draft-mode` is now `native`, and it is updated with `draft: false`
- **THEN** the prefix is removed from its title and no native draft change is sent to GitHub

#### Scenario: Retitle a draft
- **WHEN** a draft pull request gets the new title "DF-1: Fix login redirect"
- **THEN** it remains a draft in the same form with that title

#### Scenario: Already ready
- **WHEN** a pull request that is not a draft is updated with `draft: false`
- **THEN** no draft change is sent to GitHub and the result has `draft: false`

### Requirement: Pull request state
Pull requests SHALL be returned with state `merged` when GitHub reports a merge time, `closed` when closed without one, and `open` otherwise. A body GitHub reports as empty or missing SHALL be returned as an empty string.

#### Scenario: Merged pull request
- **WHEN** GitHub reports `state: closed` with `merged_at` set
- **THEN** the state is `merged`

#### Scenario: Empty body
- **WHEN** GitHub reports `body: null`
- **THEN** the body is empty

### Requirement: Issues
`get` SHALL return an issue's title, body, state, labels and URL; `add-comment` SHALL add a Markdown comment and return its id and URL. Pull request numbers SHALL be accepted as issue numbers.

#### Scenario: Comment on a pull request
- **WHEN** `add-comment` is called with a pull request's number
- **THEN** the comment appears on the pull request

#### Scenario: Issue without body
- **WHEN** the issue has no description
- **THEN** `get` returns an empty body

### Requirement: Draft mode setting
The setting `draft-mode` SHALL accept `title-prefix` (default) and `native`. The setting `wip-prefixes` SHALL list the recognized title prefixes (default `WIP:,[WIP]`), the first one marking new drafts. Any other `draft-mode` SHALL fail every call with `config` naming the value.

#### Scenario: Unknown draft mode
- **WHEN** `draft-mode` is `auto`
- **THEN** every call fails with `config` naming `auto`

#### Scenario: Custom prefix
- **WHEN** `wip-prefixes` is `Draft:` and a draft is created in `title-prefix` mode
- **THEN** GitHub receives the title "Draft: DF-1: Fix login"

### Requirement: Request headers
Every request SHALL carry a `User-Agent` naming the plugin and its version, `Accept: application/vnd.github+json` and `X-GitHub-Api-Version` from the setting `api-version` (default `2026-03-10`). The plugin SHALL NOT set `Authorization`.

#### Scenario: Headers on every request
- **WHEN** the plugin sends any request
- **THEN** it carries the three headers and no `Authorization` header of its own

#### Scenario: API version override
- **WHEN** `api-version` is `2022-11-28`
- **THEN** requests carry `X-GitHub-Api-Version: 2022-11-28`

### Requirement: API base URL
REST requests SHALL go to `base-url` including its path. GraphQL requests SHALL go to `/api/graphql` on the same host when `base-url` ends with `/api/v3`, else to `<base-url>/graphql`. A missing `base-url`, or the web host `https://github.com` as `base-url`, SHALL fail every call with `config`.

#### Scenario: github.com
- **WHEN** `base-url` is `https://api.github.com`
- **THEN** pull requests are read from `https://api.github.com/repos/<owner>/<repo>/pulls/<number>` and GraphQL requests go to `https://api.github.com/graphql`

#### Scenario: GitHub Enterprise Server
- **WHEN** `base-url` is `https://ghe.example.org/api/v3`
- **THEN** REST paths start with `/api/v3/repos/` and GraphQL requests go to `https://ghe.example.org/api/graphql`

#### Scenario: Web host as base URL
- **WHEN** `base-url` is `https://github.com`
- **THEN** every call fails with `config` naming `apiUrl: https://api.github.com`

### Requirement: Error mapping
HTTP 401 and 403 without rate-limit signal SHALL map to `unauthorized`, 404 to `not-found`, 409 and responses saying a pull request already exists to `conflict`, other 422 to `invalid-input` with GitHub's error messages, and other non-2xx to `upstream`. GraphQL errors (HTTP 200 with `errors`) SHALL map by type: `NOT_FOUND` to `not-found`, `FORBIDDEN` to `unauthorized`, `RATE_LIMITED` to `rate-limited`, any other to `upstream`.

#### Scenario: Bad token
- **WHEN** the injected token is invalid
- **THEN** every call returns `unauthorized`

#### Scenario: Missing permission
- **WHEN** GitHub answers 403 with "Resource not accessible by personal access token" and no rate-limit signal
- **THEN** the result is `unauthorized` with GitHub's message

#### Scenario: GraphQL error
- **WHEN** marking a native draft ready returns HTTP 200 with an error of type `NOT_FOUND`
- **THEN** the result is `not-found`

### Requirement: Rate limits
A 403 or 429 response SHALL map to `rate-limited` when `x-ratelimit-remaining` is `0`, a `retry-after` header is present or the message names a rate limit. Its retry-after SHALL be the `retry-after` header, else the seconds until `x-ratelimit-reset`, else 60 s.

#### Scenario: Primary rate limit
- **WHEN** GitHub answers 403 with `x-ratelimit-remaining: 0` and `x-ratelimit-reset` 600 s ahead
- **THEN** the result is `rate-limited` with a retry-after of about 600 s

#### Scenario: Secondary rate limit
- **WHEN** GitHub answers 429 with `retry-after: 30`
- **THEN** the result is `rate-limited` with a retry-after of 30 s

#### Scenario: Secondary rate limit without hint
- **WHEN** GitHub answers 403 with a message about a secondary rate limit and no `retry-after` or `x-ratelimit-remaining: 0`
- **THEN** the result is `rate-limited` with a retry-after of 60 s
