# Tasks

These changes must be merged before this one:
- `iter3-branch-workflows`: `list-open` and its filter record.
- `iter3-review-tools`: the "Tool set v2" text; its `factory-tools` spec must be archived so that the MODIFIED delta here applies.

## 1. Contract

- [ ] 1.1 Add interface `ci` (design D1) to the integrations WIT, export it from world `forge`, and add `unsupported` to the error variant if it is missing. Implement a scripted `ci` in `plugins/test-probe`. Verify with `cargo build --workspace` and the plugin-host tests.
- [ ] 1.2 Write failing tests, then make the host's capability detection report `ci` and route `ci` calls with the existing retry rules. Verify with `cargo test -p df-plugins` for the three "Capability detection" scenarios.

## 2. Host-side log handling (`df-plugins` or `df-agent-channel`)

- [ ] 2.1 Write failing tests, then implement ANSI stripping and 64 KiB paging on character boundaries (design D2). Verify with unit tests:
  - CSI colour codes, OSC hyperlinks and a lone ESC are removed;
  - the tail page and the page at `offset: 0` are returned;
  - a multi-byte character on a page edge is kept whole;
  - `total_bytes` and `head_dropped` are passed through.

## 3. Forgejo plugin

- [ ] 3.1 Write failing tests, then implement the `list-open` filters (design D3). Verify with recorded-response tests for the prefix-and-label scenario, and `unsupported` for an author filter below v10.
- [ ] 3.2 Write failing tests, then implement `ci.status` with the version check and job-id mapping. Verify with recorded-response tests for v16 (job id) and v15 (no job id).
- [ ] 3.3 Write failing tests, then implement `ci.job-log` with the `Range` tail and the 4 MiB cap. Verify with recorded-response tests: a log is read, a log larger than 4 MiB gives `head-dropped`, and v15 is `unsupported`.

## 4. GitHub plugin

- [ ] 4.1 Write failing tests, then implement the `list-open` filters (design D4). Verify with recorded-response tests for the author scenario.
- [ ] 4.2 Write failing tests, then implement `ci.status`, which merges check runs and statuses and maps the states. Verify with recorded-response tests for the mixed Actions and external status scenario, plus a pending run.
- [ ] 4.3 Write failing tests, then implement `ci.job-log`: follow the redirect without `Authorization`, keep the 4 MiB tail, strip the BOM and timestamps. Find the actual redirect hosts and add them to the default plugin allowlist in the Helm chart. Verify with recorded-response tests, `helm unittest` for the allowlist, and one manual call against a public repository noted in the PR.

## 5. Tools (`df-agent-channel`)

- [ ] 5.1 Add the three tools with their JSON Schemas to `contract/mcp/factory-tools.v2.json`. The descriptions mark logs as untrusted and state that `read_ci_status` takes exactly one of its arguments. Verify with the existing schema tests and the "CI tools listed" scenario.
- [ ] 5.2 Write failing tests, then implement `list_pull_requests` with the 100-item cap and `truncated`. Verify with `df-agent-channel` tool tests for the `list_pull_requests` and size-limit scenarios.
- [ ] 5.3 Write failing tests, then implement `read_ci_status` (PR to head SHA, ref validation, the both-arguments error). Verify with tool tests for every `read_ci_status` scenario.
- [ ] 5.4 Write failing tests, then implement `read_ci_log`, mapping `unsupported`, expired and unknown jobs to error results. Verify with tool tests for every `read_ci_log`, "Long logs keep their end", "CI reads need forge support" and "CI reads stay in the project repository" scenario.
- [ ] 5.5 Document the three tools in the factory tools reference (`crates/factory-agent/docs/` or `docs/`, wherever the tool reference lives). Include the Forgejo version requirements. Verify that every argument and result field from 5.1 is listed.

## 6. Integration

- [ ] 6.1 Extend the e2e fake forge with commit statuses and a job log, and add an e2e step: a task woken by `ci-failed` calls `read_ci_status` and `read_ci_log` and gets the failing check and log tail. Verify with `just e2e-local`.
- [ ] 6.2 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length` and `openspec validate iter3-ci-tools --strict`. Verify that all four pass.

## Workflow follow-up

- Archive after review.
