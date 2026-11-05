---
title: Model providers
linkTitle: Model providers
weight: 2
---

A **model provider** is the model service a persona works with, together with the one credential
Dark Factory uses for it. Agents never hold that credential: their harness talks to the
[gateway's inference endpoint](../security/inference-endpoint.md) with a placeholder key, and the
gateway adds the real credential on the way out. The gateway also reads the token usage of every
request, so the run card shows the same numbers for every harness.

Dark Factory passes three wire APIs through unchanged and does not translate between them:

| Wire API | Paths the gateway forwards |
| --- | --- |
| Anthropic Messages | `POST /v1/messages`, `POST /v1/messages/count_tokens`, `GET /v1/models` |
| OpenAI Chat Completions | `POST /v1/chat/completions`, `GET /v1/models` |
| OpenAI Responses | `POST /v1/responses`, `GET /v1/models` |

Routing features (fallback between providers, load balancing, GitHub Copilot subscriptions) are not
part of Dark Factory. Use an LLM router such as agentgateway and configure it as the upstream URL of
a model provider.

## Create a provider

**Settings → Model providers → New model provider**. The dialog has two tabs: **General** (the form below) and **Models** (the [model list](#models)). A preset prefills both; it is not stored, the provider keeps the values you save. Every change is audited.

| Field | Required | Meaning |
| --- | --- | --- |
| Name, Preset | yes, no | Name is unique and shown on the persona form. Preset (only when creating) prefills the form. |
| Wire APIs and upstream URLs | at least one | Each offered API with the base URL of the upstream. The URL uses `https` and has no user info, query or fragment. A base path is kept: `https://host/inference/go/openai` receives `/v1/chat/completions` as `https://host/inference/go/openai/v1/chat/completions`. |
| Credential reference, Credential scheme | yes | The [credential reference](setup.md#credential-references) names the Secret key that holds the provider's key or token; the page shows the reference, never the value. The scheme says how the gateway presents the credential, see below. |
| Header, Value | Header: scheme *Custom header*; prefix: no | A preview of what the gateway sends: the header name, then the prefix and an example token (`sk-…example…4f7a`, a fixed text, not your key). Both are disabled when the scheme fixes them: `x-api-key` for scheme `x-api-key`, `Authorization` with prefix `Bearer ` (with the space) for `bearer` and `claude-subscription`. With *Custom header* you set the header and an optional prefix, for example `api-key` without prefix or `Authorization` with `Token `. The name may be any valid header name except hop-by-hop headers, `Host` and `Content-Length`. |
| Extra headers | no | One `Name: value` per line, see below. Disabled for the Claude subscription scheme, and a save with extra headers for it is rejected. |

Changing the scheme updates the Header and Value preview; the form keeps its size and what you typed in Header, Value and Extra headers, also while the scheme disables them. A save stores only what the scheme uses.

### Credential schemes

| Scheme | Header sent to the upstream |
| --- | --- |
| `x-api-key` | `x-api-key: <value>` |
| `bearer` | `Authorization: Bearer <value>` |
| `header` | `<name>: <prefix><value>` |
| `claude-subscription` | `Authorization: Bearer <value>`; Anthropic Messages only, only for personas with the harness `claude-code`, and no extra headers |

The client credentials (`Authorization`, `Proxy-Authorization`, `x-api-key`, `api-key`, and the
header of the scheme) are removed from every request before the gateway adds its own.

### Extra headers

The value of an extra header is a template over `task_id`, `task_key`, `run_id`, `project_key` and
`persona_id`, with one filter, `sha256` (lowercase hex). The control plane renders it once per run;
it replaces a header of the same name that the harness sends.

```text
x-opencode-session: {{ task_id | sha256 }}
```

All runs of a task send the same value, so the provider can group them as one session. The form
rejects unknown filters and variables, and names the problem. Extra headers cannot be named like a
credential header (`Authorization`, `x-api-key`, `api-key`, the header of the scheme), a hop-by-hop
header, `Host` or `Content-Length`: the gateway owns these.

### Models

The tab **Models** lists the models personas of this provider can select. A model has:

| Field | Required | Meaning |
| --- | --- | --- |
| Model ID | yes | The ID the upstream expects in the `model` field, unique within the provider. |
| Wire API | yes | The API the model is served over. A model uses one API; if the upstream serves it over two, add it twice under different IDs. The select lists all three APIs, but the save is refused for an API the General tab does not offer ("gpt-6-luna uses OpenAI Responses, which this provider does not offer (tab General)."). |
| Context, Max output | both or neither | Context window and maximum output in tokens, positive whole numbers. OpenCode uses them to track the context and to compact it; without them it assumes none. |

**+ Add model** appends a row, the trash button removes one. The list is not stored in the order
you typed it: after saving it is sorted by model ID. Problems show under the row (a limit without the
other, a model ID twice, an API the provider does not offer) and on the list (a removed model that
a persona uses: "minimax-m3 cannot be removed: persona Ada uses it."). A removed wire API that a model
still uses is refused as well.

### Presets

| Preset | Wire APIs and upstream | Scheme |
| --- | --- | --- |
| Anthropic | Anthropic Messages, `https://api.anthropic.com` | `x-api-key` |
| Claude subscription | Anthropic Messages, `https://api.anthropic.com` | `claude-subscription` |
| OpenAI | Chat Completions and Responses, `https://api.openai.com` | `bearer` |
| OpenCode Go | Chat Completions and Responses, `https://opencode.ai/inference/go/openai`; Anthropic Messages, `https://opencode.ai/inference/go/anthropic`; header `x-opencode-session: {{ task_id | sha256 }}` | `bearer` |

Presets prefill models too (context window and max output as in OpenCode's catalog):

| Preset | Models |
| --- | --- |
| Anthropic, Claude subscription | `claude-opus-5-5`, `claude-sonnet-5-5`, `claude-haiku-5-5` on Anthropic Messages (1000000 / 128000) |
| OpenAI | `gpt-6-luna`, `gpt-6.1-terra`, `gpt-6.1-sol`, `gpt-6-astra` on Responses (1050000 / 128000) |
| OpenCode Go | Chat Completions: `glm-5.3` (1000000 / 131072), `kimi-k3` (1048576 / 131072), `deepseek-v4-pro` (1000000 / 384000); Anthropic Messages: `minimax-m3`, `qwen3.8-max` (1000000 / 131072), `claude-haiku-5-5` (1000000 / 128000); Responses: `gpt-6-luna` (1050000 / 128000) |

The lists age with the services; change them in the form before you save (remove a model, add
`claude-opus-5-6`), and edit the provider later to add new ones.

## Persona and harness

A persona uses exactly one model provider and one model of it. Each harness can use some wire APIs:

| Harness | Wire APIs |
| --- | --- |
| `claude-code` | Anthropic Messages |
| `opencode` | OpenAI Chat Completions, OpenAI Responses, Anthropic Messages |

The persona form lists only providers that offer a wire API of the harness, and below them the models
of the chosen provider whose wire API the harness can use. The model is required. Changing the
harness or the provider updates the lists and drops a choice that no longer fits. A `claude-subscription`
provider is only offered to `claude-code`.

A run uses the wire API of its persona's model. The control plane gives the harness the gateway URL,
the placeholder key `df-placeholder` and the instance CA, in environment variables or, for
`opencode`, in a generated configuration (see [Run a persona with OpenCode](opencode.md)). Claude Code
gets the persona's model through `ANTHROPIC_MODEL`; its small and fast model is not set.

A provider cannot be deleted while a persona uses it, a model cannot be removed while a persona
uses it, and a change of scheme or of a model's wire API that a persona's harness cannot use is
refused. The messages name the personas.

## How-to

### Use a Claude subscription

1. `kubectl -n dark-factory create secret generic claude-token --from-file=token=<file>` with a
   token from `claude setup-token`.
2. **Settings → Credentials → New credential reference**: name `claude`, the Secret `claude-token`,
   key `token`.
3. **Settings → Model providers → New model provider**, preset *Claude subscription* (header, value and extra
   headers are disabled: the scheme fixes them), name it, choose the
   credential reference, save.
4. **Settings → Personas → New persona**: harness `claude-code`, model provider the one you created, a model of it (for example `claude-sonnet-5-5`).

`api.anthropic.com` does not have to be on the egress allowlist: model traffic goes to the gateway,
which connects to the provider itself. That traffic is not shown in the network graph; see
[where the traffic is visible](../security/inference-endpoint.md#where-the-traffic-is-visible).

Claude Code would also send non-essential traffic (telemetry, update checks) directly, not
through `ANTHROPIC_BASE_URL`. Dark Factory sets `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` for
Claude Code, so this traffic does not appear in the network graph.

### Use an OpenAI-compatible provider

1. Create the credential reference for the API key.
2. **New model provider**, preset *OpenAI* (or empty), set the URL of your service, scheme `bearer`.
3. Add the models of the service on the **Models** tab (the preset has the usual ones). A persona with
   the harness `opencode` can select any of them; `claude-code` only models on Anthropic Messages.

### Use OpenCode Go

1. Create the credential reference for the OpenCode Go key.
2. **New model provider**, preset *OpenCode Go*; the session header is already filled in.
3. The preset has the models of Go with the API each is served over. `claude-code` personas can select
   the ones on Anthropic Messages, `opencode` personas all of them: see
   [Run a persona with OpenCode](opencode.md).

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| The model provider is missing on the persona form | The provider offers no wire API of the harness, or uses `claude-subscription` with another harness. |
| The model select of the persona form is empty | The provider has no model on a wire API of the harness: add one on the provider's **Models** tab. |
| Requests answer `502 model provider credential unavailable` | The Secret of the credential reference cannot be read: check the reference, the Secret and the key. Gateways pick up a rotated Secret within a minute. |
| Requests answer `502 model provider unreachable` | The gateway cannot connect to the upstream URL. |
| The run card shows "N requests without usage" | The provider's responses carried no usage (for example an OpenAI stream without `include_usage`). |
