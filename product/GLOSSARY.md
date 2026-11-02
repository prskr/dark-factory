# Dark Factory

A single-tenant control plane that delegates tasks to coding agents running in Kubernetes sandboxes and lets humans follow and steer their work.

## Language

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

### Authors

**System principal**:
Dark Factory itself as the author of a prompt sent to the agent, such as the task assignment or a routed webhook.
_Avoid_: System event
