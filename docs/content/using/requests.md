---
title: Requests
weight: 3
---

Agents ask before they guess, and they cannot grant themselves anything. Both show up as
**requests**: in the **Requests** inbox (the count is on every page) and as cards on the task's
timeline.

| Kind | Raised by | You decide |
| --- | --- | --- |
| Question | the agent, of one [kind](#question-kinds) | choose an option or write an answer |
| `network-access` approval | the gateway when a host is blocked, or the agent with a reason | approve for the task, the project or everywhere; deny; or **Deny always** ([Network access](network-access.md)) |
| `push-check-exception` approval | the Git proxy when a push hits findings in *approval* mode | approve exactly these findings for this task, or deny ([Git proxy](../security/git-proxy.md#push-checks)) |
| `create-issue` approval | the agent of a [chat](chats.md#file-and-list-issues), with a title, a body and labels | read the issue under **Preview** and **Markdown**, then approve or deny; approving creates exactly that issue |

## Question kinds

Every question has a kind that sets the input on its card. The card header shows it
("Question · Single choice").

| Kind | The agent offers | You answer |
| --- | --- | --- |
| Single choice | 2 to 10 options | one option, or Other |
| Multiple choice | 2 to 10 options, optionally a minimum and a maximum number of answers | several options; Other counts as one of them |
| Text | nothing | a Markdown text of up to 4,000 characters, with **Write** and **Preview** tabs |

An option has a label (one line, up to 200 characters) and optionally a description (up to 300).
Both are plain text. The question itself is Markdown.

- **Other.** A choice question also offers **Other** as its last row with a text input (up to 1,000
  characters), unless the agent asked for a strict choice. Choose the Other row and write your own
  answer. Text in the Other input is ignored while the row is not chosen.
- **Limits.** A multiple choice question shows its limit above the options, for example "Choose one
  or more", "Choose up to 3" or "Choose 2 to 3".
- **Submit.** Nothing is preselected. **Answer** sends the form; a single choice needs one option
  or Other, and the button.
- **Errors.** The server checks the answer. A rejected answer keeps what you entered and shows the
  reason under the input, for example "Choose at most 2 options.", "Write your Other answer." or
  "Write an answer.". Nothing is stored until the answer is valid.

An answered question lists every option read-only: the chosen ones are marked, the others dimmed,
and "Other: ..." follows when you wrote one. A text answer is shown as rendered Markdown. Below it
the card shows who answered and when.

### What the agent receives

The answer reaches the agent as one message at its next turn, in three parts: a line naming the
question and who answered, a JSON block with the structured answer, and the readable answer.

````text
Request 3f2a... (your question "Which PostgreSQL version ...") was answered by Mara Okafor. Structured answer:

```json
{"request_id":"3f2a...","kind":"multi_choice","selected":[{"id":"api","label":"API"}],"other":"also the CLI"}
```

Answer: API, Other: also the CLI
````

A choice answer has `selected` (the chosen options with `id` and `label`, in the order of the
question) and `other` (the text, or `null`). A text answer has `text`. The agent reads the JSON
block; the readable answer is for people who show [hidden messages](#what-happens-on-a-decision).

## Blocking and non-blocking

Questions are **blocking** by default: the agent ends its turn and waits for the answer. If nobody
answers within the idle deadline, the run ends and the task becomes `blocked`; your answer later
queues the task again and the next run continues with it. An agent sets **non-blocking** only when
it can carry on with a fallback and revisit it later; then the answer arrives as a message at a later
turn. Approval requests are non-blocking by default.

## What happens on a decision

- The control plane applies an approved action itself (it creates the egress rule or the push
  exception); the agent never does.
- The decision reaches the agent as a message at its next turn, including your comment.
- Identical pending requests of a task are merged (a question by its text: asking the same text
  again returns the pending question with its original kind and options); a task can have at most 20 pending requests.
- Completing or cancelling a task closes its pending requests.
- Raising, deciding and closing are audited.

A request of a [chat](chats.md) is listed like any other; its link opens the chat page, and the
chat is marked as needing you until the request is decided. A message you write in the chat does
not answer or close a pending question.

The request card on the task timeline keeps the answer or approval outcome and who decided it.
The decision message is hidden in the transcript and queued-message stack, so the answer does not
appear a second time. Its full content still reaches the agent.

For prompt diagnostics, an operator can enable `web.showHiddenBlocks` (optional, default `false`;
[instance configuration](../reference/agent-channel/configuration.md)). Hidden decision messages
then appear labeled **Instruction for the agent**, with the author **Request decision**.

Request texts (questions and approval reasons) come from the agent and are rendered as Markdown with
the same rules as [agent messages](tasks.md#agent-messages-markdown-code-and-diagrams): HTML in the
text is shown as text, images appear as links, unsafe links are dropped, fenced code is highlighted
and `mermaid` fences are drawn as diagrams in an isolated frame.
