---
title: Plan work in a chat
linkTitle: Chats
weight: 5
---

A **chat** is a conversation with one persona in one project. Use it to think a piece of work
through, ask about the code and hand the result to agents as tasks. A chat has no branch and no
pull request: it reads the repository, talks, and creates tasks for personas that write code.

## Start a chat

Open **Chats** in the navigation rail (on a phone, the first entry of the bottom bar). The page shows
the project of the chat that was active last; the left column lists the projects with their number
of chats, then the chats of the selected project.

1. Choose **New chat** (the header button, or the round button on a phone).
2. The dialog shows the project (fixed), a message field and a persona picker. The picker lists
   every persona with its harness, model and description.
3. Write the first message and send. The chat gets the next key of the project, `DF-25-CHAT`: it
   shares the number counter with the tasks, so `DF-25-CHAT` and `DF-26` never clash.

The first message is yours, unchanged. The persona sets the **topic** of the chat in its first reply
(until then the list shows the start of your message in italics and
`<persona> sets the topic when the run starts`). You can rename the topic in the details (**Overview**, **Topic**); a name you set
is not replaced by the persona later.

A persona can have any number of chats in a project; agents cannot start chats.

## The chat list

Each entry shows the persona, the time of the last activity, the topic and the last message. The
status shows as a badge, except for an idle chat (`waiting`): after a reply it has no badge.

A chat that **needs you** (it has an open approval request) is listed first and carries an orange dot
instead of a badge; its project in the project list carries the dot as well. The other chats follow
by last activity, newest first. Answer the request on the chat's timeline or in
[Requests](requests.md); its link opens the chat page.

From 768 px the left column collapses to a strip with one avatar per persona that has a chat in the
project. An avatar opens the persona's chat that needs you, otherwise its most recently active chat.
The details column on the right collapses to a strip as on the task page. Both choices are
remembered in this browser.

## Talk to the persona

The center shows the transcript, rendered like a task's (tool call groups, thinking rows and tool
details included, see [Reading the transcript](tasks.md#reading-the-transcript)), and the message
box. Messages reach the
persona at the next turn boundary; **Stop** (in the run card of the details) stops the active run
and the message box offers **Interrupt** while a turn runs. A chat has no **Cancel** and no **Mark
done**: it is never completed.

A chat run differs from a task run:

- **Read-only checkout.** The workspace is the project's base branch, reset to the remote base
  branch at the start of every run. Edits and untracked files of an earlier run are gone, and the
  Git proxy rejects every push of a chat run (`df:read-only`). There is no task branch, no WIP
  branch and no pull request.
- **Chat instructions.** The persona's system prompt is followed by fixed instructions that say
  this: it talks to a board member, the checkout is read-only, work that needs a branch goes into
  tasks, and the chat is never completed.
- **Tools.** A chat run is offered `set_task_title`, `report_progress`, `create_task`,
  `list_personas`, `send_message`, `request_approval` and the [artifact](artifacts.md) tools. It is
  not offered `complete_task`, `ask_question`, `update_pull_request`, the pull request and issue
  tools; a call to one is answered with an error and changes nothing.
- **Context.** When a run cannot resume the harness session of the run before and starts a new one
  with the summary of the transcript, the transcript shows a **context_compacted** notice (a bot
  icon, one line). Expand it to read the summary the session started with. Tasks show the same
  notice.

## Delegate work

The persona hands work to other personas with `create_task`; the created tasks have the chat as
their parent ([Delegation](delegation.md)). The transcript shows each call as
`Created DF-20 · <title> · <persona>`. The **Subtasks** tab of the details lists the direct subtasks with their
status and "0 of 2 done", and the task details show the chat as the parent
(`DF-12-CHAT · Linus · Task dependencies plan`, linked to the chat). When a subtask ends, the chat
is woken with a message, as a task parent is.

## Artifacts

A chat's [artifacts](artifacts.md) are in the **Artifacts** tab, one card each, scrolling below the
tab bar. A Markdown artifact opens next to the transcript from its row in the transcript or from
its card; drag the divider to resize the two parts. On a phone the artifact opens full screen with a
way back to the chat.

## Archive a chat

A finished conversation is archived, not completed. **Archive** (in the details next to the
status) records who and when, keeps the chat's status and takes it out of the list. It is not
possible while the chat is `queued` or `running`, or has an open approval request: the button is
disabled and says why.

- **A run in its keep-alive window is released.** If the agent still waits for your next message,
  archiving ends that run and frees its chat slot, as an expired keep-alive does. Archiving keeps
  the status, but a message queued while the chat is archived (a finished subtask, for example)
  changes it: an archived chat whose idle run is still connected shows `running` until the
  released run ends, then `queued`. No run starts until you unarchive it, and that run starts a
  new sandbox.
- **Subtasks are not touched.** A running subtask keeps running. When one ends, its message is
  still queued on the archived chat, but no run starts.
- **An archived chat takes no messages from you.** Its page shows **Unarchive** in place of the
  message box.
- **Unarchive** clears the record. If messages wait, the chat becomes `queued` and a run delivers
  them.
- Chats are never deleted: transcripts, artifacts and the parent links of subtasks stay.

**Archived chats** at the end of the list opens the archive of the project: a searchable list (topic,
persona and messages) with persona, topic, key, last message, number of artifacts and archive
date. Selecting a chat turns the list into a sidebar next to the chat's **Transcript** (read-only)
and **Artifacts** (cards, several per row; opening one shows it rendered, with a way back to the
cards). **Unarchive** closes the modal and opens the chat. The volume of an archived chat is
deleted after `volumes.retainAfterCloseSeconds`, like that of a closed task.

## Limits

Chat runs count against `runs.maxConcurrentChats` (default 2), not against `runs.maxConcurrent`: a
chat does not wait behind long task runs. When a chat is queued and both chat slots are taken, the
chat run that has been idle the longest after a turn is released. Its chat keeps its status and the
next message starts a new run, which resumes the harness session. See the
[configuration reference](../reference/agent-channel/configuration.md).
