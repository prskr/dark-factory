---
title: Run a persona with OpenCode
linkTitle: OpenCode
weight: 3
---

[OpenCode](https://opencode.ai) is the second harness next to Claude Code. It speaks
ACP (`opencode acp`) and works with any [model provider](model-providers.md) that serves a model
over OpenAI Chat Completions, OpenAI Responses or Anthropic Messages, so a persona is not tied to
one vendor. Like every harness it reaches its model only through the gateway's
[inference endpoint](../security/inference-endpoint.md) with a placeholder key.

## Create the persona

1. Create the credential reference for the provider's key
   ([Credentials](setup.md#credential-references)).
2. **Settings → Model providers → New model provider**, for example with the preset *OpenCode Go*.
   On the **Models** tab check the models and their wire APIs; add the ones you need.
3. **Settings → Personas → New persona**: harness `opencode`, the model provider, and a model of it,
   for example `glm-5.3`. The provider list shows only providers that offer a wire API of OpenCode,
   the model list only models served over one. A provider with the scheme `claude-subscription` is not
   offered: it is for `claude-code`.
4. Create a task for the persona. The first run starts OpenCode from the harness image (`harness.opencode.image`).

## What a run gets

The control plane generates OpenCode's configuration for every run and passes it in the environment,
so it ranks above the user's and the repository's OpenCode configuration:

- One provider `df` that points at `<gateway>/inference/v1` with the placeholder key, and your
  model as `model` and `small_model`. The AI SDK package follows the wire API of the model. The
  model's context window and max output tokens are passed when the model has them; OpenCode uses
  them to track the context and to compact it.
- Update checks and the model catalog download are off. OpenCode contacts no host besides the
  gateway unless a tool needs one or the repository's `opencode.json` defines a provider (see
  [Repository configuration](#repository-configuration)).
- The persona's system prompt is OpenCode's global `AGENTS.md`. The repository's own `AGENTS.md`
  still applies: project configuration stays enabled, so the model sees both.

## Repository configuration

OpenCode reads the `opencode.json` and the `.opencode/` directory of the repository. This stays in
effect:

- agents, commands and custom tools of the repository;
- MCP servers (remote ones are subject to the [egress policy](network-access.md));
- permissions, except the two tools below;
- the repository's `AGENTS.md`, including nested ones.

The generated configuration is merged last and always overrides:

- `model`, the title model and `small_model`: they stay the persona's model;
- the providers that may be used: only `df`;
- plugins: denied;
- the `question` and `websearch` tools: denied, even when the repository allows them.

A provider that the repository defines stays configured but unused, and OpenCode contacts its host
when it starts. If the [egress policy](network-access.md) denies that host, OpenCode does not start
and the run fails at the start of the harness; the denied request shows in the egress log. Remove the
provider from the repository's `opencode.json` or allow its host.

The reference table is in [Agent channel configuration](../reference/agent-channel/configuration.md); the keys
`harness.opencode.image`, `.adapterCommand` and `.env` of the instance configuration (chart values
`image.harness.opencode.*`) change the image, command and environment.

## What is disabled, and why

| Feature | State | Why |
| --- | --- | --- |
| `question` tool | denied | A blocking question ends the turn of a run. Agents ask humans with the factory tool `ask_question`, which fits that model. |
| `websearch` tool | denied | There is no search provider behind it. |
| Plugins | denied | A plugin in the repository's or a global configuration would be installed from a package registry, which the sandbox cannot reach, and would run code in the harness. |
| `webfetch` tool | allowed | It goes through the egress proxy like every other outbound request: allow the host in [Network access](network-access.md). |
| MCP servers of the repository | allowed | Remote servers fail at the egress policy unless the host is allowed and only log warnings. The factory MCP server is always available. |

## Session resume

OpenCode keeps its session state below `HOME` (`/data/home`), which is on the task volume. The next
run of the task starts with the harness session id of the previous run and loads the session, so
OpenCode continues with its conversation. If it cannot, the run starts a new session and the
control plane passes a summary of the earlier runs, as for Claude Code.

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| The run fails right at the start and names the harness | `harness.opencode.image` is blank and the project sets no adapter command. |
| The run fails at the start of the harness and the egress log shows a denied host | The repository's `opencode.json` defines a provider with that host. Remove the provider or allow the host. |
| Model requests answer `502` | See [Model providers](model-providers.md#troubleshooting). |
| Reasoning output of a model looks plain | OpenCode's catalog marks models with a field that carries reasoning text; a model of a Dark Factory provider has no such entry, so only the display of reasoning is affected. |
