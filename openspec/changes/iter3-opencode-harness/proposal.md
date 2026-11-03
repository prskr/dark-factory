# Proposal

> **Partial (2026-11-02):** proposal and design only. Specs and tasks follow once `iter3-inference-gateway` is implemented and the spike in design.md (Open Questions) is done. Until then `.openspec.yaml` sets `skip_specs: true`; remove it together with writing the specs.

## Why

Claude Code is the only harness, so every Dark Factory run depends on one vendor's client and subscription terms. OpenCode speaks ACP natively (`opencode acp`) and works with any model provider of the inference endpoint, which makes it the cheapest way to prove that Dark Factory is harness-agnostic (cheaper than `df-native`) before the harness registry and Generic ACP (F12, iteration 4).

## What Changes

- New harness `opencode`: `personas.harness` accepts `opencode`; harness constant with wire APIs `openai-responses`, `openai-chat`, `anthropic-messages` (preference order to be confirmed by the spike) and CA variables `NODE_EXTRA_CA_CERTS` and `SSL_CERT_FILE`.
- Harness image `deploy/images/opencode-harness` with a pinned OpenCode binary, mounted at `/opt/harness/opencode`, default adapter command `opencode acp`; configurable in `harness.opencode` of the instance config and Helm values.
- Generated OpenCode configuration per run through `OPENCODE_CONFIG_CONTENT`: one provider pointing at the gateway's inference URL with the placeholder key, the persona's model as `model`, `share: "disabled"`, `autoupdate: false`; permission requests go over ACP (auto-approved like Claude Code).
- Sandbox hardening for OpenCode: `OPENCODE_DISABLE_AUTOUPDATE`, `OPENCODE_DISABLE_MODELS_FETCH`, `OPENCODE_DISABLE_LSP_DOWNLOAD`, `OPENCODE_PURE`, `OPENCODE_DISABLE_DEFAULT_PLUGINS`; the repository's own `opencode.json` / `.opencode/` stays loaded unless the spike shows it can override the generated provider (then `OPENCODE_DISABLE_PROJECT_CONFIG`).
- Session state (`$XDG_DATA_HOME/opencode`) on the task volume so `session/load` resumes across runs, like Claude Code's session directory.
- Persona form offers `opencode`; the model field takes the upstream model ID.
- Docs: harness reference entry, "Run a persona with OpenCode" guide.

Out of scope:
- Harness registry and Generic ACP: iteration 4 (F12).
- OpenCode plugins, MCP servers from OpenCode config, LSP downloads: rejected for sandboxes (no egress for them).
- Usage from OpenCode's ACP `usage`: rejected; usage comes from the gateway (ADR-0050).

## Capabilities

### New Capabilities
- `opencode-harness`: starting OpenCode as an ACP harness with generated configuration, disabled network features and persisted session state.

### Modified Capabilities
- `configuration-management`: personas accept harness `opencode`.
- `model-providers` (from `iter3-inference-gateway`): harness compatibility includes `opencode`.
- `sandbox-provisioning`: the OpenCode harness image is mounted for `opencode` personas.
- `helm-deployment`: `harness.opencode` image values.

## Impact

- Depends on `iter3-inference-gateway` (model providers, inference endpoint, harness constants).
- Code: `df-core` (harness constant, config), `df-db` (CHECK constraint migration), `df-agent-channel/assign.rs` (OpenCode env and config), `factory-agent/harness.rs` (per-harness session directory), `df-k8s` (image mount), `df-web` (persona form).
- Images: new `deploy/images/opencode-harness` (Bazel, `check_versions.sh`), Helm values and README.
- Docs: `crates/factory-agent/docs/configuration.md`, user docs.
