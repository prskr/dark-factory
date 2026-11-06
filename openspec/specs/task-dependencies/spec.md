# task-dependencies Specification

## Purpose
Lets a task wait for other tasks of its project before its first start: on their Completion or on their being Done, so work handed between agents runs in order without a human starting it.

## Requirements

### Requirement: Dependencies given at creation
A task SHALL be creatable with a list of dependencies, each naming a target task and a kind, `completion` or `done`. Targets SHALL be existing tasks of the same project, each named at most once. Dependencies SHALL NOT change after creation. Invalid dependencies SHALL reject the creation with an error naming the offending entry, and no task SHALL be created.

#### Scenario: Two dependencies
- **WHEN** a task is created in project `DF` with dependencies `DF-20` (`completion`) and `DF-22` (`done`)
- **THEN** the task stores both dependencies with their kinds

#### Scenario: Target in another project
- **WHEN** a task in project `DF` is created with a dependency on `OPS-3`
- **THEN** the creation is rejected with an error naming `OPS-3` and no task exists

#### Scenario: Unknown target
- **WHEN** a task is created with a dependency on `DF-999`, which doesn't exist
- **THEN** the creation is rejected with an error naming `DF-999`

#### Scenario: Same target twice
- **WHEN** a task is created with dependencies `DF-20` (`completion`) and `DF-20` (`done`)
- **THEN** the creation is rejected with an error naming `DF-20`

### Requirement: When a dependency is satisfied
A `completion` dependency SHALL be satisfied once its target has completed at least once (the agent called `complete_task` and the run ended with outcome `completed`) or has been Done. A `done` dependency SHALL be satisfied once its target has been Done: a board member marked it done or its PR was merged. A satisfied dependency SHALL stay satisfied, whatever the target does afterwards. A run that ends `completed` on a target that is already `cancelled` SHALL NOT count as a completion.

#### Scenario: Completion on a cancelled target
- **WHEN** `DF-20` is cancelled and its running run then ends with outcome `completed`
- **THEN** no dependency on `DF-20` is satisfied

#### Scenario: Completion satisfies completion
- **WHEN** `DF-20` ends a run with outcome `completed` and becomes `in_review`
- **THEN** a `completion` dependency on `DF-20` is satisfied and a `done` dependency on it is not

#### Scenario: Completed, then reopened
- **WHEN** `DF-20` completed once and a message reopened it, so it is `running` again
- **THEN** a `completion` dependency on `DF-20` is still satisfied

#### Scenario: Done satisfies both kinds
- **WHEN** a board member marks `DF-20` done before it ever completed
- **THEN** both a `completion` and a `done` dependency on `DF-20` are satisfied

#### Scenario: Failed target
- **WHEN** `DF-20` fails or is cancelled without having completed
- **THEN** no dependency on `DF-20` is satisfied

#### Scenario: PR closed without merge
- **WHEN** the PR of `DF-20` is closed without merge
- **THEN** a `done` dependency on `DF-20` stays unsatisfied

### Requirement: Waiting for dependencies
A task created with at least one unsatisfied dependency SHALL get status `awaiting_dependencies` and SHALL NOT start a run, provision a sandbox or volume, or create a task branch. A task created with all dependencies satisfied, or with none, SHALL get status `queued`.

#### Scenario: Unsatisfied dependency
- **WHEN** `DF-21` is created with a `completion` dependency on `DF-20`, which is `running` and has never completed
- **THEN** `DF-21` is `awaiting_dependencies` and has no run

#### Scenario: Already satisfied
- **WHEN** `DF-21` is created with a `completion` dependency on `DF-20`, which is `in_review`
- **THEN** `DF-21` is `queued` and its first run starts as for any new task

### Requirement: Dependents start once
When a dependency is satisfied, every task in `awaiting_dependencies` whose dependencies are now all satisfied SHALL become `queued`, in the same transaction as the event that satisfied it. A task SHALL leave `awaiting_dependencies` at most this once; a later Completion or Done of a target SHALL NOT start, wake or message a task that depends on it.

#### Scenario: Last dependency satisfied
- **WHEN** `DF-23` depends on `DF-20` (`completion`) and `DF-22` (`done`), `DF-22` is already Done, and `DF-20` completes
- **THEN** `DF-23` becomes `queued` and its first run starts

#### Scenario: One of two satisfied
- **WHEN** `DF-23` depends on `DF-20` and `DF-22` and only `DF-20` completes
- **THEN** `DF-23` stays `awaiting_dependencies`

#### Scenario: Target completes again
- **WHEN** reviewer `DF-21` ran after `DF-20` completed, ended, and `DF-20` completes a second time
- **THEN** `DF-21` gets no message and no run starts for it

#### Scenario: Dependent cancelled in the meantime
- **WHEN** `DF-21` waiting for `DF-20` was cancelled and `DF-20` then completes
- **THEN** `DF-21` stays `cancelled`

### Requirement: No webhooks needed for handover
Satisfying `completion` dependencies SHALL work without any forge webhook. A `done` dependency SHALL be satisfiable without webhooks by a board member marking the target done.

#### Scenario: Project without webhooks
- **WHEN** a project has no webhook configured, `DF-30` depends on `DF-29` (`done`) and a board member marks `DF-29` done
- **THEN** `DF-30` becomes `queued`

### Requirement: Human actions on a task waiting for dependencies
A board member SHALL be able to cancel or mark done a task in `awaiting_dependencies` under the same rules as for other statuses. Marking it done SHALL make it Done and so satisfy dependencies on it. A message queued on it SHALL NOT change its status and SHALL be delivered in its first run.

#### Scenario: Message while waiting
- **WHEN** a board member queues "use the new API" on `DF-21` in `awaiting_dependencies`
- **THEN** `DF-21` stays `awaiting_dependencies` and its first run receives the assignment prompt and the message

#### Scenario: Mark done while waiting
- **WHEN** a board member marks `DF-21` in `awaiting_dependencies` done and `DF-24` depends on `DF-21` (`done`)
- **THEN** `DF-21` is `completed` without ever running and `DF-24` becomes `queued`

#### Scenario: Cancel while waiting
- **WHEN** a board member cancels `DF-21` in `awaiting_dependencies`
- **THEN** `DF-21` is `cancelled` and its queued assignment prompt is discarded
