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

The list shows each reference as a row with its name and, below it, `secret / key`. Click the row to
edit the reference in a dialog.

Two kinds of credentials are referenced:

| Credential | Used by | Reaches the sandbox |
| --- | --- | --- |
| Forge credential (a token of a bot account) | the forge plugins (API) and the Git proxy | never |
| Model credential (for example a Claude subscription token or an API key), referenced by a [model provider](model-providers.md) | the gateway's inference endpoint | never; the harness gets a placeholder |

## Projects

Projects are a section of the settings area: on a desktop open **Settings** in the navigation rail
(it opens **Projects**), on a phone use **Projects** in the bottom bar. The project list shows the
number of queued, running, waiting and done tasks per project. Each project is a row of two lines:
the name, and below it the repository (host and path, without the `https://` or `http://` scheme and
the `.git` suffix) with its forge icon and the base branch. On a desktop the whole row opens the
project's settings page; a count that is not zero links to the task list of that status, and an
empty count is left blank. On a phone the project is a card with an edit icon.

**New project** opens a form; **Create project** stores the project and opens its settings page. A
project is edited on its **settings page** at `/projects/<key>` (its row in the list on a desktop, its
edit icon on a phone). The page has a back link to the list, the project's key, name and forge icon, tabs and one
**Save project** button. After a save the page stays open and says **Project saved**. The open tab
is part of the URL (`?tab=security`, `?tab=egress`, `?tab=integrations`), so a tab can be linked. After a save with an error the tab with the first error opens and the URL follows it.

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

The **Security** tab has two groups (the second also holds the subtask merge setting). **Save project** saves General and Security together.

| Group | Field | Meaning |
| --- | --- | --- |
| Push checks | Protected paths, secret scan, blob size, linear history | What the [Git proxy](../security/git-proxy.md#push-checks) checks for this project: each check in mode *approval* or *reject*, and whether merge commits are allowed. |
| | Hidden paths | Files that are left out of the agent's workspace and never pushable ([Hide files from agents](hidden-paths.md)). |
| Branches | Branch rewrites | Whether agents may rebase their task branch: *With lease (default)* or *Never*. A change applies from the next run of each task and is audited with the before and after value. See [Rebase with lease](existing-branches.md#rebase-with-lease). |
| | Subtask merges | Who merges the pull requests of a task's subtasks into the task's branch: *Human (default)*, or *Parent agent*, which lets the agent of the parent task merge them with `merge_subtask_pr` ([Build a feature in iterations](build-a-feature.md)). Merges into the default branch always stay with humans. Changes apply to the next call and are audited with the before and after value (`subtask_merge`). |
| | Agent branches | One glob per line (`*` stops at `/`, `**` crosses it). The existing branches an agent may start tasks on with `create_task`. Default `renovate/**` and `dependabot/**` for new projects and the migrated ones; an empty field means agents cannot use existing branches. Board members can pick any branch in the composer. The globs are checked when the task is created, so a change does not affect existing tasks. An invalid line is refused with its line number. Changes are audited. See [Agent branches](existing-branches.md#agent-branches). |

The **Egress** tab lists the project's [egress rules](network-access.md#rules-of-one-project) (its own
rules and those of its tasks), with **New egress rule**, **Add cluster service**, edit and delete, and
below them the read-only instance rules under **Also applies (instance)**. It saves with its own
dialogs, so **Save project** is not shown on it. Rules of the same host in several projects get the
hint **Also allowed for project …** with **Make global**.

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
| Display name | 1 to 100 characters without line breaks, `<` or `>`. Shown on the persona card, in the task list, in the task details and as the author of the agent's messages in the transcript, for example "Linus". Two personas may share a display name. |
| Handle | The unique short name of the persona: lowercase letters, digits and hyphens, at most 39 characters, starting with a letter or digit. Shown as `@linus`. Agents address the persona by it (`create_task`, [delegation](delegation.md)). |
| Commit email | The author and committer email of the persona's commits: one address (`linus@example.org`), at most 254 characters, stored in lower case. Unique across personas. See [Give a persona its identity](#give-a-persona-its-identity). |
| Description | Optional, at most 500 characters: the role and strengths of the persona. The persona card shows it, and agents read it through `list_personas` when they choose whom to [delegate](delegation.md) to. |
| System prompt | The role and the working rules of this agent. |
| Harness | The coding agent that runs in the sandbox: `claude-code` or `opencode` ([guide](opencode.md)). |
| Model provider | The [model provider](model-providers.md) the harness talks to. Only providers that offer a wire API of the harness are listed. |
| Model | Required. One of the models of the provider whose wire API the harness can use. |

Changing the harness or the model provider updates the other selects. A persona has no adapter
command; a project can set one ([Projects](#projects)).

Every task is assigned to one persona; the persona's harness, model and prompt apply to all runs of
the task.

The form shows a preview of the persona's **avatar** left of the display name and the handle: the
initials of the display name (up to two words) on one of eight colors. The color comes from the
persona's id, so it stays the same when you rename the persona; a persona that is not saved yet has a
neutral preview. Avatars are generated by the control plane: there is no upload and no request to
another host. The cards on **Personas** are ordered by handle and show avatar, display name,
`@handle`, commit email, harness and model.

### Give a persona its identity

Every commit an agent makes carries the identity of the persona that runs it: the display name as
author and committer name, the commit email as author and committer email. That holds for the
commits of the harness and for the WIP snapshots of the runtime, and the
[Git proxy](../security/git-proxy.md) rejects a push with any other author (`df:identity`). Forge
actions of the factory (pushes, pull requests, comments) still use the project's forge credential.

1. Open **Settings → Personas** and edit the persona.
2. Set **Commit email** to an address that suits your forge, for example `linus@example.org`.
   A persona that was created by an upgrade has `<handle>@dark-factory.localhost`
   ([upgrade notes](../operations/upgrades.md#persona-identity)); change it if the commits should
   map to a real address.
3. Save. Another persona that already uses the handle or the email is named at the field
   ("Already used by @linus.") and nothing is saved.

An edit applies to runs started after you saved it. An active run keeps the identity it started with
(its commits would otherwise be rejected by the Git proxy), so the next run of the same task commits
with the new one. The transcript shows the current display name and handle of the persona, also for
old messages; commits in Git history keep the identity they were made with.
