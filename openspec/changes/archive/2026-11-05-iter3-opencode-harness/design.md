# Design

## Context

See proposal.md (Why). After `iter3-inference-gateway`:

- Harnesses are code constants in `df-core::harness` (wire APIs, env variables); a persona references a model provider and holds a free-text model; the control plane passes base URL, placeholder key and CA variables through `AcpHarness.env`.
- Claude Code comes from the `claude-harness` image mounted at `/opt/harness/claude-code`; `AcpHarness.settings_json` is written to `~/.claude/settings.json`, the system prompt to `~/.claude/CLAUDE.md` by the runtime for every harness.

### Spike (2026-11-04)

OpenCode v2.2.5 (official image `ghcr.io/anomalyco/opencode:2.2.5`, Alpine) driven over ACP by a client that mirrors factory-agent (empty client capabilities, auto-approve), in Podman on an internal network with a fake TLS upstream and a logging DNS server. Source read at tag `v2.2.5`.

| Question | Result |
|---|---|
| ACP | `opencode acp` starts a local server child and speaks ACP v1 (SDK 1.6.0): `loadSession`, `session/resume`, `list`, `close`, `fork`; MCP servers stdio and HTTP, not SSE. |
| Version | Pin 2.2.5. v1 had a different ACP implementation. GitHub's "latest" flag points at the v1 maintenance line (1.19.x). |
| Outbound hosts with D1 and D7 | Only the inference endpoint (DNS log). The catalog comes from the bundled snapshot. |
| CA | `NODE_EXTRA_CA_CERTS` and `SSL_CERT_FILE` each work alone; without either, TLS fails. |
| Wire APIs | Chat (`@ai-sdk/openai-compatible`), Responses (`@ai-sdk/openai`) and Messages (`@ai-sdk/anthropic`) all work with streaming and tool calls; paths `<baseURL>/chat/completions`, `/responses`, `/messages`; Chat sends `stream_options.include_usage`. The three packages are built in, no npm fetch. |
| Auth headers | Bearer for both OpenAI APIs, `x-api-key` for Anthropic. The gateway strips both and injects the provider's credential. |
| Session state | `$XDG_DATA_HOME/opencode`, default `$HOME/.local/share/opencode` (`opencode.db`). `session/load` after a restart replays the history. It rejects a `cwd` other than the session's. |
| Permissions | The shell tool runs without `session/request_permission`. |
| Tools without a counterpart | `question` fails cleanly (no elicitation); `websearch` is cancelled; `webfetch` connects directly. `permission: {"question": "deny"}` removes the tool from the model's tool list. |
| Repository `opencode.json` | Cannot override the generated provider or model; a provider it defines stays configured and its host is contacted at start (found in the manual run, D7). A `plugin` entry makes `session/new` fail (`Location plugins did not finish activating`) when npm is unreachable; a `plugin.use` deny policy in `OPENCODE_CONFIG_CONTENT` fixes it, `plugin: []` does not. A remote MCP server only logs warnings. |
| Instructions | `~/.config/opencode/AGENTS.md` and the repository's `AGENTS.md` reach the model; `~/.claude/CLAUDE.md` is not read. `OPENCODE_DISABLE_PROJECT_CONFIG` would also drop the repository's `AGENTS.md`. |
| v1 switches | `OPENCODE_PURE`, `OPENCODE_DISABLE_LSP_DOWNLOAD`, `OPENCODE_DISABLE_DEFAULT_PLUGINS` are not read in v2 (only documented). |
| Custom provider | Needs a `models` entry; `limit.context` and `limit.output` (both required once `limit` is set) drive context tracking and compaction. |
| OpenCode Go | Serves each model on one API (catalog: 19 Chat, 5 Messages, 5 Responses, 1 Google, without deprecated ones), so the wire API is a property of the model. |

## Goals / Non-Goals

**Goals:**
- A persona with harness `opencode` runs the demo scenario with any compatible model provider.
- No network access besides the inference endpoint and the Git proxy is needed.
- Harness-specific files and configuration are declared by the harness constants, not hard-coded in the runtime, so the next harness (Pi) adds constants only.

**Non-Goals:**
- A generic harness abstraction beyond the constants table: iteration 4 (F12).
- OpenCode-specific UI (modes, agents, compaction settings): rejected until needed.
- Model discovery from the upstream (`/v1/models`).

## Decisions

### D1: OpenCode configuration through `OPENCODE_CONFIG_CONTENT`
The control plane renders one JSON document per run and passes it as env:

```json
{
  "provider": { "df": {
    "npm": "@ai-sdk/openai-compatible",
    "name": "Dark Factory",
    "options": { "baseURL": "https://<gateway>/inference/v1", "apiKey": "df-placeholder" },
    "models": { "<model>": { "name": "<model>", "limit": { "context": 1000000, "output": 131072 } } }
  } },
  "model": "df/<model>",
  "small_model": "df/<model>",
  "enabled_providers": ["df"],
  "autoupdate": false,
  "policies": [{ "action": "plugin.use", "resource": "*", "effect": "deny" }],
  "permission": { "question": "deny", "websearch": "deny" }
}
```

`npm` follows the model's wire API: Chat `@ai-sdk/openai-compatible`, Responses `@ai-sdk/openai`, Messages `@ai-sdk/anthropic`. `limit` is left out when the model has no limits. The keys are OpenCode's v1 names, which v2 normalizes. The document ranks above global and project configuration; only managed configuration (`/etc/opencode`) ranks higher.
Alternative: a file in `~/.config/opencode/`. Rejected: lower precedence than the project file and it would sit on the volume.

### D2: Generic harness files
`AcpHarness.settings_json` becomes `repeated HarnessFile files` with `path` (relative to `HOME`) and `content`. The runtime rejects absolute paths and `..` components, creates real directories and writes fresh files (`fs_util::create_real_dir`, `write_fresh`) before every start, and no longer knows `.claude`. The control plane composes the files: the system prompt at the harness's prompt path and, for Claude Code, `harness.claudeCode.settingsJson` as `.claude/settings.json` when it is not empty.
Alternative: keep `settings_json` and add a prompt path. Rejected: every new harness would add a field; Pi needs two files of its own.

### D3: Harness profiles
`df-core::harness` declares per harness one profile: the wire APIs it can use, optional base URL, model and key variables, CA variables, fixed variables, the system prompt path, and an optional config document (delivery: env variable or file below `HOME`) rendered by one function per harness. `claude-code`: Anthropic Messages, `ANTHROPIC_BASE_URL`, `ANTHROPIC_MODEL` (the persona's model ID) and the key variable by scheme, `NODE_EXTRA_CA_CERTS`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`, `.claude/CLAUDE.md`, no document. `opencode`: Chat, Responses, Messages, no base URL or key variables, `NODE_EXTRA_CA_CERTS`, `OPENCODE_DISABLE_MODELS_FETCH=1`, `OPENCODE_DISABLE_AUTOUPDATE=1`, `.config/opencode/AGENTS.md`, D1 in `OPENCODE_CONFIG_CONTENT`. `SSL_CERT_FILE` is not set: it replaces the trust store instead of adding to it.

### D4: Models on the model provider
New table `model_provider_models (provider_id, model_id, wire_api, context_window_tokens NULL, max_output_tokens NULL)`, primary key `(provider_id, model_id)`, foreign key `(provider_id, wire_api)` to `model_provider_apis` (a model only uses an API its provider offers), CHECK: both limits or neither, both positive. `personas.model` becomes required and, with `model_provider_id`, a foreign key to `model_provider_models` (`ON DELETE RESTRICT`); deleting a model or an API that a persona or model uses is refused. A persona is compatible when its model's wire API is one of its harness's. The run's wire API is the model's, so the harness preference order goes away (`choose_wire_api` is removed). The migration inserts one model per distinct (provider, persona model) with Anthropic Messages, the only API existing personas use, and fails for a persona without a model (pre-MVP).
Alternative: wire API and limits on the persona. Rejected: they belong to the model at a provider; two personas on the same model would repeat them.

### D5: Presets with models
Presets prefill models like they prefill URLs; nothing preset-specific is stored. Limits from OpenCode's catalog snapshot (2.2.5):

| Preset | Models (wire API, context / output) |
|---|---|
| Anthropic, Claude subscription | `claude-opus-5-5`, `claude-sonnet-5-5`, `claude-haiku-5-5` (Messages, 1000000 / 128000) |
| OpenAI | `gpt-6-luna`, `gpt-6.1-terra`, `gpt-6.1-sol`, `gpt-6-astra` (Responses, 1050000 / 128000) |
| OpenCode Go | `glm-5.3` (Chat, 1000000 / 131072), `kimi-k3` (Chat, 1048576 / 131072), `deepseek-v4-pro` (Chat, 1000000 / 384000), `minimax-m3` (Messages, 1000000 / 131072), `qwen3.8-max` (Messages, 1000000 / 131072), `claude-haiku-5-5` (Messages, 1000000 / 128000), `gpt-6-luna` (Responses, 1050000 / 128000) |

### D6: Patched binaries, no launcher
Both harness images ship their binaries with the ELF interpreter and RUNPATH set to the musl runtime bundled under `/opt/harness/<name>/lib` (patchelf at build time, pinned in `multitool.lock.json`), so the kernel starts them directly in any project image, glibc or musl.

- `opencode-harness`: the pinned release binary (`opencode-linux-{arm64,x64}-musl.tar.gz`, checksum verified) at `bin/opencode`; the musl loader, `libgcc_s` and `libstdc++` under `lib/`.
- `claude-harness`: Node.js at `bin/node` and the Claude Code CLI at `bin/claude`, patched the same way; the adapter under `node_modules/`, without the unpatched `@anthropic-ai/claude-agent-sdk-linux-*` package that `bin/claude` is built from (it would ship the CLI twice). The default adapter command is `bin/node` with the adapter's `dist/index.js` (a list, so no shell and no shebang); when the run mounts the harness image, the claude-code profile sets `CLAUDE_CODE_EXECUTABLE=/opt/harness/claude-code/bin/claude` (a project's own adapter gets no path into an image that is not mounted). `launcher.c` is removed.

Renovate tracks `anomalyco/opencode` releases; `check_versions.sh` checks that the copies of the version agree.

Alternative: the static launcher of `claude-harness` with root and target as build parameters. Rejected: the launcher runs the musl loader with the program as argument, so `process.execPath` is the loader; `opencode acp` re-executes `process.execPath` for its server child, which then starts the loader without a program (`session/new` fails). Node in the Claude image has the same latent problem. `LD_LIBRARY_PATH`: rejected, it leaks into the tools the agent runs.

Trade-off: the image path is compiled into the binaries, and patchelf rewrites Bun single-file executables (OpenCode, Claude Code CLI). The ACP tests of both images run in the Harness images workflow, so on every version bump.

### D7: Disabled OpenCode features
`question` and `websearch` are denied (D1): questions go through `ask_question` of the factory MCP server, which fits the turn-based run (a blocking question ends the turn), web search has no provider. `webfetch` stays: it goes through the egress policy like Claude Code's. Plugins are denied (D1). Project configuration stays enabled for the repository's `AGENTS.md` and its OpenCode customization (agents, commands, tools, MCP servers, permissions). `OPENCODE_CONFIG_CONTENT` is merged last, so its model, title model, policies and permissions win; a provider the repository defines stays configured but unused, and OpenCode contacts its host at start. If the egress policy denies that host, OpenCode does not start.
Alternative: `OPENCODE_DISABLE_PROJECT_CONFIG=1` with the repository's `AGENTS.md` in `instructions`. Rejected: it drops all repository customization and nested `AGENTS.md` files to protect against a repository that only breaks its own runs.
Alternative: map `question` to `ask_question` through ACP elicitation. Rejected: an elicitation holds the turn open until a human answers.

### D8: Session state
Nothing to add: `HOME=/data/home` puts `opencode.db` on the task volume, and the cwd `/data/repo` is the same in every run, which `session/load` requires.

## Risks / Trade-offs

- [`opencode acp` re-executes itself for its server child; a patchelf or Bun release breaks the bundle embedded in a single-file executable; the mount path changes while the patched interpreter path does not] → the ACP tests of both images run `initialize` (and for OpenCode `session/new`) from the mounted volume in a project image, in the Harness images workflow.
- [Repository `opencode.json` adds plugins or MCP servers] → plugins denied by policy; remote MCP servers fail at the egress policy and only log warnings.
- [Repository `opencode.json` defines a provider; OpenCode contacts its host at start and fails when the egress policy denies it, also when the agent wrote the file in an earlier run] → affects only that repository's runs; the OpenCode guide names the fix (remove the provider or allow the host); the image test pins the merge order with `opencode debug config`.
- [OpenCode release cadence breaks ACP behaviour] → pinned version, Renovate PR per release, the image test and one manual OpenCode run per bump.
- [Catalog per-model compatibility (`reasoning_field` for most Go Chat models) is lost with the custom provider] → only reasoning display is affected; to be checked with a Go model (roadmap backlog, task 9.3 postponed).
- [Preset model lists age] → models can always be added by hand; presets are updated with the OpenCode bump.
- [Factory MCP over stdio with OpenCode not covered by the spike] → manual run task.
- [The spike ran without the pod's proxy environment; OpenCode must send model traffic directly to the gateway (`NO_PROXY`) and `webfetch` through the proxy] → manual run task checks the egress log.

## Open Questions

None. Resolved: OpenCode's Anthropic endpoint accepts Bearer, so the Go preset keeps scheme `bearer` for all APIs. Checked with a Zen key on `https://opencode.ai/inference/anthropic/v1/messages` (same host and path scheme as Go): `Authorization: Bearer` and `x-api-key` both pass authentication (402, no balance), an invalid key gets 401. Go itself is to be checked with a real key (roadmap backlog, task 9.3 postponed).
