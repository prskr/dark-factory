---
title: Credentials, projects and personas
linkTitle: Projects and personas
weight: 1
---

Everything below is configured in the web UI by a board member and stored in the database. Every
create and edit is audited with the old and new values.

## Credential references

Dark Factory never stores secret values. A **credential reference** names a Kubernetes Secret in the
instance namespace and a key in it; the control plane reads the value when it uses it.

1. Create the Secret, for example
   `kubectl -n dark-factory create secret generic claude-token --from-file=token=<file>`.
2. **Settings → Credentials → New credential reference**: a name, the Secret name and the key.

Two kinds of credentials are referenced:

| Credential | Used by | Reaches the sandbox |
| --- | --- | --- |
| Forge credential (a token of a bot account) | the forge plugins (API) and the Git proxy | never |
| Model credential (for example a Claude subscription token or an API key), referenced by a [model provider](model-providers.md) | the gateway's inference endpoint | never; the harness gets a placeholder |

## Projects

**Projects → New project**:

| Field | Meaning |
| --- | --- |
| Name, Key | Display name and the task key prefix (`DF` → tasks `DF-1`, `DF-2`, …). The key is 2–10 capital letters or digits, starting with a letter, and cannot change later. |
| Repository URL | HTTPS clone URL. Its host selects the forge plugin; the hint below the field names it. |
| Base branch | Where task branches start and pull requests point to. |
| Issue tracker URL | Optional. |
| Sandbox image | The image the agent works in. It needs `git` and a shell; the runtime and the harness are mounted into it. |
| Adapter command | Optional. Replaces the instance's harness image for this project (a harness installed in the sandbox image). |
| Forge credential | The credential reference for the forge. |
| Wake tasks on reviews and mentions | Whose PR reviews and `@mentions` of the bot account wake a task when [forge webhooks](../integrations/forges/webhooks.md) are set up: *Nobody*, *Members with write access* (default) or *Anyone*. Merged or closed pull requests and failed checks apply regardless. |

The **Push checks** tab sets what the [Git proxy](../security/git-proxy.md#push-checks) checks for
this project: protected paths, secret scan and blob size, each in mode *approval* or *reject*, and
whether merge commits are allowed.

Users with the role `admin` also see the tab **Lifecycle events**: the instance public key and the
project's [lifecycle event subscriptions](../integrations/lifecycle-events.md#subscriptions).
Subscriptions save on their own; **Save project** does not apply to them.

The project list shows the number of queued, running, waiting and done tasks per project.

## Personas

A **persona** is an agent with a role. **Settings → Personas → New persona**:

| Field | Meaning |
| --- | --- |
| Name | Shown in the task list and transcript, for example "Linus". |
| Description | Optional, at most 500 characters: the role and strengths of the persona. The persona card shows it, and agents read it through `list_personas` when they choose whom to [delegate](delegation.md) to. |
| System prompt | The role and the working rules of this agent. |
| Harness | The coding agent that runs in the sandbox: `claude-code` or `opencode` ([guide](opencode.md)). |
| Model provider | The [model provider](model-providers.md) the harness talks to. Only providers that offer a wire API of the harness are listed. |
| Model | Required. One of the models of the provider whose wire API the harness can use. |

Changing the harness or the model provider updates the other selects. A persona has no adapter
command; a project can set one ([Projects](#projects)).

Every task is assigned to one persona; the persona's harness, model and prompt apply to all runs of
the task.
