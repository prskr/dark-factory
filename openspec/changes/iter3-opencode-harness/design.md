# Design

## Context

See proposal.md (Why). After `iter3-inference-gateway`:

- Harnesses are code constants in `df-core` (wire APIs, env variables); a persona references a model provider; the control plane passes base URL, placeholder key and CA variables through `AcpHarness.env`.
- Claude Code comes from the `claude-harness` image mounted at `/opt/harness/claude-code`; `AcpHarness.settings_json` is written to `~/.claude/settings.json`.

Research (OpenCode 2.2.5, `anomalyco/opencode`, 2026-11-02):

- `opencode acp` implements ACP over stdio, including `session/load`, `session/resume` and `session/request_permission`.
- Provider config: `provider.<id>.options.{baseURL, apiKey, headers}`; `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL` are **not** honoured, so the base URL must go into the config. `OPENCODE_CONFIG_CONTENT` (inline JSON) has the highest precedence below managed config.
- The common provider packages (`@ai-sdk/openai-compatible`, `@ai-sdk/openai`, `@ai-sdk/anthropic`) are bundled in v2; unknown packages and plugins are fetched from npm.
- Outbound hosts besides the provider: `models.opencode.ai` (catalog), `opencode.ai/update` (update check), share service, LSP downloads, npm. Each has a disable switch.
- Runs on Bun: `SSL_CERT_FILE` adds a CA; `NODE_EXTRA_CA_CERTS` is honoured by OpenCode's own code.
- Sends `x-opencode-session` and session-affinity headers itself on every model request; OpenAI chat streams request `include_usage` by default.
- State: `$XDG_DATA_HOME/opencode` (`opencode.db` SQLite with sessions).

## Goals / Non-Goals

**Goals:**
- A persona with harness `opencode` runs the demo scenario with any compatible model provider.
- No network access besides the inference endpoint and the Git proxy is needed.

**Non-Goals:**
- A generic harness abstraction beyond the constants table: iteration 4 (F12).
- OpenCode-specific UI (modes, agents, compaction settings): rejected until needed.

## Decisions

### D1: Configuration through `OPENCODE_CONFIG_CONTENT`
The control plane renders one JSON document per run and passes it as env. Provider entry `df` with `npm` matching the chosen wire API (`@ai-sdk/anthropic`, `@ai-sdk/openai-compatible`, `@ai-sdk/openai` for Responses), `options.baseURL` = inference URL (+ `/v1` where the SDK expects it), `options.apiKey` = placeholder, `enabled_providers: ["df"]`, `model: "df/<persona model>"`, `share: "disabled"`, `autoupdate: false`.
Alternative: writing `~/.config/opencode/opencode.json` like Claude Code's settings file. Rejected: env needs no file on the volume and wins over files in the repository.

### D2: Image like `claude-harness`
`deploy/images/opencode-harness` downloads the pinned release binary (checksum verified) instead of building from source; the same launcher pattern and `check_versions.sh` entry as `claude-harness`.

### D3: Session state on the task volume
`XDG_DATA_HOME=/data/home/.local/share` (under `HOME=/data/home`, already on the volume), so `opencode.db` survives runs and `session/load` works. No secret is stored there: the key is a placeholder.

### D4: Model ID
The persona's model field holds the upstream model ID; the generated config prefixes the provider (`df/`). An empty model lets OpenCode pick the provider's default from its bundled catalog; the spike checks whether that works with the catalog fetch disabled.

## Risks / Trade-offs

- [Repository `opencode.json` overrides the generated provider or adds plugins] → `OPENCODE_CONFIG_CONTENT` has higher precedence; `OPENCODE_PURE` blocks external plugins; the spike confirms, otherwise `OPENCODE_DISABLE_PROJECT_CONFIG`.
- [OpenCode release cadence breaks the ACP behaviour] → pinned version, `check_versions.sh`, the e2e runs with the fake agent and one manual OpenCode run per bump.
- [npm fetch for an unexpected package] → egress proxy denies it and raises a network access request, visible to the board.

## Open Questions

Spike before writing specs and tasks (OpenCode 2.2.5 in a sandbox with egress blocked):
- Which version introduced `opencode acp`; minimum version to pin.
- No npm, catalog or update calls with the settings of D1 and the disable variables.
- Bun trusts the instance CA with `SSL_CERT_FILE` and/or `NODE_EXTRA_CA_CERTS` for requests to the gateway.
- Preference order of wire APIs for the constant (Responses vs Chat vs Anthropic Messages per provider type).
- Auth header expected by OpenCode Go's Anthropic endpoint (`x-api-key` or Bearer) for the preset.
