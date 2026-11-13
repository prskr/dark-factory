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

Projects are a section of the settings area: on a desktop open **Settings** in the navigation rail
(it opens **Projects**), on a phone use **Projects** in the bottom bar. The project list shows the
number of queued, running, waiting and done tasks per project.

**New project** opens a form; **Create project** stores the project and opens its settings page. A
project is edited on its **settings page** at `/projects/<key>` (the project's name or its edit icon in the
list). The page has a back link to the list, the project's key, name and forge icon, tabs and one
**Save project** button. After a save the page stays open and says **Project saved**. The open tab
is part of the URL (`?tab=security`, `?tab=integrations`), so a tab can be linked. After a save with an error the tab with the first error opens and the URL follows it.

The **General** tab and the new-project form group the fields:

| Group | Field | Meaning |
| --- | --- | --- |
| Project | Key | The task key prefix (`DF` → tasks `DF-1`, `DF-2`, …): 2–10 capital letters or digits, starting with a letter. Typed when the project is created, read-only afterwards. |
| | Name | Display name. |
| Repository | Repository URL | HTTPS clone URL. Its host selects the forge plugin; the hint below the field names it. |
| | Base branch | Where task branches start and pull requests point to. |
| | Forge credential | The credential reference for the forge. |
| | Issue tracker URL | Optional. |
| Sandbox | Sandbox image | The image the agent works in. It needs `git` and a shell; the runtime and the harness are mounted into it. |
| | Adapter command | Optional. Replaces the instance's harness image for this project (a harness installed in the sandbox image). Arguments use shell-style quoting: `/opt/adapter '--config=/workspace/my config.json'` is two arguments, the second with a space. Nothing is expanded (`$HOME` stays literal). A word that starts with `#` must be quoted (`'#fff'`), otherwise it starts a comment and the rest of the field is dropped. A literal backslash is written `\\`. An unclosed quote is refused. The field shows the stored arguments quoted the same way. |
| Forge events | Wake tasks on reviews and mentions | Whose PR reviews and `@mentions` of the bot account wake a task when [forge webhooks](../integrations/forges/webhooks.md) are set up: *Nobody*, *Members with write access* (default) or *Anyone*. Merged or closed pull requests and failed checks apply regardless. |

The **Security** tab has two groups. **Save project** saves General and Security together.

| Group | Field | Meaning |
| --- | --- | --- |
| Push checks | Protected paths, secret scan, blob size, linear history | What the [Git proxy](../security/git-proxy.md#push-checks) checks for this project: each check in mode *approval* or *reject*, and whether merge commits are allowed. |
| | Hidden paths | Files that are left out of the agent's workspace and never pushable ([Hide files from agents](hidden-paths.md)). |
| Branches | Branch rewrites | Whether agents may rebase their task branch: *With lease (default)* or *Never*. A change applies from the next run of each task and is audited with the before and after value. See [Rebase with lease](existing-branches.md#rebase-with-lease). |
| | Agent branches | One glob per line (`*` stops at `/`, `**` crosses it). The existing branches an agent may start tasks on with `create_task`. Default `renovate/**` and `dependabot/**` for new projects and the migrated ones; an empty field means agents cannot use existing branches. Board members can pick any branch in the composer. The globs are checked when the task is created, so a change does not affect existing tasks. An invalid line is refused with its line number. Changes are audited. See [Agent branches](existing-branches.md#agent-branches). |

Users with the role `admin` also see the last tab, **Integrations**, with two groups:

- **Lifecycle events**: the instance public key and the project's
  [lifecycle event subscriptions](../integrations/lifecycle-events.md#subscriptions). Subscriptions
  save on their own; **Save project** does not apply to them.
- **Incoming webhook**: the URL the forge sends events to (`<publicUrl>/webhooks/<plugin>`, the
  path only when the instance has no [`publicUrl`](../authentication/_index.md)) and the webhook
  secret, to [register the webhook](../integrations/forges/webhooks.md#register-the-webhook-in-forgejo).
  The secret is shared by every project the forge plugin serves. It is shown masked; **Show secret**
  fetches it (each read is audited) and **Copy secret** copies it without showing it. The copy
  buttons work over HTTPS or on a loopback host only; elsewhere the group says so and **Show secret**
  still works. When no webhook applies, the group says why: no forge credential or no integration
  serves the project, the integration did not load, it does not support webhooks, or no webhook
  secret is configured.

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
