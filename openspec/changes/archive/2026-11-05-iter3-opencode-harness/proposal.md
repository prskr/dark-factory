# Proposal

## Why

Claude Code is the only harness, so every Dark Factory run depends on one vendor's client and subscription terms. OpenCode speaks ACP natively (`opencode acp`) and works with any model provider of the inference endpoint, which makes it the cheapest way to prove that Dark Factory is harness-agnostic (cheaper than `df-native`) before the harness registry and Generic ACP (F12, iteration 4).

## What Changes

- New harness `opencode` (OpenCode 2.2.5, `opencode acp`): `personas.harness` accepts `opencode`; the harness uses OpenAI Chat Completions, OpenAI Responses and Anthropic Messages; CA variable `NODE_EXTRA_CA_CERTS`.
- **BREAKING** Models on the model provider: a provider holds a list of models, each with model ID, wire API (one the provider offers) and optional context window and max output tokens (both or neither). A persona selects one model of its provider; the model is required for every persona. The run's wire API is the model's. The persona's free-text `model` and the harness preference order are removed. Presets prefill models (Anthropic, Claude subscription, OpenAI, OpenCode Go); models can always be added and edited by hand. The migration turns each persona's current `model` into a model of its provider and fails for a persona without one.
- **BREAKING** Generic harness files: `AcpHarness.settings_json` is replaced by `files` (path below `HOME`, content). The runtime writes them before every start and keeps no harness-specific paths. The control plane renders them per harness: Claude Code gets `.claude/settings.json` (from `harness.claudeCode.settingsJson`) and the system prompt as `.claude/CLAUDE.md`; OpenCode gets the system prompt as `.config/opencode/AGENTS.md`. The same mechanism fits file-configured harnesses such as Pi (`models.json`, `SYSTEM.md`).
- Harness constants gain an optional config document: where the rendered harness configuration goes (env variable or file under `HOME`) and whether base URL, model and key are env variables (Claude Code gets the persona's model as `ANTHROPIC_MODEL`). OpenCode's document goes to `OPENCODE_CONFIG_CONTENT`: provider `df` (`npm` by the model's wire API, `baseURL` `<inference>/v1`, placeholder key, the persona's model with its limits), `model` and `small_model` `df/<model>`, `enabled_providers: ["df"]`, `autoupdate: false`, a policy denying `plugin.use` and permissions denying the `question` and `websearch` tools (questions go through the factory tool `ask_question`).
- Fixed OpenCode environment: `OPENCODE_DISABLE_MODELS_FETCH=1`, `OPENCODE_DISABLE_AUTOUPDATE=1`. Project configuration stays enabled: the repository's `AGENTS.md` and its OpenCode customization (agents, commands, tools, MCP servers, permissions) apply, while the generated configuration, merged last, pins the model, the title model, the allowed providers, plugins and the denied tools. A provider the repository defines is contacted at start; if the egress policy denies its host, the run fails (design D7).
- Harness image `deploy/images/opencode-harness` (pinned release musl binary, checksum verified, patched to start with the musl loader and libraries bundled under `/opt/harness/opencode/lib`, no launcher), mounted at `/opt/harness/opencode`, default adapter command `/opt/harness/opencode/bin/opencode acp`; configurable in `harness.opencode` of the instance configuration and in the Helm values.
- **BREAKING** `claude-harness` without a launcher: Node.js (`bin/node`) and the Claude Code CLI (`bin/claude`) are patched the same way and `launcher.c` is removed. The default claude-code adapter command is `bin/node` with the adapter's `dist/index.js`; `CLAUDE_CODE_EXECUTABLE` comes from the claude-code harness profile. An overridden claude-code adapter command must be changed.
- Session state needs no change: OpenCode keeps it in `$HOME/.local/share/opencode`, already on the task volume.
- Persona form: harness `opencode`, model select listing the selected provider's models compatible with the harness. Provider form: model list.
- Docs: harness reference entry, provider models, "Run a persona with OpenCode" guide.

Out of scope:
- Harness registry and Generic ACP: iteration 4 (F12).
- A Pi harness: only the generic files prepare for it.
- Importing a provider's model list from its `/v1/models`: added when typing lists becomes tedious.
- OpenCode plugins and MCP servers from OpenCode configuration: plugins are denied; MCP servers from the repository's configuration work when the egress policy allows their hosts.
- Usage from OpenCode's ACP `usage`: rejected; usage comes from the gateway (ADR-0050).

## Capabilities

### New Capabilities
- `opencode-harness`: starting OpenCode as an ACP harness with a generated configuration, disabled network features and tools, and persisted session state.

### Modified Capabilities
- `model-providers`: models per provider (wire API, limits), presets with models, harness compatibility by the persona's model.
- `configuration-management`: personas accept harness `opencode` and reference a provider model.
- `acp-harness`: the runtime writes the harness files of `Assign`; the system prompt is one of them.
- `sandbox-provisioning`: the harness image of `opencode` comes from `harness.opencode.image`.
- `build-pipeline`: OpenCode harness image.
- `helm-deployment`: `image.harness.opencode` values.

## Impact

- Code: `df-core` (harness profiles, OpenCode config rendering, `harness.opencode` config, provider models), `df-db` (migration: `model_provider_models`, persona model reference, harness CHECK; `.sqlx` metadata), `df-proto` (`AcpHarness.files`, breaking), `df-agent-channel/assign.rs` (files and env per harness), `factory-agent/harness.rs` (writes `files`, no Claude paths), `df-k8s` (image map), `df-web` (provider model list, persona model select, presets).
- Images: new `deploy/images/opencode-harness` (Bazel, `test.sh`, `check_versions.sh`, Renovate), Helm values and README.
- Docs: `crates/factory-agent/docs/configuration.md`, `crates/df-agent-channel/docs/configuration.md`, user docs (model providers, personas, OpenCode guide).
