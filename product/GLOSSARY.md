# Dark Factory

A single-tenant control plane that delegates tasks to coding agents running in Kubernetes sandboxes and lets humans follow and steer their work.

## Language

### Tasks

**Completion**:
The agent's declaration that its work on a task is finished for now; the task then waits for review. A task can complete several times.
_Avoid_: Done, finished

**Done**:
The final state of a task whose pull request was merged or that a board member marked done.
_Avoid_: Completed, closed, merged

**Parent task**:
The task another task reports to: it is woken whenever that task ends and receives its escalated questions.
_Avoid_: Creator, owner, supervisor

**Question**:
A free-text request from an agent for a decision, addressed to its parent task's agent or to a human. Answering it only delivers the answer to the asking agent. A question to a parent task that has reached a final state, or to an archived chat, goes to a human instead.
_Avoid_: Escalation request, query, ask

**Subtask**:
A task whose parent task is set.
_Avoid_: Child task, sub-task

**Base task**:
The task whose branch another task's branch starts from, so that task's workspace contains the base task's work.
_Avoid_: Source task, upstream task

**Dependency**:
A condition under which a task may start for the first time: another task of the same project reaching its first Completion, or reaching Done.
_Avoid_: Blocker, blocked by, prerequisite

**Delegation**:
An agent handing part of its work to another persona by creating a task whose parent task is its own.
_Avoid_: Sub-agent, hand-off

### Chats

**Chat**:
A conversation of a human with one persona in one project. Its agent can create tasks and is then their parent task. A persona can have several chats in a project.
_Avoid_: Conversation, session, thread

**Topic**:
The title of a chat, set by its agent in the first run and renamable by a human.
_Avoid_: Chat title, subject

**Archived chat**:
A chat a human set aside: read-only and without runs until a human unarchives it. Its subtasks go on.
_Avoid_: Closed chat, completed chat, deleted chat

### Transcript

**Transcript**:
The ordered history of one task across all its runs.
_Avoid_: Log, session, chat history

**Transcript event**:
One entry of a transcript, written either by the agent runtime or by the control plane.
_Avoid_: Message, row

**Notice**:
A transcript event in which the agent runtime reports something that is not model output, such as a diverged task branch.
_Avoid_: System event, warning

**Forge activity**:
A transcript event in which the control plane records that the task's work reached the forge: a task-branch push (accepted, rejected, or failed at the forge), a pull request opened, or a pull request marked ready for review. It is shown to humans only, never to the agent.
_Avoid_: System event, Git event, notice

### Results

**Artifact**:
A named file that belongs to one task (chats included), written by its agent for humans or for related tasks. It is either text, such as a Markdown document, or an image, such as a screenshot of the running app.
_Avoid_: Attachment, document, upload

### Pull request feedback

**PR comment**:
A top-level comment in the conversation of a pull request, not anchored to any line of the change.
_Avoid_: Issue comment, conversation comment

**Review**:
One submitted batch of feedback on a pull request with a verdict (approve, request changes, or comment) and an optional body.
_Avoid_: Approval, code review

**Review comment**:
A comment anchored to a file and line of a pull request's change, belonging to a review.
_Avoid_: Inline comment, line comment

**Review thread**:
A review comment together with its replies; it can be resolved.
_Avoid_: Conversation, discussion

### Authors

**System principal**:
Dark Factory itself as the author of a prompt sent to the agent, such as the task assignment or a routed webhook.
_Avoid_: System event

### Integrations

**Lifecycle event**:
A notification that something happened to a task's sandbox or volume, such as a sandbox being provisioned or destroyed, sent by Dark Factory to external systems after the fact. It never influences the run.
_Avoid_: Webhook, hook, outgoing webhook

**Event subscription**:
A receiver URL registered for lifecycle events, either for the whole instance or for one project.
_Avoid_: Webhook, hook, endpoint

**Webhook**:
An HTTP request a forge sends to Dark Factory. Notifications Dark Factory sends are lifecycle events, never webhooks.
_Avoid_: Incoming webhook, forge event

### Network

**Egress rule**:
A board member's decision about which external hosts sandboxes may reach. It is either an allow rule or a deny rule.
_Avoid_: Allowlist entry, firewall rule

**Allow rule**:
An egress rule that lets the runs within its scope (instance, project or task) connect to a host.
_Avoid_: Allowlist entry, grant, whitelist

**Deny rule**:
An instance-wide egress rule naming a host that will never be allowed, so no network access request is raised for it. An allow rule still lets a connection through.
_Avoid_: Silencer, blocklist, blacklist, mute

### Models

**Model provider**:
An upstream model service together with the one credential Dark Factory uses for it. Two accounts at the same service are two model providers. A persona uses exactly one.
_Avoid_: Connection, provider account, LLM backend
