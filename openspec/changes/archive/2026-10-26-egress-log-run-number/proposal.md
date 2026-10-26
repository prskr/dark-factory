# Proposal

## Why

The network traffic page names a sandbox by the first 8 characters of its run UUID (`a4c2e7f0`). That value appears nowhere else:

- the task page and the transcript name runs `#1`, `#2`;
- the Sandbox custom resource is named `run-<task key>-<run number>` (`run-df-7-1`).

A board member cannot get from `a4c2e7f0` to the run or its sandbox without querying the database. The run number links all three.

## What Changes

- The sandbox registry that the control plane sends to the gateways carries each run's number next to its id. The flow events copy it.
- The traffic page names a sandbox by task key and run number:
  - graph node: `#1` with `DF-7`;
  - table and details panel: `DF-7 #1`.
- The run UUID stays the internal key: the graph edges, the dimming and the row ids keep using it.
- A connection from an unknown source still shows its source IP.
- The graph keeps both node columns apart at every divider width: destination nodes narrow down to 120 px before the gutter shrinks under 60 px.
- Both protobuf additions are new fields. An older gateway or viewer ignores them; an event without a number (number 0) falls back to the short run id.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `egress-log`: flow events carry the run number; graph nodes, the events table and the details panel show `#N` with the task key instead of the short run id.
- `egress-policy-distribution`: the sandbox registry carries the run number.

## Impact

- `contract/proto/darkfactory/gateway/v1/gateway.proto`: `SandboxEntry.run_number` and `FlowEvent.run_number` (`uint32`, new field numbers).
- `crates/df-db/src/egress.rs`: `active_runs` also selects `r.number`; regenerate `.sqlx/`.
- `crates/df-gateway`: copy the number into the snapshot entry and into `Flow::start`.
- `crates/df-web/src/egress/log/`: sandbox label in graph, rows and details.
- `crates/df-web/src/egress/log/graph.rs`: destination width as a CSS `clamp()` on a size container; `docs/design-system.md` graph text.
- `crates/dark-factory` demo flows: events carry run numbers.
- Depends on `network-management-ui-polish`, whose egress-log requirements this change modifies. Archive that change first.
