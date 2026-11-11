---
title: Tasks
weight: 2
---

A task is a piece of work for one persona in one project. Its code lives on the branch
`factory/task/<KEY>`; the agent opens a draft pull request with its first commit and you review it
in your forge.

## Create a task

**New task** (the green button) opens the composer: pick the project, describe the work, pick the
persona and send. The task gets the next key of the project (`DF-12`) and starts as soon as a run
slot is free. Without a title, the first sentence of the description is used until the agent sets a
better one; you can rename a task at any time.

## Follow and steer it

The task view has three columns: the task list, the transcript and the details.

- **Transcript**: the agent's messages and tool calls (commands, file reads and edits) live, as they
  happen. Forge activity appears in between: pushes to the task branch (with the commit range, or
  the reason if the push was rejected), and the pull request being opened and marked ready, with a
  link to it. Stored artifacts appear as a row with the artifact name (see
  [Artifacts](artifacts.md)). These rows come from the control plane and
  the gateway, not from the agent, and are never sent to the agent.
- **Messages**: type into the message box at any time. A message reaches the agent at the next
  turn boundary, without restarting anything; while it waits it is shown as queued and can be
  edited or removed. A message to a finished task reopens it. A message to a task that awaits its dependencies is
  delivered in its first run.
- **Details**: what the agent is doing now, the pull request and issue links, project, persona
  (the project links to its [settings page](setup.md#projects), the persona to the persona list),
  branch, who created the task and its parent (for a task an agent created), the
  [dependencies](delegation.md#in-the-task-view) and the list of runs with model and token usage. The tokens are summed per model from the
  [gateway's usage records](../security/inference-endpoint.md#usage); requests whose response carried
  no usage are counted separately ("2 requests without usage") instead of as zero. The task
  description is a collapsible section: **Description** with a chevron shows only its header while
  closed, and the first three lines of the text when opened.
- **Artifacts**: once the agent has written an [artifact](artifacts.md), the details get an
  Artifacts tab with a card per artifact; an artifact opens next to the transcript.

The transcript shows the author of each message. In the queued-message stack, messages from agents and
the system are prefixed with their name; your own messages have none. System authors use
readable names instead of internal reason codes: **Task assignment**, **Resume**, **Request decision**,
**Push check**, **Forge events** and **Subtask update**. Other system authors show **Dark Factory**.
[Request decisions](requests.md#what-happens-on-a-decision) appear on their cards; their delivery
messages are hidden by default.

## Agent messages: Markdown, code and diagrams

An agent message is plain text while it streams and is rendered as Markdown when its turn ends.

- **Safety**: the text is untrusted. HTML in it is shown as text, images appear as links (the
  browser loads nothing an agent names), links with unsafe schemes are dropped and every link opens
  with `rel="noopener noreferrer nofollow"`.
- **Code**: a fenced block is highlighted when its info string names a language or file extension
  (` ```rust `, ` ```rs `). Blocks without a language, with an unknown one or over 64 KiB stay
  plain; the language is never guessed from the content. The theme follows the browser's light or
  dark color scheme; the operator picks the themes with `web.codeTheme.light` and
  `web.codeTheme.dark` ([instance configuration](../reference/agent-channel/configuration.md)).
- **Diagrams**: a ` ```mermaid ` fence is drawn as a diagram in the browser, in a light or dark
  theme following the color scheme. The diagram runs isolated from the page (sandboxed iframe), so
  its source cannot run script in the page. A diagram that cannot be parsed stays a code block
  with a one-line error. The diagram renderer (about 3 MB) is loaded only on pages that contain a
  diagram. Without JavaScript the diagram source is shown as a code block.

## Your messages: Markdown

The visible text of your messages is rendered as Markdown with the same safety rules and code
highlighting as agent messages (fenced code, ` ```mermaid ` diagrams). A single line break in
your text stays a line break; blank lines separate paragraphs, and several in a row count as
one. Text that looks like Markdown (`*`, `_`, a `#` at the start of a line, a list) is formatted as
Markdown. Hidden blocks ("Instruction for the agent") and the stack of queued messages stay plain
text.

## Status

| Status | Meaning |
| --- | --- |
| `awaiting_dependencies` | Created with an unsatisfied [dependency](delegation.md#dependencies); it has no run, branch or sandbox and becomes `queued` when the last dependency is satisfied |
| `queued` | Work is pending; a run starts when a slot is free or the resume time has passed |
| `running` | A run is working on it |
| `waiting` | The agent finished its turn and waits for you; the sandbox stays alive for a while (`runs.keepAliveSeconds`) |
| `blocked` | A blocking question or approval was not answered in time; answering resumes the task |
| `in_review` | The agent called `complete_task`; the pull request is ready for review |
| `rate_limited` | The model provider's usage limit was hit; the task resumes automatically at the shown time |
| `limit_reached` | A budget of the run, or the harness's token or turn limit, was reached; a message continues it |
| `completed` | Marked done by a human, or its pull request was merged |
| `cancelled` | Cancelled by a human |
| `failed` | The run failed and automatic retries are exhausted |

The list's filters group them: *Active*, *Waiting* (`awaiting_dependencies`, `waiting`, `blocked`,
`in_review`, `limit_reached`) and *Done* (`completed`, `cancelled`, `failed`).

## Sort order

The sort menu next to the project menu in the title row of the task list (an icon button named after the
current sort; on a phone it sits left of the avatar) offers:

| Option | Order | URL |
| --- | --- | --- |
| Recently updated (default) | last update, newest first | no `sort` |
| Least recently updated | last update, oldest first | `sort=updated-asc` |
| Newest created | creation, newest first | `sort=created` |

The sort is part of the URL and stays when you pick a status chip, a project or a task; an unknown
value uses the default. The list updates live, so a task moves to its new place when it is updated.

A task's last update is its creation, its latest status change or its latest rename. Agent output and
queued messages that do not change the status do not count, so a task that only works in
`running` stays where its last status change put it.

## Interrupt, stop, mark done, cancel

**Interrupt** is the button in the message box while the agent works on a turn (the run is
`running`). It ends the current turn only; the run, the agent's session and the workspace stay as
they are. Nothing the turn already did is rolled back: commits, created tasks and raised questions
stay. The transcript ends the turn with the row "Interrupted by *your name*".

- If messages are queued, they start the next turn right away. To steer the agent, write the
  message first, then press Interrupt. The button shows while the message box is empty; with text
  in it, the round button sends.
- Without queued messages, the run waits for your next message, as after a normal turn end.
- The next turn's input starts with "Your previous turn was interrupted by *your name*." so the
  agent knows why it stopped. A Dark Factory tool call that was still open (for example
  `ask_question`) returns as cancelled.
- The button interrupts the turn you see running. It only works while a turn is open: after the
  agent finished its turn but before it reports waiting (it pushes commits first), the page shows
  "No turn is running".
- An interrupted turn counts as a normal turn: it is no failure, uses no automatic resume and does
  not change the task's status by itself. It counts toward the run's turn budget.
- Exceptions: Interrupt sends a cancel notification (ACP `session/cancel`) and waits up to 5
  seconds for the harness to end the turn.
  - If the harness fails the cancel or does not answer within 5 seconds, the turn ends as an
    error, the run fails and the transcript shows no "Interrupted by" row.
  - If the answer is a provider rate-limit error, the turn and the run end as rate limited.
  - If the turn completed on its own as you pressed the button, it counts as an ordinary turn,
    without the row.
- It is best effort and not stored. If the turn ended just before, or the agent's runtime is
  reconnecting, the page shows "No turn is running" (or the turn simply continues) and nothing is
  sent later. Use **Stop run** if the agent does not react.

From the status menu in the details:

| Action | Effect |
| --- | --- |
| **Stop run** (also the button in the run row under *Runs*) | Ends the current run; the agent finalizes (pushes, snapshots) and the task waits for your next message. |
| **Mark done** | The task becomes `completed` and its pull request is marked ready for review (a background job, retried if the forge is down). |
| **Cancel task** | Stops the run, discards queued messages and closes pending requests. The pull request stays as it is. A new message reopens the task. |
| **Reset workspace** | Deletes the task's volume (not possible while a run is active). Committed work and the WIP snapshot stay in Git; the next run clones again. |

All of them, and Interrupt (`run.interrupt`), are audited with your name and need the same permission.

## Where the work is

| Where | What |
| --- | --- |
| `factory/task/<KEY>` | Commits of the agent, pushed after every turn |
| `factory/wip/<KEY>` | Snapshot of uncommitted files at the end of each run (secrets such as `.env` and keys excluded) |
| Draft pull request | Opened on the first commit ahead of the base branch; marked ready when the task completes. Forge failures are retried for an hour |
| Task volume | Cache of the checkout between runs; deleted 7 days after the task closed or after 30 days without a run |
